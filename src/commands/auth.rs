use super::Output;
use crate::{
    backend::SourceKind,
    cli::AuthCommand,
    config::{AuthConfig, Config, Credentials, Paths, credentials, default_user_agent},
    error::{AppError, ErrorCode},
    http::Endpoints,
    live::auth::{TokenProvider, logout, token_valid},
    store::Store,
};
use serde_json::json;
use std::io::{self, BufRead, IsTerminal};

pub fn run(
    command: AuthCommand,
    paths: &Paths,
    mut config: Config,
    offline: bool,
) -> Result<Output, AppError> {
    let data = match command {
        AuthCommand::Status => {
            let (credentials, origins) = credentials(&config.auth);
            let source = SourceKind::select(config.source(None)?, credentials.is_some());
            let store = Store::open(paths, config.retention()?)?;
            let endpoints = Endpoints::from_env()?;
            json!({
                "credentials": origins,
                "source": source.as_str(),
                "token_valid": token_valid(paths, credentials.as_ref())?,
                "rate_limit": store.rate_limit(endpoints.host(source))?,
            })
        }
        AuthCommand::Login {
            client_id,
            client_secret_stdin,
            user_agent,
        } => {
            if offline {
                return Err(AppError::new(
                    ErrorCode::EOfflineMiss,
                    "auth login requires a token endpoint request",
                ));
            }
            let client_secret = read_secret(client_secret_stdin)?;
            if client_id.trim().is_empty()
                || client_id.starts_with('$')
                || client_secret.trim().is_empty()
                || client_secret.starts_with('$')
            {
                return Err(AppError::usage("client ID and secret must be set"));
            }
            let provider = TokenProvider::new(
                paths.clone(),
                Credentials {
                    client_id: client_id.clone(),
                    client_secret: client_secret.clone(),
                    user_agent: user_agent.clone().unwrap_or_else(default_user_agent),
                },
            )?;
            config.auth = AuthConfig {
                client_id: Some(client_id),
                client_secret: Some(client_secret),
                user_agent,
            };
            config.save(paths)?;
            logout(paths)?;
            json!(provider.login()?)
        }
        AuthCommand::Logout => {
            config.auth = AuthConfig::default();
            config.save(paths)?;
            logout(paths)?;
            json!({ "ok": true })
        }
    };
    Ok(Output::new(data, "store"))
}

fn read_secret(from_stdin: bool) -> Result<String, AppError> {
    let read_error = |_| AppError::usage("could not read the client secret");
    let stdin = io::stdin();
    if from_stdin {
        let mut line = String::new();
        stdin.lock().read_line(&mut line).map_err(read_error)?;
        Ok(line.trim_end_matches(['\r', '\n']).into())
    } else if stdin.is_terminal() {
        rpassword::prompt_password("Client secret: ").map_err(read_error)
    } else {
        Err(AppError::usage(
            "use --client-secret-stdin when stdin is not a terminal",
        ))
    }
}
