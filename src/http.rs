use crate::{
    backend::SourceKind,
    error::{AppError, ErrorCode},
    live::auth::{TokenProvider, auth_error},
    model::now,
    store::{RateLimit, Store},
};
use reqwest::{Url, blocking::Client, header::HeaderMap};
use serde_json::Value;
use std::{
    env,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_WAIT: Duration = Duration::from_secs(15);
const MAX_RETRIES: u32 = 2;
pub const LISTING_TTL: i64 = 300;
pub const THREAD_TTL: i64 = 600;
pub const PROFILE_TTL: i64 = 900;
const ARCHIVE_TTL: i64 = 3600;

#[derive(Clone, Copy, Default)]
pub struct CachePolicy {
    pub offline: bool,
    pub refresh: bool,
    pub no_cache: bool,
}

pub struct Endpoints {
    live: Url,
    archive: Url,
}

impl Endpoints {
    pub fn from_env() -> Result<Self, AppError> {
        Ok(Self {
            live: endpoint_from_env("REDDIT_CLI_OAUTH_BASE", "https://oauth.reddit.com")?,
            archive: endpoint_from_env(
                "REDDIT_CLI_ARCTIC_BASE",
                "https://arctic-shift.photon-reddit.com/api",
            )?,
        })
    }

    pub fn new(live: &str, archive: &str) -> Result<Self, AppError> {
        Ok(Self {
            live: checked_url(live, "live API base")?,
            archive: checked_url(archive, "archive API base")?,
        })
    }

    fn base(&self, source: SourceKind) -> &Url {
        match source {
            SourceKind::Live => &self.live,
            SourceKind::Archive => &self.archive,
        }
    }

    pub fn host(&self, source: SourceKind) -> &str {
        self.base(source)
            .host_str()
            .expect("endpoint host was validated")
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    pub body: Value,
    pub cached: bool,
    pub fetched_at: i64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Observation {
    pub cached: bool,
    pub fetched_at: Option<i64>,
}

struct ArchiveGate {
    active: Mutex<usize>,
    ready: Condvar,
}

struct ArchivePermit<'a>(&'a ArchiveGate);

impl Drop for ArchivePermit<'_> {
    fn drop(&mut self) {
        let mut active = self.0.active.lock().expect("archive request lock poisoned");
        *active -= 1;
        self.0.ready.notify_one();
    }
}

pub struct HttpClient {
    client: Client,
    store: Arc<Store>,
    policy: CachePolicy,
    endpoints: Endpoints,
    observation: Mutex<Observation>,
    archive: ArchiveGate,
}

impl HttpClient {
    pub fn new(
        store: Arc<Store>,
        policy: CachePolicy,
        endpoints: Endpoints,
        user_agent: &str,
    ) -> Result<Self, AppError> {
        Ok(Self {
            client: transport(user_agent)?,
            store,
            policy,
            endpoints,
            observation: Mutex::new(Observation::default()),
            archive: ArchiveGate {
                active: Mutex::new(0),
                ready: Condvar::new(),
            },
        })
    }

    /// GET an API-relative path on the configured host. Pass query values separately.
    /// Live adds raw_json and bearer auth, with one 401 refresh; archive ignores tokens
    /// and uses its own TTL. Both sources share retry, rate-limit and cache handling.
    pub fn get(
        &self,
        source: SourceKind,
        path: &str,
        query: &[(&str, &str)],
        ttl: i64,
        tokens: Option<&TokenProvider>,
    ) -> Result<HttpResponse, AppError> {
        if !path.starts_with('/')
            || path.contains(['?', '#', '\\'])
            || path.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err(AppError::usage("invalid API path"));
        }
        let mut url = self.endpoints.base(source).clone();
        url.set_path(&format!("{}{path}", url.path().trim_end_matches('/')));
        let mut query = query.to_vec();
        if source == SourceKind::Live {
            query.retain(|(key, _)| *key != "raw_json");
            query.push(("raw_json", "1"));
        }
        query.sort_unstable();
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let key = format!("{}|GET|{url}", source.as_str());
        let read = !self.policy.no_cache && (self.policy.offline || !self.policy.refresh);
        if read && let Some(entry) = self.store.read_cache(&key, self.policy.offline)? {
            let response = HttpResponse {
                body: parse_body(&entry.body)?,
                cached: true,
                fetched_at: entry.fetched_at,
            };
            self.observe(&response);
            return Ok(response);
        }
        if self.policy.offline {
            return Err(AppError::new(
                ErrorCode::EOfflineMiss,
                "request is absent from the local cache",
            )
            .with_hint("Run the command online to populate the cache."));
        }
        let tokens = match source {
            SourceKind::Live => {
                Some(tokens.ok_or_else(|| auth_error("OAuth credentials are required"))?)
            }
            SourceKind::Archive => None,
        };
        let mut request = Request {
            source,
            url,
            tokens,
            token: None,
            refreshed: false,
            retries: 0,
            archive_timeout_retried: false,
        };
        let response = self.fetch(&mut request)?;
        let fetched_at = now();
        let body = parse_body(&response)?;
        if !self.policy.no_cache {
            let ttl = if source == SourceKind::Archive {
                ARCHIVE_TTL
            } else {
                ttl
            };
            self.store.write_cache(&key, &response, fetched_at, ttl)?;
        }
        let response = HttpResponse {
            body,
            cached: false,
            fetched_at,
        };
        self.observe(&response);
        Ok(response)
    }

    pub fn observation(&self) -> Observation {
        *self
            .observation
            .lock()
            .expect("HTTP observation lock poisoned")
    }

    pub fn rate_limit(&self, source: SourceKind) -> Result<Option<RateLimit>, AppError> {
        self.store.rate_limit(self.endpoints.host(source))
    }

    fn observe(&self, response: &HttpResponse) {
        let mut observation = self
            .observation
            .lock()
            .expect("HTTP observation lock poisoned");
        observation.cached =
            response.cached && (observation.cached || observation.fetched_at.is_none());
        observation.fetched_at = Some(
            observation
                .fetched_at
                .map_or(response.fetched_at, |old| old.min(response.fetched_at)),
        );
    }

    fn fetch(&self, request: &mut Request<'_>) -> Result<Vec<u8>, AppError> {
        let host = self.endpoints.host(request.source);
        loop {
            self.wait_for_rate_limit(host)?;
            let mut outgoing = self.client.get(request.url.clone());
            if let Some(tokens) = request.tokens {
                if request.token.is_none() {
                    request.token = Some(tokens.token(None)?);
                }
                outgoing =
                    outgoing.bearer_auth(request.token.as_deref().expect("token was resolved"));
            }
            let result = {
                let _permit =
                    (request.source == SourceKind::Archive).then(|| self.archive.acquire());
                outgoing.send().map(|response| {
                    let status = response.status().as_u16();
                    let headers = response.headers().clone();
                    (status, headers, response.bytes())
                })
            };
            let (status, headers, body) = match result {
                Ok((status, headers, body)) => {
                    let rate = rate_from_headers(host, &headers);
                    if let Some(rate) = &rate {
                        self.store.record_rate_limit(rate)?;
                    }
                    match body {
                        Ok(body) => (status, headers, body),
                        Err(error) if error.is_timeout() || error.is_body() => {
                            request.retry_network()?;
                            continue;
                        }
                        Err(_) => return Err(network_error()),
                    }
                }
                Err(error) if error.is_timeout() || error.is_connect() || error.is_request() => {
                    request.retry_network()?;
                    continue;
                }
                Err(_) => return Err(network_error()),
            };

            if status == 401
                && !request.refreshed
                && let Some(tokens) = request.tokens
            {
                request.token = Some(tokens.token(request.token.as_deref())?);
                request.refreshed = true;
                continue;
            }
            if matches!(status, 429 | 503) {
                let wait = retry_after(&headers)
                    .or_else(|| {
                        reset_at(&headers).map(|reset| {
                            Duration::from_secs(reset.saturating_sub(now()).max(0) as u64)
                        })
                    })
                    .unwrap_or_else(|| backoff(request.retries));
                if request.retry(wait) {
                    continue;
                }
                if status == 429 {
                    return Err(rate_error(wait, Some(status)));
                }
            }
            if request.source == SourceKind::Archive
                && let Some(message) = archive_error(&body)
            {
                if message.contains("Timeout") {
                    if !request.archive_timeout_retried {
                        request.archive_timeout_retried = true;
                        thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                    let mut error = AppError::upstream(message)
                        .with_status(status)
                        .with_hint("Narrow --since/--until or add --sub/--author.");
                    error.retryable = true;
                    return Err(error);
                }
                if matches!(status, 400 | 422) {
                    return Err(AppError::usage(message).with_status(status));
                }
                if (200..300).contains(&status) {
                    return Err(AppError::upstream(message).with_status(status));
                }
            }
            match status {
                200..=299 => return Ok(body.to_vec()),
                401 | 403 if request.source == SourceKind::Live => {
                    return Err(live_denied(status, &body));
                }
                status
                    if matches!(status, 403 | 404)
                        || (request.source == SourceKind::Live && (300..400).contains(&status)) =>
                {
                    return Err(
                        AppError::new(ErrorCode::ENotFound, "upstream resource not found")
                            .with_status(status),
                    );
                }
                400 | 422 => {
                    return Err(
                        AppError::usage("upstream rejected the request").with_status(status)
                    );
                }
                _ => {
                    let mut error =
                        AppError::upstream("upstream request failed").with_status(status);
                    error.retryable = status >= 500;
                    return Err(error);
                }
            }
        }
    }

    fn wait_for_rate_limit(&self, host: &str) -> Result<(), AppError> {
        while let Some(rate) = self.store.rate_limit(host)? {
            let seconds = rate.reset_at.saturating_sub(now());
            if rate.remaining >= 1.0 || seconds <= 0 {
                break;
            }
            let wait = Duration::from_secs(seconds as u64);
            if wait > MAX_WAIT {
                return Err(rate_error(wait, None));
            }
            thread::sleep(wait);
        }
        Ok(())
    }
}

impl ArchiveGate {
    fn acquire(&self) -> ArchivePermit<'_> {
        let active = self.active.lock().expect("archive request lock poisoned");
        let mut active = self
            .ready
            .wait_while(active, |active| *active >= 4)
            .expect("archive request lock poisoned");
        *active += 1;
        ArchivePermit(self)
    }
}

