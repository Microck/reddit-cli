use reddit_research::config::Paths;
use reqwest::Url;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tempfile::TempDir;
use tiny_http::{Header, Method, Response, Server, StatusCode};

pub struct State {
    _dir: TempDir,
    pub paths: Paths,
}

impl State {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("config"),
            data_dir: dir.path().join("data"),
        };
        Self { _dir: dir, paths }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reddit"));
        command
            .env_remove("REDDIT_CLIENT_ID")
            .env_remove("REDDIT_CLIENT_SECRET")
            .env_remove("REDDIT_USER_AGENT")
            .env_remove("REDDIT_CLI_FORMAT")
            .env_remove("REDDIT_CLI_SOURCE")
            .env("REDDIT_CLI_CONFIG_DIR", &self.paths.config_dir)
            .env("REDDIT_CLI_DATA_DIR", &self.paths.data_dir)
            .env("REDDIT_CLI_TOKEN_URL", "http://127.0.0.1:9/token")
            .env("REDDIT_CLI_OAUTH_BASE", "http://127.0.0.1:9")
            .env("REDDIT_CLI_ARCTIC_BASE", "http://127.0.0.1:9/api");
        command
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
}

pub fn with_stdin(command: &mut Command, stdin: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

pub struct Route {
    method: Method,
    target: String,
    body: FixtureBody,
    status: u16,
    headers: Vec<(&'static str, &'static str)>,
}

enum FixtureBody {
    File(&'static str),
    Inline(Vec<u8>),
}

impl Route {
    pub fn new(method: Method, target: &str, fixture: &'static str, status: u16) -> Self {
        Self {
            method,
            target: normalize(target),
            body: FixtureBody::File(fixture),
            status,
            headers: Vec::new(),
        }
    }

    pub fn inline(method: Method, target: &str, body: impl Into<Vec<u8>>, status: u16) -> Self {
        Self {
            method,
            target: normalize(target),
            body: FixtureBody::Inline(body.into()),
            status,
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((name, value));
        self
    }
}

pub struct Recorded {
    pub method: Method,
    pub target: String,
    pub body: String,
    pub headers: Vec<Header>,
}

pub struct Fixtures {
    pub base: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Fixtures {
    pub fn serve(mut routes: Vec<Route>) -> Self {
        let server = Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let requests = Arc::clone(&requests);
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let Some(mut request) = server.recv_timeout(Duration::from_millis(50)).unwrap()
                    else {
                        continue;
                    };
                    let method = request.method().clone();
                    let target = normalize(request.url());
                    let route = routes
                        .iter()
                        .position(|route| route.method == method && route.target == target);
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).unwrap();
                    requests.lock().unwrap().push(Recorded {
                        method,
                        target,
                        body,
                        headers: request.headers().to_vec(),
                    });
                    if let Some(index) = route {
                        let route = routes.remove(index);
                        let body = match route.body {
                            FixtureBody::File(fixture) => {
                                let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                                    .join("tests/fixtures")
                                    .join(fixture);
                                fs::read(path).unwrap()
                            }
                            FixtureBody::Inline(body) => body,
                        };
                        let mut response =
                            Response::from_data(body).with_status_code(StatusCode(route.status));
                        for (name, value) in route
                            .headers
                            .into_iter()
                            .chain([("Content-Type", "application/json")])
                        {
                            response.add_header(Header::from_bytes(name, value).unwrap());
                        }
                        request.respond(response).unwrap();
                    } else {
                        request.respond(Response::empty(StatusCode(404))).unwrap();
                    }
                }
            })
        };
        Self {
            base,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    pub fn requests(&self) -> std::sync::MutexGuard<'_, Vec<Recorded>> {
        self.requests.lock().unwrap()
    }
}

impl Drop for Fixtures {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn normalize(target: &str) -> String {
    let mut url = Url::parse(&format!("http://127.0.0.1{target}")).unwrap();
    let mut pairs: Vec<_> = url.query_pairs().into_owned().collect();
    pairs.sort();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    url.as_str()
        .trim_start_matches("http://127.0.0.1")
        .to_owned()
}
