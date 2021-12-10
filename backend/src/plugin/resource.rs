use bytes::Bytes;
use futures::stream;
use grafana_plugin_sdk::backend;
use http::{Response, StatusCode};
use serde::Serialize;
use thiserror::Error;

use crate::ConsolePlugin;

use super::DatasourceUid;

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("Path not found")]
    NotFound,

    #[error("Plugin error: {0}")]
    Plugin(#[from] super::Error),

    #[error("Missing datasource settings")]
    MissingDatasourceSettings,

    #[error("Console not connected; please load a dashboard connected to this console first.")]
    ConsoleNotConnected,
}

#[derive(Debug, Serialize)]
pub struct JsonError {
    error: String,
}

impl backend::ErrIntoHttpResponse for ResourceError {
    fn into_http_response(self) -> Result<Response<Bytes>, Box<dyn std::error::Error>> {
        let status = match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Plugin(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MissingDatasourceSettings => StatusCode::BAD_REQUEST,
            Self::ConsoleNotConnected => StatusCode::BAD_REQUEST,
        };
        Ok(Response::builder().status(status).body(Bytes::from(
            serde_json::to_vec(&JsonError {
                error: self.to_string(),
            })
            .expect("valid JSON"),
        ))?)
    }
}

#[backend::async_trait]
impl backend::ResourceService for ConsolePlugin {
    type Error = ResourceError;

    type InitialResponse = Response<Bytes>;

    type Stream = backend::BoxResourceStream<Self::Error>;

    async fn call_resource(
        &self,
        request: backend::CallResourceRequest,
    ) -> Result<(Self::InitialResponse, Self::Stream), Self::Error> {
        let datasource_settings = request
            .plugin_context
            .and_then(|pc| pc.datasource_instance_settings)
            .ok_or(ResourceError::MissingDatasourceSettings)?;
        let datasource_uid = DatasourceUid(datasource_settings.uid.clone());

        if self.state.get(&datasource_uid).is_none() {
            self.connect(datasource_settings).await?;
            if self.state.get(&datasource_uid).is_none() {
                return Err(ResourceError::ConsoleNotConnected);
            }
        }

        let initial_response = self
            .get_tasks(&datasource_uid)
            .await
            .map(|mut tasks| {
                tasks.sort_unstable();
                Response::new(Bytes::from(serde_json::to_vec(&tasks).expect("valid JSON")))
            })
            .map_err(ResourceError::Plugin)?;
        Ok((initial_response, Box::pin(stream::empty())))
    }
}
