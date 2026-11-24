use crate::{
    archive::ArchiveBackend,
    backend::{Backend, SourceKind},
    cli::{Cli, Command, Format},
    commands,
    config::{Config, Paths, credentials, default_user_agent},
    error::{AppError, ErrorCode},
    http::{CachePolicy, Endpoints, HttpClient},
    live::{LiveBackend, auth::TokenProvider},
    model::now,
    output,
    store::Store,
};
use clap::{CommandFactory, Parser, ValueEnum, error::ErrorKind};
use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal, Write},
    process::ExitCode,
    sync::Arc,
};

pub fn run() -> ExitCode {
    let args: Vec<_> = env::args_os().collect();
    let terminal = io::stdout().is_terminal();
    let format_env = env::var_os("REDDIT_CLI_FORMAT");
    let format_env = format_env
        .as_deref()
        .map(|value| {
            value
                .to_str()
                .and_then(|raw| Format::from_str(raw, false).ok())
                .ok_or_else(|| AppError::new(ErrorCode::EConfig, "invalid REDDIT_CLI_FORMAT"))
        })
        .transpose();

    let cli = match Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = io::stdout().write_all(error.to_string().as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            let format = resolve_format(
                raw_format(&args).or_else(|| format_env.ok().flatten()),
                terminal,
            );
            return report(
                &AppError::usage(error.to_string().trim().trim_start_matches("error: ")),
                format,
            );
        }
    };
    let format = match cli.format.map_or(format_env, |format| Ok(Some(format))) {
        Ok(format) => resolve_format(format, terminal),
        Err(error) => return report(&error, resolve_format(None, terminal)),
    };
    let color = terminal && env::var_os("NO_COLOR").is_none();
    let quiet = cli.quiet;
    match execute(cli, format, color) {
        Ok((text, notes)) => {
            if let Err(error) = io::stdout().lock().write_all(text.as_bytes())
                && error.kind() != io::ErrorKind::BrokenPipe
            {
                return report(
                    &AppError::new(ErrorCode::EConfig, "cannot write to stdout"),
                    format,
                );
            }
            // JSON/JSONL already carry notes in meta; echoing them would double them for
            // agents that merge stderr into stdout.
            if !quiet && matches!(format, Format::Pretty | Format::Md) {
                let mut stderr = io::stderr().lock();
                for note in notes {
                    let _ = writeln!(stderr, "note: {note}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => report(&error, format),
    }
}

pub fn resolve_format(format: Option<Format>, terminal: bool) -> Format {
    match format.unwrap_or(Format::Auto) {
        Format::Auto if terminal => Format::Pretty,
        Format::Auto => Format::Json,
        format => format,
    }
}

fn raw_format(args: &[OsString]) -> Option<Format> {
    let mut args = args.iter().skip(1);
    let mut format = None;
    while let Some(value) = args.next().and_then(|arg| arg.to_str()) {
        if value == "--" {
            break;
        }
        let raw = if value == "--format" {
            args.next().and_then(|arg| arg.to_str())
        } else {
            value.strip_prefix("--format=")
        };
        if let Some(raw) = raw {
            format = Format::from_str(raw, false).ok();
        }
    }
    format
}

fn execute(cli: Cli, format: Format, color: bool) -> Result<(String, Vec<String>), AppError> {
    let command = match cli.command {
        Command::Completions { shell } => {
            let mut bytes = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "reddit", &mut bytes);
            return Ok((
                String::from_utf8(bytes).expect("completion script is UTF-8"),
                Vec::new(),
            ));
        }
        Command::Skills(command) => {
            let output = commands::skills(command)?;
            return Ok((output::write(&output, format, color)?, output.meta.notes));
        }
        command => command,
    };
    let paths = Paths::discover()?;
    // Clear must remain callable even when the database or config cannot open.
    let config = if matches!(command, Command::Cache(crate::cli::CacheCommand::Clear)) {
        Config::default()
    } else {
        Config::load(&paths)?
    };
    let output = match command {
        Command::Auth(command) => commands::auth(command, &paths, config, cli.offline)?,
        Command::Cache(command) => commands::cache(command, &paths, &config)?,
        command => {
            let (credentials, _) = credentials(&config.auth);
            let source = SourceKind::select(config.source(cli.source)?, credentials.is_some());
            let store = Arc::new(Store::open(&paths, config.retention()?)?);
            let user_agent = credentials
                .as_ref()
                .map_or_else(default_user_agent, |credentials| {
                    credentials.user_agent.clone()
                });
            let http = Arc::new(HttpClient::new(
                Arc::clone(&store),
                CachePolicy {
                    offline: cli.offline,
                    refresh: cli.refresh,
                    no_cache: cli.no_cache,
                },
                Endpoints::from_env()?,
                &user_agent,
            )?);
            let backend: Box<dyn Backend> = match source {
                SourceKind::Live => {
                    let tokens = if cli.offline {
                        None
                    } else {
                        credentials
                            .map(|credentials| TokenProvider::new(paths, credentials))
                            .transpose()?
                            .map(Arc::new)
                    };
                    Box::new(LiveBackend::new(
                        Arc::clone(&http),
                        Arc::clone(&store),
                        tokens,
                    ))
                }
                SourceKind::Archive => {
                    Box::new(ArchiveBackend::new(Arc::clone(&http), Arc::clone(&store)))
                }
            };
            let mut output = commands::run_backend(backend.as_ref(), command)?;
            let observed = http.observation();
            let fetched_at = observed.fetched_at.unwrap_or_else(now);
            output.meta.cached = observed.cached;
            output.set_fetched_at(fetched_at);
            output.meta.rate_limit = http.rate_limit(source)?;
            if !cli.no_cache {
                output.persist(&store, source, fetched_at)?;
            }
            if cli.offline {
                output.meta.source = "store";
            }
            output
        }
    };
    Ok((output::write(&output, format, color)?, output.meta.notes))
}

fn report(error: &AppError, format: Format) -> ExitCode {
    let _ = io::stderr().write_all(output::render_error(error, format).as_bytes());
    ExitCode::from(error.code.exit())
}
