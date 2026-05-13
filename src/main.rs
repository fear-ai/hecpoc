mod hec_receiver;

use hec_receiver::{
    AppState, ConfigAction, ObserveConfig, ObserveFormat, Phase, RuntimeConfig, TokenRegistry,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loaded = RuntimeConfig::load()?;
    init_tracing(&loaded.config.observe);
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
    let report_outputs = config.observe.report_outputs();
    let tokens = TokenRegistry::single(
        config.token_id,
        config.token,
        config.token_enabled,
        config.default_index,
        config.allowed_indexes,
        config.token_ack_enabled,
    );
    let state = Arc::new(
        match config.capture_path {
            Some(path) => {
                AppState::capture_file_with_registry(tokens, config.limits, path, report_outputs)
            }
            None => AppState::drop_events_with_registry(tokens, config.limits, report_outputs),
        }
        .with_protocol(config.protocol),
    );
    let app = hec_receiver::router(Arc::clone(&state));
    let listener = TcpListener::bind(addr).await?;

    eprintln!("hec receiver listening on http://{addr}");
    eprintln!("hec config precedence: defaults < TOML < CLI < environment");
    eprintln!("hec config tools: --config, --show-config, --check-config");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(Arc::clone(&state)))
        .await?;

    Ok(())
}

async fn shutdown_signal(state: Arc<AppState>) {
    let _ = tokio::signal::ctrl_c().await;
    state.health.set_phase(Phase::Stopping);
}

fn init_tracing(observe: &ObserveConfig) {
    if !observe.tracing {
        return;
    }

    let filter = EnvFilter::try_new(observe.filter_directives())
        .unwrap_or_else(|_| EnvFilter::new("info,hec.receiver=info"));
    let result = match observe.format {
        ObserveFormat::Compact => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init(),
        ObserveFormat::Json => tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init(),
    };
    let _ = result;
}
