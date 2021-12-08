use serde::Deserialize;

use grafana_plugin_sdk::{backend, data};

use super::{ConsolePlugin, Path};

#[derive(Debug, thiserror::Error)]
#[error("Error querying backend for {}", .ref_id)]
pub struct QueryError {
    ref_id: String,
}

impl backend::DataQueryError for QueryError {
    fn ref_id(self) -> String {
        self.ref_id
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct ConsoleQueryDataRequest {
    #[serde(flatten)]
    path: Path,
}

#[backend::async_trait]
impl backend::DataService for ConsolePlugin {
    type QueryError = QueryError;
    type Iter = backend::BoxDataResponseIter<Self::QueryError>;
    async fn query_data(&self, mut request: backend::QueryDataRequest) -> Self::Iter {
        Box::new(request.queries.into_iter().map(move |x| {
            let uid = request
                .plugin_context
                .datasource_instance_settings
                .take()
                .map(|x| x.uid)
                .ok_or_else(|| QueryError {
                    ref_id: x.ref_id.clone(),
                })?;

            let mut frame = data::Frame::new("");

            if let Ok(path) =
                serde_json::from_value(x.json).map(|req: ConsoleQueryDataRequest| req.path)
            {
                frame.set_channel(format!("ds/{}/{}", uid, path).parse().unwrap());
            }

            Ok(backend::DataResponse::new(
                x.ref_id,
                vec![frame.check().unwrap()],
            ))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::*;

    #[test]
    fn deserialize_path() {
        assert_eq!(
            serde_json::from_str::<Path>(r#"{"path": "tasks"}"#).unwrap(),
            Path::Tasks
        );
        assert_eq!(
            serde_json::from_str::<Path>(r#"{"path": "task", "taskId": 1}"#).unwrap(),
            Path::TaskDetails { task_id: TaskId(1) }
        );
        assert_eq!(
            serde_json::from_str::<Path>(r#"{"path": "taskHistogram", "taskId": 1}"#).unwrap(),
            Path::TaskHistogram { task_id: TaskId(1) }
        );
        assert_eq!(
            serde_json::from_str::<Path>(r#"{"path": "resources"}"#).unwrap(),
            Path::Resources
        );
    }

    #[test]
    fn deserialize_request() {
        assert_eq!(
            serde_json::from_str::<ConsoleQueryDataRequest>(r#"{"path": "tasks"}"#).unwrap(),
            ConsoleQueryDataRequest { path: Path::Tasks }
        );
        assert_eq!(
            serde_json::from_str::<ConsoleQueryDataRequest>(
                r#"{"path": "taskHistogram", "taskId": 1}"#
            )
            .unwrap(),
            ConsoleQueryDataRequest {
                path: Path::TaskHistogram { task_id: TaskId(1) }
            }
        );
        assert_eq!(
            serde_json::from_str::<ConsoleQueryDataRequest>(r#"{"path": "resources"}"#).unwrap(),
            ConsoleQueryDataRequest {
                path: Path::Resources
            }
        );
    }
}
