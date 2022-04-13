use grafana_plugin_sdk::backend::{self, HealthStatus};
use serde_json::Value;

use crate::connection::Connection;

use super::{ConsolePlugin, Error};

#[backend::async_trait]
impl backend::DiagnosticsService for ConsolePlugin {
    type CheckHealthError = Error;

    async fn check_health(
        &self,
        request: backend::CheckHealthRequest,
    ) -> Result<backend::CheckHealthResponse, Self::CheckHealthError> {
        let url = request
            .plugin_context
            .ok_or(Error::MissingDatasource)
            .and_then(|pc| {
                pc.datasource_instance_settings
                    .ok_or(Error::MissingDatasource)
                    .and_then(|x| {
                        x.url
                            .parse()
                            .map_err(|_| Error::InvalidDatasourceUrl(x.url.clone()))
                    })
            })?;
        Ok(Connection::try_connect(url)
            .await
            .map(|_| {
                backend::CheckHealthResponse::new(
                    HealthStatus::Ok,
                    "Connection successful".to_string(),
                    Value::Null,
                )
            })
            .unwrap_or_else(|e| {
                backend::CheckHealthResponse::new(HealthStatus::Error, e.to_string(), Value::Null)
            }))
    }

    type CollectMetricsError = Error;

    async fn collect_metrics(
        &self,
        _request: backend::CollectMetricsRequest,
    ) -> Result<backend::CollectMetricsResponse, Self::CollectMetricsError> {
        todo!()
    }
}
