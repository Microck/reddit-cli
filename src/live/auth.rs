use crate::{
    config::{Credentials, Paths, read_private, remove_if_present, write_private},
    error::{AppError, ErrorCode},
    http::{endpoint_from_env, transport},
    model::now,
};
use reqwest::{Url, blocking::Client};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const TOKEN_URL: &str = "https://www.reddit.com/api/v1/access_token";

#[derive(Deserialize, Serialize)]
struct Token {
    access_token: String,
    client_id: String,
    expires_at: i64,
    scope: Option<String>,
}

#[derive(Serialize)]
pub struct Login {
    pub ok: bool,
    pub scope: Option<String>,
    pub expires_in: i64,
}

pub struct TokenProvider {
    paths: Paths,
    credentials: Credentials,
    url: Url,
    client: Client,
    access: Mutex<()>,
}

impl TokenProvider {
    pub fn new(paths: Paths, credentials: Credentials) -> Result<Self, AppError> {
        let url = endpoint_from_env("REDDIT_CLI_TOKEN_URL", TOKEN_URL)?;
        let client = transport(&credentials.user_agent)?;
        Ok(Self {
            paths,
            credentials,
            url,
            client,
            access: Mutex::new(()),
        })
    }

    /// Read a valid token for these credentials, or refresh it when needed.
    /// Pass the token rejected by a 401 so parallel requests can reuse one refresh.
    pub fn token(&self, rejected: Option<&str>) -> Result<String, AppError> {
        let _access = self.access.lock().expect("token lock poisoned");
        if let Some(token) = read_token(&self.paths)?
            && token.valid_for(&self.credentials)
            && rejected != Some(token.access_token.as_str())
        {
            return Ok(token.access_token);
        }
        let (token, _) = self.fetch()?;
        self.save(&token)?;
        Ok(token.access_token)
    }

    pub fn login(&self) -> Result<Login, AppError> {
        let _access = self.access.lock().expect("token lock poisoned");
        let (token, expires_in) = self.fetch()?;
        self.save(&token)?;
        Ok(Login {
            ok: true,
            scope: token.scope,
            expires_in,
        })
    }

    fn save(&self, token: &Token) -> Result<(), AppError> {
        let bytes = serde_json::to_vec(token).expect("token record serializes");
        write_private(&self.paths.token(), &bytes)
    }

    fn fetch(&self) -> Result<(Token, i64), AppError> {
        let response = self
            .client
            .post(self.url.clone())
            .basic_auth(
                &self.credentials.client_id,
                Some(&self.credentials.client_secret),
            )
            .form(&[("grant_type", "client_credentials")])
            .send()
            .map_err(|_| {
                let mut error =
                    AppError::new(ErrorCode::ENetwork, "cannot reach the token endpoint");
                error.retryable = true;
                error
            })?;
        let status = response.status().as_u16();
        let rejected = || {
            auth_error("the token endpoint rejected the credentials or returned an invalid token")
                .with_status(status)
        };
        if !response.status().is_success() {
            return Err(rejected());
        }
        // Error bodies can echo credentials. Only the HTTP status is safe to report.
        let reply: serde_json::Value = response.json().map_err(|_| rejected())?;
        if reply.get("error").is_some() {
            return Err(rejected());
        }
        let access_token = reply["access_token"]
            .as_str()
            .filter(|token| {
                !token.trim().is_empty() && token.bytes().all(|byte| byte.is_ascii_graphic())
            })
            .ok_or_else(rejected)?;
        let expires_in = reply["expires_in"]
            .as_i64()
            .filter(|&expires| expires > 0)
            .ok_or_else(rejected)?;
        let expires_at = now().checked_add(expires_in).ok_or_else(rejected)?;
        let scope = reply["scope"]
            .as_str()
            .filter(|scope| {
                !scope.contains(access_token) && !scope.contains(&self.credentials.client_secret)
            })
            .map(str::to_owned);
        Ok((
            Token {
                access_token: access_token.to_owned(),
                client_id: self.credentials.client_id.clone(),
                expires_at,
                scope,
            },
            expires_in,
        ))
    }
}

impl Token {
    fn valid_for(&self, credentials: &Credentials) -> bool {
        self.client_id == credentials.client_id
            && !self.access_token.is_empty()
            && self.expires_at.saturating_sub(now()) > 60
    }
}

fn read_token(paths: &Paths) -> Result<Option<Token>, AppError> {
    Ok(read_private(&paths.token())?.and_then(|json| serde_json::from_str(&json).ok()))
}

pub fn token_valid(paths: &Paths, credentials: Option<&Credentials>) -> Result<bool, AppError> {
    Ok(match credentials {
        Some(credentials) => read_token(paths)?.is_some_and(|token| token.valid_for(credentials)),
        None => false,
    })
}

pub fn logout(paths: &Paths) -> Result<(), AppError> {
    remove_if_present(&paths.token())
}

pub fn auth_error(message: impl Into<String>) -> AppError {
    let mut error = AppError::new(ErrorCode::EAuth, message)
        .with_hint("Check OAuth credentials with reddit auth status, or try --source archive.");
    error.suggested_commands.push("reddit auth status".into());
    error
}
