use bytes::Bytes;
use futures::stream;
use grafana_plugin_sdk::backend;
use http::{Response, StatusCode};
use serde::Serialize;
use thiserror::Error;
use url::{ParseError, Url};

use crate::ConsolePlugin;

use super::DatasourceUid;

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("Path not found")]
    NotFound,

    #[error("Invalid URI: {0}")]
    InvalidUri(#[from] ParseError),

    #[error("Plugin error: {0}")]
    Plugin(#[from] super::Error),

    #[error("Missing datasource UID")]
    MissingDatasourceUid,

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
            Self::InvalidUri(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Plugin(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::MissingDatasourceUid => StatusCode::BAD_REQUEST,
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
        let datasource_uid = extract_datasource_uid(request.request.uri())?;

        if self.state.get(&datasource_uid).is_none() {
            return Err(ResourceError::ConsoleNotConnected);
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

fn extract_datasource_uid(uri: &http::Uri) -> Result<DatasourceUid, ResourceError> {
    if !uri.path().ends_with("/variablevalues/tasks") {
        return Err(ResourceError::NotFound);
    }
    // Annoying to have to allocate here, but I don't want to reimplement
    // query param parsing...
    // We need to use this fake scheme/host because Grafana only gives us a relative
    // URL relevant to our plugin, and the `Url` type can only be parsed from absolute URLs.
    let dummy = Url::parse("http://localhost").unwrap();
    let url = dummy.join(&uri.to_string())?;
    let datasource_uid = url
        .query_pairs()
        .find(|x| x.0 == "datasourceUid")
        .ok_or(ResourceError::MissingDatasourceUid)?;
    Ok(DatasourceUid(datasource_uid.1.to_string()))
}
