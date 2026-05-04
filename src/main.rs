mod hec_receiver;

use hec_receiver::{AppState, ConfigAction, RuntimeConfig};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loaded = RuntimeConfig::load()?;
    match loaded.action {
        ConfigAction::ShowConfig => {
            print!("{}", loaded.config.redacted_toml()?);
            return Ok(());
        }
        ConfigAction::CheckConfig => {
            eprintln!("hec configuration ok");
            return Ok(());
        }
        ConfigAction::Run => {}
    }

    let config = loaded.config;
    let addr = config.addr;
    let state = Arc::new(
        match config.capture_path {
            Some(path) => AppState::capture_file(vec![config.token], config.limits, path),
            None => AppState::drop_only(vec![config.token], config.limits),
        }
        .with_protocol(config.protocol),
    );
    let app = hec_receiver::router(state);
    let listener = TcpListener::bind(addr).await?;

    eprintln!("hec receiver listening on http://{addr}");
    eprintln!("hec config precedence: defaults < TOML < CLI < environment");
    eprintln!("hec config tools: --config, --show-config, --check-config");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
