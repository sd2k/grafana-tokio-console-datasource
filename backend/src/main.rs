use tracing_subscriber::{filter::filter_fn, prelude::*};

use grafana_tokio_console_datasource::ConsolePlugin;

#[grafana_plugin_sdk::main(
    services(data, stream),
    init_subscriber = false,
    shutdown_handler = "0.0.0.0:10001"
)]
async fn plugin() -> ConsolePlugin {
    console_subscriber::build()
        .with(
            grafana_plugin_sdk::backend::layer().with_filter(filter_fn(|metadata| {
                !metadata.target().starts_with("console_subscriber")
                    && !metadata.target().starts_with("tokio")
                    && !metadata.target().starts_with("runtime")
                    && !metadata.target().starts_with("h2")
                    && !metadata.target().starts_with("hyper")
            })),
        )
        .init();
    ConsolePlugin::default()
}