struct Request<'a> {
    source: SourceKind,
    url: Url,
    tokens: Option<&'a TokenProvider>,
    token: Option<String>,
    retries: u32,
    refreshed: bool,
    archive_timeout_retried: bool,
}

impl Request<'_> {
    fn retry(&mut self, wait: Duration) -> bool {
        if self.retries >= MAX_RETRIES || wait > MAX_WAIT {
            return false;
        }
        self.retries += 1;
        thread::sleep(wait);
        true
    }

    fn retry_network(&mut self) -> Result<(), AppError> {
        if self.retry(backoff(self.retries)) {
            Ok(())
        } else {
            Err(network_error())
        }
    }
}

pub(crate) fn transport(agent: &str) -> Result<Client, AppError> {
    Client::builder()
        .user_agent(agent)
        .timeout(MAX_WAIT)
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()
        .map_err(|_| {
            AppError::new(
                ErrorCode::EConfig,
                "cannot initialize HTTP client; check the User-Agent",
            )
        })
}

pub(crate) fn endpoint_from_env(key: &str, default: &str) -> Result<Url, AppError> {
    let Some(raw) = env::var_os(key) else {
        return checked_url(default, key);
    };
    let url = checked_url(&raw.to_string_lossy(), key)?;
    // Test endpoints must never redirect credentials or requests to an outside host.
    if !is_loopback(&url) {
        return Err(AppError::new(
            ErrorCode::EConfig,
            format!("{key} must point to a loopback host"),
        ));
    }
    Ok(url)
}

