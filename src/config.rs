use crate::{
    cli::Source,
    error::{AppError, ErrorCode},
    ids::duration,
};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self, AppError> {
        let base = BaseDirs::new();
        let resolve = |key, default: Option<&Path>| {
            env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .or_else(|| default.map(|dir| dir.join("reddit-cli")))
                .ok_or_else(|| config_error("cannot locate Reddit CLI directories"))
        };
        Ok(Self {
            config_dir: resolve(
                "REDDIT_CLI_CONFIG_DIR",
                base.as_ref().map(BaseDirs::config_dir),
            )?,
            data_dir: resolve("REDDIT_CLI_DATA_DIR", base.as_ref().map(BaseDirs::data_dir))?,
        })
    }

    pub fn config(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn token(&self) -> PathBuf {
        self.data_dir.join("token.json")
    }
}

#[derive(Default, Deserialize, Serialize)]
pub struct Config {
    pub source: Option<String>,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub store: StoreConfig,
}

#[derive(Default, Deserialize, Serialize)]
pub struct AuthConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
pub struct StoreConfig {
    pub retention: Option<String>,
}

impl Config {
    pub fn load(paths: &Paths) -> Result<Self, AppError> {
        ensure_private_dir(&paths.config_dir)?;
        let Some(contents) = read_private(&paths.config())? else {
            return Ok(Self::default());
        };
        // TOML parser errors include source snippets, which could contain credentials.
        toml::from_str(&contents)
            .map_err(|_| config_error("config.toml contains invalid configuration"))
    }

    pub fn save(&self, paths: &Paths) -> Result<(), AppError> {
        let contents = toml::to_string_pretty(self)
            .map_err(|_| config_error("cannot serialize config.toml"))?;
        write_private(&paths.config(), contents.as_bytes())
    }

    pub fn retention(&self) -> Result<i64, AppError> {
        duration(self.store.retention.as_deref().unwrap_or("30d"))
            .map_err(|_| config_error("invalid [store] retention duration"))
    }

    pub fn source(&self, flag: Option<Source>) -> Result<Source, AppError> {
        if let Some(flag) = flag {
            return Ok(flag);
        }
        if let Some(raw) = env::var_os("REDDIT_CLI_SOURCE") {
            return parse_source(&raw.to_string_lossy())
                .ok_or_else(|| config_error("invalid REDDIT_CLI_SOURCE"));
        }
        self.source.as_deref().map_or(Ok(Source::Auto), |raw| {
            parse_source(raw).ok_or_else(|| config_error("invalid config.toml source"))
        })
    }
}

fn parse_source(raw: &str) -> Option<Source> {
    match raw {
        "live" => Some(Source::Live),
        "archive" => Some(Source::Archive),
        "auto" => Some(Source::Auto),
        _ => None,
    }
}

#[derive(Clone)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
    pub user_agent: String,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Env,
    Config,
    Unset,
}

#[derive(Serialize)]
pub struct CredentialOrigins {
    pub client_id: Origin,
    pub client_secret: Origin,
    pub user_agent: Origin,
}

fn present(value: Option<String>) -> Option<String> {
    value.filter(|value| {
        let trimmed = value.trim();
        !trimmed.is_empty() && !trimmed.starts_with('$')
    })
}

fn resolve_field(key: &str, config: Option<String>) -> (Option<String>, Origin) {
    if let Some(value) = present(env::var(key).ok()) {
        (Some(value), Origin::Env)
    } else if let Some(value) = present(config) {
        (Some(value), Origin::Config)
    } else {
        (None, Origin::Unset)
    }
}

pub fn credentials(config: &AuthConfig) -> (Option<Credentials>, CredentialOrigins) {
    let (client_id, id_origin) = resolve_field("REDDIT_CLIENT_ID", config.client_id.clone());
    let (client_secret, secret_origin) =
        resolve_field("REDDIT_CLIENT_SECRET", config.client_secret.clone());
    let (user_agent, agent_origin) = resolve_field("REDDIT_USER_AGENT", config.user_agent.clone());
    (
        client_id
            .zip(client_secret)
            .map(|(client_id, client_secret)| Credentials {
                client_id,
                client_secret,
                user_agent: user_agent.unwrap_or_else(default_user_agent),
            }),
        CredentialOrigins {
            client_id: id_origin,
            client_secret: secret_origin,
            user_agent: agent_origin,
        },
    )
}

pub fn default_user_agent() -> String {
    format!(
        "{}:reddit-cli:{} (+https://github.com/Microck/reddit-cli)",
        env::consts::OS,
        env!("CARGO_PKG_VERSION")
    )
}

pub(crate) fn read_private(path: &Path) -> Result<Option<String>, AppError> {
    use std::io::Read;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(file_error(path, error)),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = file
            .metadata()
            .map_err(|error| file_error(path, error))?
            .permissions();
        if permissions.mode() & 0o077 != 0 {
            return Err(config_error(format!(
                "{} must have private permissions (0600)",
                path.display()
            )));
        }
    }
    let mut contents = String::new();
    (&file)
        .read_to_string(&mut contents)
        .map_err(|error| file_error(path, error))?;
    Ok(Some(contents))
}

pub(crate) fn write_private(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let parent = path.parent().expect("private file paths have parents");
    ensure_private_dir(parent)?;
    let mut file =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| file_error(path, error))?;
    file.write_all(contents)
        .map_err(|error| file_error(path, error))?;
    file.as_file()
        .sync_all()
        .map_err(|error| file_error(path, error))?;
    file.persist(path)
        .map_err(|error| file_error(path, error.error))?;
    Ok(())
}

pub(crate) fn ensure_private_dir(path: &Path) -> Result<(), AppError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| file_error(path, error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::metadata(path)
            .map_err(|error| file_error(path, error))?
            .permissions();
        if permissions.mode() & 0o7777 != 0o700 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|error| file_error(path, error))?;
        }
    }
    Ok(())
}

pub(crate) fn remove_if_present(path: &Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(file_error(path, error)),
    }
}

pub(crate) fn file_error(path: &Path, error: io::Error) -> AppError {
    config_error(format!("{}: {error}", path.display()))
}

fn config_error(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::EConfig, message)
}
