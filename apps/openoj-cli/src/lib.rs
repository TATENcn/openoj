use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use openoj_application::{ControlPlane, CreateEvaluation, EvaluationSnapshot};
use openoj_domain::{EvaluationId, UnixMillis};
use openoj_protocol::{MAX_EVALUATION_REQUEST_BYTES, decode_evaluation_request};
use openoj_storage::{DatabasePoolSize, PostgresEvaluationStore, SUPPORTED_SCHEMA_VERSION};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CliCommand {
    Migrate,
    Submit { path: PathBuf },
    Status { evaluation_id: EvaluationId },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CliError {
    Usage,
    InvalidEvaluationId,
    InputTooLarge,
    Io,
    MissingDatabaseUrl,
    InvalidDatabasePoolSize,
    InvalidRequest,
    Storage,
    Clock,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Usage => {
                "usage: openoj-cli migrate | submit <request.json> | status <evaluation-id>"
            }
            Self::InvalidEvaluationId => "evaluation ID is invalid",
            Self::InputTooLarge => "input exceeds the evaluation request byte limit",
            Self::Io => "input could not be read",
            Self::MissingDatabaseUrl => "OPENOJ_DATABASE_URL is required",
            Self::InvalidDatabasePoolSize => "OPENOJ_DATABASE_MAX_CONNECTIONS must be in 1..=64",
            Self::InvalidRequest => "input is not a valid canonical evaluation request",
            Self::Storage => "storage operation failed",
            Self::Clock => "system clock is outside the supported range",
        })
    }
}

impl Error for CliError {}

/// Parses the optional database pool bound, defaulting to four connections.
///
/// # Errors
///
/// Returns [`CliError::InvalidDatabasePoolSize`] unless the value is an integer in `1..=64`.
pub fn parse_database_pool_size(value: Option<&str>) -> Result<DatabasePoolSize, CliError> {
    let value = value
        .unwrap_or("4")
        .parse::<u32>()
        .map_err(|_| CliError::InvalidDatabasePoolSize)?;
    DatabasePoolSize::new(value).ok_or(CliError::InvalidDatabasePoolSize)
}

/// Parses exactly one bounded `OpenOJ` CLI command without collecting an unbounded argv list.
///
/// # Errors
///
/// Returns [`CliError::Usage`] for unknown or incorrectly sized commands and
/// [`CliError::InvalidEvaluationId`] for a malformed status target.
pub fn parse_args<I, S>(arguments: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut arguments = arguments.into_iter().map(Into::into);
    let command = arguments
        .next()
        .ok_or(CliError::Usage)?
        .into_string()
        .map_err(|_| CliError::Usage)?;
    match command.as_str() {
        "migrate" if arguments.next().is_none() => Ok(CliCommand::Migrate),
        "submit" => {
            let path = arguments.next().ok_or(CliError::Usage)?;
            if arguments.next().is_some() {
                return Err(CliError::Usage);
            }
            Ok(CliCommand::Submit {
                path: PathBuf::from(path),
            })
        }
        "status" => {
            let value = arguments
                .next()
                .ok_or(CliError::Usage)?
                .into_string()
                .map_err(|_| CliError::InvalidEvaluationId)?;
            if arguments.next().is_some() {
                return Err(CliError::Usage);
            }
            Ok(CliCommand::Status {
                evaluation_id: EvaluationId::parse(value)
                    .map_err(|_| CliError::InvalidEvaluationId)?,
            })
        }
        _ => Err(CliError::Usage),
    }
}

/// Reads at most `maximum` bytes and detects growth after the metadata check.
///
/// # Errors
///
/// Returns a declassified [`CliError`] for oversized or unreadable input.
pub fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, CliError> {
    let file = File::open(path).map_err(|_| CliError::Io)?;
    let metadata = file.metadata().map_err(|_| CliError::Io)?;
    if metadata.len() > u64::try_from(maximum).map_err(|_| CliError::InputTooLarge)? {
        return Err(CliError::InputTooLarge);
    }
    let read_limit = maximum.checked_add(1).ok_or(CliError::InputTooLarge)?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len())
            .map_err(|_| CliError::InputTooLarge)?
            .min(maximum),
    );
    file.take(u64::try_from(read_limit).map_err(|_| CliError::InputTooLarge)?)
        .read_to_end(&mut bytes)
        .map_err(|_| CliError::Io)?;
    if bytes.len() > maximum {
        return Err(CliError::InputTooLarge);
    }
    Ok(bytes)
}

/// Executes one command against a concrete durable store using an injected bounded clock value.
///
/// # Errors
///
/// Returns a declassified [`CliError`] for incompatible storage, invalid input, or failed writes.
pub async fn execute_with_store(
    command: CliCommand,
    store: PostgresEvaluationStore,
    now: UnixMillis,
) -> Result<String, CliError> {
    match command {
        CliCommand::Migrate => {
            store.migrate().await.map_err(|_| CliError::Storage)?;
            Ok(format!("migrated schema {SUPPORTED_SCHEMA_VERSION}"))
        }
        CliCommand::Submit { path } => {
            store
                .check_compatibility()
                .await
                .map_err(|_| CliError::Storage)?;
            let bytes = read_bounded(&path, MAX_EVALUATION_REQUEST_BYTES)?;
            let request =
                decode_evaluation_request(&bytes).map_err(|_| CliError::InvalidRequest)?;
            let snapshot = ControlPlane::new(store)
                .create_evaluation(CreateEvaluation {
                    request,
                    created_at: now,
                })
                .await
                .map_err(|_| CliError::Storage)?;
            Ok(format_snapshot(&snapshot))
        }
        CliCommand::Status { evaluation_id } => {
            store
                .check_compatibility()
                .await
                .map_err(|_| CliError::Storage)?;
            let snapshot = ControlPlane::new(store)
                .evaluation_status(evaluation_id)
                .await
                .map_err(|_| CliError::Storage)?;
            Ok(format_snapshot(&snapshot))
        }
    }
}

/// Connects a bounded pool and executes one CLI command.
///
/// # Errors
///
/// Returns a declassified [`CliError`] when connection or command execution fails.
pub async fn execute(
    command: CliCommand,
    database_url: &str,
    pool_size: DatabasePoolSize,
    now: UnixMillis,
) -> Result<String, CliError> {
    let store = PostgresEvaluationStore::connect(database_url, pool_size)
        .await
        .map_err(|_| CliError::Storage)?;
    execute_with_store(command, store, now).await
}

/// Reads the process clock as a bounded Unix millisecond value.
///
/// # Errors
///
/// Returns [`CliError::Clock`] before the Unix epoch, after year 9999, or on overflow.
pub fn system_now() -> Result<UnixMillis, CliError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CliError::Clock)?;
    let millis = u64::try_from(duration.as_millis()).map_err(|_| CliError::Clock)?;
    UnixMillis::new(millis).map_err(|_| CliError::Clock)
}

fn format_snapshot(snapshot: &EvaluationSnapshot) -> String {
    format!(
        "evaluation={} attempt={} attempt_number={} state={} terminal_result={}",
        snapshot.evaluation_id,
        snapshot.current_attempt_id,
        snapshot.attempt_number,
        snapshot.state.as_str(),
        snapshot.terminal_result
    )
}