fn is_loopback(url: &Url) -> bool {
    matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

fn checked_url(raw: &str, label: &str) -> Result<Url, AppError> {
    let invalid = || AppError::new(ErrorCode::EConfig, format!("invalid {label} URL"));
    let url = Url::parse(raw).map_err(|_| invalid())?;
    if url.host().is_none()
        || (url.scheme() != "https" && !(url.scheme() == "http" && is_loopback(&url)))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    Ok(url)
}

fn parse_body(body: &[u8]) -> Result<Value, AppError> {
    serde_json::from_slice(body).map_err(|_| AppError::upstream("upstream returned invalid JSON"))
}

/// Classifies a live 401/403 that survived the token refresh. Reddit uses 403 for three
/// different things, and only one of them means the credentials are the problem:
/// - an HTML page: its edge blocked the request before the API saw it;
/// - JSON with a `reason` (`private`, `banned`, `quarantined`, ...): the subreddit is
///   not readable, which the spec treats as not found;
/// - anything else: OAuth was refused, e.g. the app lost Data API access.
fn live_denied(status: u16, body: &[u8]) -> AppError {
    if body.trim_ascii_start().starts_with(b"<") {
        return AppError::upstream("Reddit's edge returned an HTML block page instead of the API")
            .with_status(status)
            .with_hint("Retry later, or use --source archive.");
    }
    if let Some(reason) = parse_body(body)
        .ok()
        .and_then(|json| json.get("reason")?.as_str().map(str::to_owned))
    {
        return AppError::new(
            ErrorCode::ENotFound,
            format!("Reddit refused access to this resource ({reason})"),
        )
        .with_status(status);
    }
    auth_error("Reddit denied OAuth access. The app may have lost Data API access.")
        .with_status(status)
}

fn archive_error(body: &[u8]) -> Option<String> {
    parse_body(body)
        .ok()?
        .get("error")?
        .as_str()
        .map(str::to_owned)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

fn reset_at(headers: &HeaderMap) -> Option<i64> {
    let reset: f64 = header(headers, "x-ratelimit-reset")?.parse().ok()?;
    if !reset.is_finite() || reset < 0.0 {
        return None;
    }
    let seconds = reset.ceil() as i64;
    // Reddit returns seconds until reset. Archive may use a Unix timestamp.
    if seconds >= 1_000_000_000 {
        Some(seconds)
    } else {
        now().checked_add(seconds)
    }
}

fn rate_from_headers(host: &str, headers: &HeaderMap) -> Option<RateLimit> {
    let remaining: f64 = header(headers, "x-ratelimit-remaining")?.parse().ok()?;
    remaining.is_finite().then_some(RateLimit {
        host: host.into(),
        remaining,
        reset_at: reset_at(headers)?,
        window: header(headers, "x-ratelimit-window").map(str::to_owned),
    })
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let value = header(headers, "retry-after")?;
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = httpdate::parse_http_date(value).ok()?;
    Some(date.duration_since(SystemTime::now()).unwrap_or_default())
}

fn backoff(retries: u32) -> Duration {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let percent = 80 + u64::from(nanos % 41);
    Duration::from_millis((500 << retries) * percent / 100)
}

fn rate_error(wait: Duration, status: Option<u16>) -> AppError {
    let mut error = AppError::new(ErrorCode::ERateLimited, "upstream rate limit reached");
    error.retryable = true;
    error.http_status = status;
    error.retry_after = Some(
        wait.as_secs()
            .saturating_add(u64::from(wait.subsec_nanos() > 0)),
    );
    error
}

fn network_error() -> AppError {
    let mut error = AppError::new(
        ErrorCode::ENetwork,
        "upstream connection failed or timed out",
    );
    error.retryable = true;
    error
}
