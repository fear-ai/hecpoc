mod hec_receiver;

use hec_receiver::{AppState, RuntimeConfig};
use std::sync::Arc;
use tokio::net::TcpListener;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = RuntimeConfig::load()?;
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
    eprintln!("hec config file: set HEC_CONFIG=/path/hec.toml; environment overrides file values");
    eprintln!("hec token source: HEC_TOKEN or SPANK_HEC_TOKEN; default is dev-token");
    eprintln!("hec capture: set HEC_CAPTURE=/path/events.jsonl to write accepted events");
    eprintln!("hec limits: HEC_MAX_BYTES, HEC_MAX_DECODED_BYTES, HEC_MAX_EVENTS override defaults");
    eprintln!(
        "hec timing/buffer: HEC_IDLE_TIMEOUT, HEC_TOTAL_TIMEOUT, HEC_GZIP_BUFFER_BYTES override defaults"
    );
    eprintln!("hec protocol: HEC_SUCCESS, HEC_TOKEN_REQUIRED, and related overrides are available");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
