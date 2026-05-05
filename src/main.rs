mod hec_receiver;

use hec_receiver::{AppState, ConfigAction, ObserveConfig, ObserveFormat, RuntimeConfig};
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
    let state = Arc::new(
        match config.capture_path {
            Some(path) => AppState::capture_file_with_report_outputs(
                vec![config.token],
                config.limits,
                path,
                report_outputs,
            ),
            None => AppState::drop_only_with_report_outputs(
                vec![config.token],
                config.limits,
                report_outputs,
            ),
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

fn init_tracing(observe: &ObserveConfig) {
    if !observe.tracing {
        return;
    }

    let filter =
        EnvFilter::try_new(&observe.level).unwrap_or_else(|_| EnvFilter::new("hec_receiver=info"));
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
