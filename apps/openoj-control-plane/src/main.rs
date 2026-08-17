use std::env;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openoj_application::{LeasePolicy, NodePolicy};
use openoj_control_plane::{JudgeControlService, bind_socket, serve_with_shutdown};
use openoj_domain::{Capability, LeaseDuration, NodeId, UnixMillis};
use openoj_storage::{DatabasePoolSize, PostgresEvaluationStore};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("openoj-control-plane failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), &'static str> {
    let database_url =
        env::var("OPENOJ_DATABASE_URL").map_err(|_| "database configuration missing")?;
    let socket_path = PathBuf::from(
        env::var("OPENOJ_JUDGE_CONTROL_SOCKET").map_err(|_| "socket configuration missing")?,
    );
    let node_policy = parse_node_policy(
        &env::var("OPENOJ_JUDGE_NODES").map_err(|_| "node policy configuration missing")?,
    )?;
    let lease_duration = LeaseDuration::new(env_u64("OPENOJ_LEASE_DURATION_MS", 30_000)?)
        .map_err(|_| "lease configuration invalid")?;
    let renew_after = LeaseDuration::new(env_u64("OPENOJ_RENEW_AFTER_MS", 10_000)?)
        .map_err(|_| "lease configuration invalid")?;
    let recovery_interval_ms = env_u64("OPENOJ_RECOVERY_INTERVAL_MS", 5_000)?;
    let lease_policy =
        LeasePolicy::new(lease_duration, renew_after).map_err(|_| "lease configuration invalid")?;
    let pool_size = DatabasePoolSize::new(4).ok_or("database pool configuration invalid")?;
    let store = PostgresEvaluationStore::connect(&database_url, pool_size)
        .await
        .map_err(|_| "database unavailable")?;
    store
        .migrate()
        .await
        .map_err(|_| "database migration failed")?;
    store
        .check_compatibility()
        .await
        .map_err(|_| "database schema incompatible")?;

    let listener = bind_socket(socket_path)
        .await
        .map_err(|_| "socket setup failed")?;
    let service = JudgeControlService::new(node_policy, lease_policy).with_store(store.clone());

    let recovery_store = store.clone();
    let sweeper = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(recovery_interval_ms));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Ok(now) = now_millis() else { continue };
            match recovery_store.recover_expired(now).await {
                Ok(count) if count > 0 => {
                    eprintln!("recovered {count} expired evaluation lease(s)");
                }
                Ok(_) => {}
                Err(_) => eprintln!("expired-lease recovery failed"),
            }
        }
    });

    let result = serve_with_shutdown(listener, service, async {
        let _ignored = tokio::signal::ctrl_c().await;
    })
    .await
    .map_err(|_| "UDS service failed");
    sweeper.abort();
    result
}

fn env_u64(name: &str, default: u64) -> Result<u64, &'static str> {
    match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| "configuration value invalid"),
        Err(_) => Ok(default),
    }
}

fn now_millis() -> Result<UnixMillis, &'static str> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock unavailable")?;
    let milliseconds = u64::try_from(elapsed.as_millis()).map_err(|_| "clock unavailable")?;
    UnixMillis::new(milliseconds).map_err(|_| "clock unavailable")
}

fn parse_node_policy(value: &str) -> Result<NodePolicy, &'static str> {
    let nodes = value
        .split(';')
        .map(|entry| -> Result<(NodeId, Vec<Capability>), &'static str> {
            let (node, capabilities) = entry.split_once(':').ok_or("node policy invalid")?;
            let node = NodeId::parse(node).map_err(|_| "node policy invalid")?;
            let capabilities = capabilities
                .split(',')
                .map(|capability| Capability::parse(capability).map_err(|_| "node policy invalid"))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((node, capabilities))
        })
        .collect::<Result<Vec<_>, _>>()?;
    NodePolicy::new(nodes).map_err(|_| "node policy invalid")
}
