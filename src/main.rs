use anyhow::Context;
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;
use wcashexplorer::{
    api::{AppState, router},
    auxpow::AuxPowVerifier,
    config::Config,
    db::Database,
    indexer::Indexer,
    rpc::ZebraRpc,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    initialize_tracing();
    let config = Config::from_env().context("could not load explorer configuration")?;
    let database = Database::connect(&config.database_url)
        .await
        .context("could not connect to PostgreSQL")?;
    if config.migrate {
        database
            .migrate()
            .await
            .context("could not migrate the explorer database")?;
    }
    database
        .initialize_network(&config.network)
        .await
        .context("could not initialize the configured Wcash network")?;

    let wcash = ZebraRpc::new(config.wcash_rpc.clone(), config.rpc_timeout)
        .context("could not initialize Wcash RPC")?;
    let parents = config
        .zcash_rpcs
        .iter()
        .cloned()
        .map(|rpc| ZebraRpc::new(rpc, config.rpc_timeout))
        .collect::<wcashexplorer::error::Result<Vec<_>>>()
        .context("could not initialize Zcash parent RPC")?;
    if config.run_indexer {
        for parent in &parents {
            parent
                .validate_chain_identity(&config.parent_chain, &config.parent_genesis_hash)
                .await
                .with_context(|| {
                    format!(
                        "configured parent RPC {} is not the expected Zcash Testnet chain",
                        parent.label()
                    )
                })?;
        }
    }
    let auxpow = AuxPowVerifier::new(wcash.clone(), parents);

    let indexer_handle = config.run_indexer.then(|| {
        let indexer = Indexer::new(
            database.clone(),
            wcash,
            auxpow,
            config.network.clone(),
            config.poll_interval,
            config.max_reorg_depth,
            config.evidence_refresh_depth,
        );
        tokio::spawn(indexer.run())
    });

    let listener = TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("could not bind explorer API to {}", config.bind))?;
    info!(bind = %config.bind, network = %config.network.id, "WcashExplorer API is listening");
    let app = router(AppState::new(database, config.network));
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
    if let Some(mut handle) = indexer_handle {
        tokio::select! {
            result = server => result.context("HTTP server failed")?,
            result = &mut handle => {
                let indexer_result = result.context("explorer indexer task panicked")?;
                if let Err(error) = indexer_result {
                    error!(error = %error, "explorer indexer stopped");
                    return Err(error).context("explorer indexer stopped");
                }
                return Err(anyhow::anyhow!("explorer indexer stopped unexpectedly"));
            }
        }
        handle.abort();
    } else {
        server.await.context("HTTP server failed")?;
    }
    Ok(())
}

fn initialize_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("wcashexplorer=info,tower_http=info"));
    if std::env::var("LOG_FORMAT").as_deref() == Ok("json") {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_current_span(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .compact()
            .init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
