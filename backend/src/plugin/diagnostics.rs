use grafana_plugin_sdk::backend;

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
            .map(|_| backend::CheckHealthResponse::ok("Connection successful".to_string()))
            .unwrap_or_else(|e| backend::CheckHealthResponse::error(e.to_string())))
    }

    type CollectMetricsError = Error;

    async fn collect_metrics(
        &self,
        _request: backend::CollectMetricsRequest,
    ) -> Result<backend::CollectMetricsResponse, Self::CollectMetricsError> {
        todo!()
    }
}
