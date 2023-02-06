use std::net::SocketAddrV4;

use tracing_subscriber::{
    filter::{LevelFilter, Targets},
    prelude::*,
};

use grafana_tokio_console_datasource::ConsolePlugin;

#[grafana_plugin_sdk::main(
    services(data, diagnostics, resource, stream),
    init_subscriber = false,
    shutdown_handler = "0.0.0.0:10001"
)]
async fn plugin() -> ConsolePlugin {
    let fmt_filter = std::env::var("RUST_LOG")
        .ok()
        .and_then(|rust_log| match rust_log.parse::<Targets>() {
            Ok(targets) => Some(targets),
            Err(e) => {
                eprintln!("failed to parse `RUST_LOG={rust_log:?}`: {e}");
                None
            }
        })
        .unwrap_or_else(|| Targets::default().with_default(LevelFilter::WARN));
    let console_layer = console_subscriber::ConsoleLayer::builder()
        .server_addr("127.0.0.1:6668".parse::<SocketAddrV4>().unwrap())
        .spawn();

    tracing_subscriber::registry()
        .with(console_layer)
        .with(grafana_plugin_sdk::backend::layer().with_filter(fmt_filter))
        .init();
    ConsolePlugin::default()
}
