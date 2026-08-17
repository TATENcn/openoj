use std::process::ExitCode;

use openoj_cli::{execute, parse_args, parse_database_pool_size, system_now};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<String, openoj_cli::CliError> {
    let command = parse_args(std::env::args_os().skip(1))?;
    let database_url = std::env::var_os("OPENOJ_DATABASE_URL")
        .ok_or(openoj_cli::CliError::MissingDatabaseUrl)?
        .into_string()
        .map_err(|_| openoj_cli::CliError::MissingDatabaseUrl)?;
    let pool_value = std::env::var("OPENOJ_DATABASE_MAX_CONNECTIONS").ok();
    let pool_size = parse_database_pool_size(pool_value.as_deref())?;
    execute(command, &database_url, pool_size, system_now()?).await
}
