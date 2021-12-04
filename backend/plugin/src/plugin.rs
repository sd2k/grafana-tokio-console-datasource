use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::Cursor,
    pin::Pin,
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime},
};

use chrono::prelude::*;
use console_api::{
    instrument::Update,
    resources::Resource,
    tasks::{Stats, TaskDetails},
};
use dashmap::DashMap;
use futures::{Stream, StreamExt};
use grafana_plugin_sdk::{backend, data, prelude::*};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, warn};

use crate::{
    connection::Connection,
    metadata::{MetaId, Metadata},
    task::{Task, TaskId},
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DatasourceUid(String);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing task ID")]
    MissingTaskId,
    #[error("invalid task ID: {0}")]
    InvalidTaskId(String),

    #[error("unknown path: {0}. must be one of: tasks, resources/<id>")]
    UnknownPath(String),

    #[error("data not found for instance")]
    DatasourceInstanceNotFound,

    #[error("invalid datasource URL: {0}")]
    InvalidDatasourceUrl(String),

    #[error("Datasource ID not present on request")]
    MissingDatasource,

    #[error("Error converting data: {0}")]
    ConvertTo(#[from] backend::ConvertToError),
    #[error("Error converting request: {0}")]
    ConvertFrom(#[from] backend::ConvertFromError),
    #[error("Error creating frame : {0}")]
    Data(#[from] data::Error),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "path")]
enum Path {
    #[serde(rename = "tasks")]
    Tasks,
    #[serde(rename = "task", rename_all = "camelCase")]
    TaskDetails { task_id: TaskId },
    #[serde(rename = "resources")]
    Resources,
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tasks => write!(f, "tasks"),
            Self::TaskDetails { task_id } => write!(f, "task/{}", task_id),
            Self::Resources => write!(f, "resources"),
        }
    }
}

impl FromStr for Path {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut iter = s.splitn(2, '/');
        match (iter.next(), iter.next()) {
            (Some("tasks"), _) => Ok(Self::Tasks),
            (Some("task"), None) => Err(Error::MissingTaskId),
            (Some("task"), Some(task_id)) => task_id
                .parse()
                .map(|id| Self::TaskDetails {
                    task_id: TaskId(id),
                })
                .map_err(|_| Error::InvalidTaskId(task_id.to_string())),
            (Some("resources"), _) => todo!(),

            _ => Err(Error::UnknownPath(s.to_string())),
        }
    }
}

#[derive(Debug)]
enum ConnectMessage {
    Connected,
    Disconnected,
}

struct Notification {
    message: ConnectMessage,
}

/// An instance of a Console datasource.
///
/// This is moved into a spawned task, and communicates back
/// to the plugin using channels.
#[derive(Debug)]
struct ConsoleInstance {
    connection: Connection,

    streams: HashSet<TaskId>,

    notifications: mpsc::Receiver<Notification>,
    stream_count: usize,
}

#[derive(Debug)]
struct DatasourceState {
    uid: DatasourceUid,

    metas: HashMap<MetaId, Metadata>,
    tasks: HashMap<TaskId, Task>,
    resources: HashMap<u64, Resource>,
    resource_stats: HashMap<u64, Stats>,

    last_updated: Option<SystemTime>,

    /// The incoming stream of task updates, to be forwarded to subscribers as `Frame`s.
    ///
    /// This will be `None` if the stream has been taken by `stream_tasks`.
    tasks_frame_tx: Option<mpsc::Sender<Result<data::Frame, Error>>>,
    tasks_frame_rx: Option<mpsc::Receiver<Result<data::Frame, Error>>>,
    /// The incoming stream of resource updates, to be forwarded to subscribers as `Frame`s.
    resources_stream_tx: Option<mpsc::Sender<Result<data::Frame, Error>>>,
    resources_stream_rx: Option<mpsc::Sender<Result<data::Frame, Error>>>,

    notification_tx: mpsc::Sender<Notification>,
}

impl DatasourceState {
    /// Create a new `DatasourceState` for a datasource from an initial update.
    async fn new(
        datasource_uid: DatasourceUid,
        update: Update,
    ) -> (Self, mpsc::Receiver<Notification>) {
        let (tasks_frame_tx, tasks_frame_rx) = mpsc::channel(128);
        let (notification_tx, notification_rx) = mpsc::channel(128);
        let mut s = DatasourceState {
            uid: datasource_uid,
            last_updated: None,
            metas: Default::default(),
            tasks: Default::default(),
            resources: Default::default(),
            resource_stats: Default::default(),

            tasks_frame_tx: Some(tasks_frame_tx),
            tasks_frame_rx: Some(tasks_frame_rx),
            resources_stream_tx: Default::default(),
            resources_stream_rx: Default::default(),
            notification_tx,
        };
        s.update(update).await;
        (s, notification_rx)
    }

    async fn update(&mut self, update: Update) {
        self.last_updated = Some(SystemTime::now());
        if let Some(new_metadata) = update.new_metadata {
            let metas = new_metadata.metadata.into_iter().filter_map(|meta| {
                let id = meta.id?.id;
                let metadata = meta.metadata?;
                let metadata = Metadata::from_proto(metadata, id);
                Some((metadata.id, metadata))
            });
            self.metas.extend(metas);
        }

        if let Some(task_update) = update.task_update {
            let mut stats_update = task_update.stats_update;
            let mut updated_ids = Vec::with_capacity(task_update.new_tasks.len());
            for new_task in task_update.new_tasks {
                if let Some(task) = Task::from_proto(&self.metas, &mut stats_update, new_task) {
                    updated_ids.push(task.id);
                    self.tasks.insert(task.id, task);
                }
            }
            for (id, stats) in stats_update {
                if let Some(task) = self.tasks.get_mut(&TaskId(id)) {
                    updated_ids.push(task.id);
                    task.stats = stats.into();
                }
            }

            // Send changes to any channels.
            let tasks_frame = self.get_tasks_frame(Some(&updated_ids));
            if let Some(tx) = &self.tasks_frame_tx {
                if let Err(e) = tx.send(tasks_frame).await {
                    error!(datasource_uid = %self.uid.0, error = %e, "error sending tasks frame")
                }
            }
        }
    }

    async fn update_details(&mut self, update: TaskDetails) {
        if let TaskDetails {
            task_id: Some(task_id),
            poll_times_histogram: Some(data),
            ..
        } = update
        {
            if let Some(task) = self.tasks.get_mut(&TaskId(task_id.id)) {
                task.histogram = hdrhistogram::serialization::Deserializer::new()
                    .deserialize(&mut Cursor::new(&data))
                    .ok();
            }
        }
    }

    fn get_tasks_frame(&self, updated_ids: Option<&[TaskId]>) -> Result<data::Frame, Error> {
        let len = updated_ids.map_or_else(|| self.tasks.len(), |x| x.len());
        let iter: Box<dyn Iterator<Item = &Task>> = match updated_ids {
            Some(ids) => Box::new(ids.iter().filter_map(|id| self.tasks.get(id))),
            None => Box::new(self.tasks.values()),
        };

        let now = Utc::now();
        let mut timestamps = Vec::with_capacity(len);
        let mut ids = Vec::with_capacity(len);
        let mut names = Vec::with_capacity(len);
        let mut targets = Vec::with_capacity(len);
        let mut fields = Vec::with_capacity(len);
        let mut states = Vec::with_capacity(len);
        let mut locations = Vec::with_capacity(len);

        let mut polls = Vec::with_capacity(len);
        let mut poll_times_histograms = Vec::with_capacity(len);
        let mut created_at = Vec::with_capacity(len);
        let mut dropped_at = Vec::with_capacity(len);
        let mut busy = Vec::with_capacity(len);
        let mut last_poll_started = Vec::with_capacity(len);
        let mut last_poll_ended = Vec::with_capacity(len);
        let mut idle = Vec::with_capacity(len);
        let mut total = Vec::with_capacity(len);
        let mut wakes = Vec::with_capacity(len);
        let mut waker_clones = Vec::with_capacity(len);
        let mut waker_drops = Vec::with_capacity(len);
        let mut last_wake = Vec::with_capacity(len);
        let mut self_wakes = Vec::with_capacity(len);

        for task in iter {
            timestamps.push(now);
            ids.push(task.id.0);
            names.push(task.name.clone());
            targets.push(task.target.clone());
            fields.push(
                task.fields
                    .iter()
                    .map(|f| format!("{}={}", f.name.to_string(), f.value))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            states.push(task.state().icon());
            locations.push(task.location.clone());

            polls.push(task.total_polls());
            poll_times_histograms
                .push(serde_json::to_string(&task.make_chart_data(100).0).unwrap());
            created_at.push(to_datetime(task.stats.created_at));
            dropped_at.push(task.stats.dropped_at.map(to_datetime));
            busy.push(self.last_updated.map(|x| task.busy(x)).map(as_nanos));
            last_poll_started.push(task.stats.last_poll_started.map(to_datetime));
            last_poll_ended.push(task.stats.last_poll_ended.map(to_datetime));
            idle.push(self.last_updated.map(|x| task.idle(x)).map(as_nanos));
            total.push(self.last_updated.map(|x| task.total(x)).map(as_nanos));
            wakes.push(task.wakes());
            waker_clones.push(task.waker_clones());
            waker_drops.push(task.waker_drops());
            last_wake.push(task.last_wake().map(to_datetime));
            self_wakes.push(task.self_wakes());
        }
        let frame = data::Frame::new("tasks").with_fields([
            timestamps.into_field("Time"),
            ids.into_field("ID"),
            names.into_field("Name"),
            targets.into_field("Target"),
            fields.into_field("Fields"),
            states.into_field("State"),
            locations.into_field("Location"),
            polls.into_field("Polls"),
            poll_times_histograms.into_field("Poll times"),
            created_at.into_field("Created At"),
            dropped_at.into_opt_field("Dropped At"),
            busy.into_opt_field("Busy"),
            last_poll_started.into_opt_field("Last Poll Started"),
            last_poll_ended.into_opt_field("Last Poll Ended"),
            idle.into_opt_field("Idle"),
            total.into_opt_field("Total"),
            wakes.into_field("Wakes"),
            waker_clones.into_field("Waker clones"),
            waker_drops.into_field("Waker drops"),
            last_wake.into_opt_field("Last wake"),
            self_wakes.into_field("Self wakes"),
        ]);
        Ok(frame)
    }

    /// Convert this state into an owned `Frame`.
    fn to_frame(&self, path: &Path) -> Result<data::Frame, Error> {
        match path {
            Path::Tasks => self.get_tasks_frame(None),
            _ => todo!(),
        }
    }
}

fn to_datetime(s: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(s)
}

fn as_nanos(d: Duration) -> Option<u64> {
    d.as_nanos()
        .try_into()
        .map_err(|e| error!(error = ?e, "error getting duration as nanos"))
        .ok()
}

#[derive(Clone, Debug, Default)]
pub struct ConsolePlugin {
    state: Arc<DashMap<DatasourceUid, DatasourceState>>,
}

impl ConsolePlugin {
    async fn connect(&self, datasource: backend::DataSourceInstanceSettings) -> Result<(), Error> {
        let datasource_uid = DatasourceUid(datasource.uid);
        let url = datasource
            .url
            .parse()
            .map_err(|_| Error::InvalidDatasourceUrl(datasource.url))?;
        info!(url = %url, "Connecting to console");
        let mut connection = Connection::new(url);
        // Get some initial state.
        let update = connection.next_update().await;
        let (instance_state, notification_rx) =
            DatasourceState::new(datasource_uid.clone(), update).await;
        self.state.insert(datasource_uid.clone(), instance_state);

        // Spawn a task to continuously fetch updates from the console, and
        // update the datasource. Each update will also send messages to the
        // listeners associated with the state, if there are any.
        let state = Arc::clone(&self.state);
        let uid_clone = datasource_uid.clone();

        spawn_named("manage connection", async move {
            let mut instance = ConsoleInstance {
                connection,
                streams: Default::default(),
                notifications: notification_rx,
                stream_count: 0,
            };
            let mut details_stream = futures::stream::SelectAll::new();
            loop {
                tokio::select! {
                    instrument_update = instance.connection.next_update() => {
                        if let Some(mut s) = state.get_mut(&uid_clone) {
                            s.update(instrument_update).await;
                            for task_id in s.tasks.keys() {
                                if !instance.streams.contains(task_id) {
                                    if let Ok(stream) = instance
                                        .connection
                                        .watch_details(task_id.0)
                                        .await {
                                        details_stream.push(stream);
                                    }
                                    instance.streams.insert(*task_id);
                                }
                            }
                        }
                    }

                    Some(Ok(details_update)) = details_stream.next() => {
                        if let Some(mut s) = state.get_mut(&uid_clone) {
                            s.update_details(details_update).await;
                        }
                    }

                    notification = instance.notifications.recv() => {
                        if let Some(n) = notification {
                            use ConnectMessage::{Connected, Disconnected};
                            match n.message {
                                Connected => instance.stream_count -= 1,
                                Disconnected => instance.stream_count -= 1,
                            };
                        } else {
                            // TODO: figure out why the sender would have dropped and how to handle it properly
                            warn!("Notifications channel dropped, stream may not be cleaned up");
                        }
                        // Drop connection and delete the initial state when we have no streams left.
                        if instance.stream_count == 0 {
                            instance.notifications.close();
                            state.remove(&uid_clone);
                            info!(url = %instance.connection.target(), "Disconnecting from console");
                            return
                        }
                    }
                }
            }
        });
        Ok(())
    }

    async fn stream_tasks(
        &self,
        datasource_uid: &DatasourceUid,
    ) -> <Self as backend::StreamService>::Stream {
        let state = self.state.get_mut(datasource_uid);
        if let Some(ref x) = state {
            if x.notification_tx
                .send(Notification {
                    message: ConnectMessage::Connected,
                })
                .await
                .is_err()
            {
                warn!(
                    datasource = %datasource_uid.0,
                    path = "tasks",
                    "Could not send connect notification",
                );
            };
        }
        Box::pin(
            state
                .and_then(|mut x| x.tasks_frame_rx.take())
                .ok_or(Error::DatasourceInstanceNotFound)
                .map(|x| {
                    ReceiverStream::new(x).map(|res| {
                        res.and_then(|frame| {
                            frame
                                .check()
                                .map_err(Error::Data)
                                .and_then(|f| Ok(backend::StreamPacket::from_frame(f)?))
                        })
                    })
                })
                .unwrap(),
        )
    }

    async fn stream_resources(
        &self,
        _datasource_uid: &DatasourceUid,
        _task_id: TaskId,
    ) -> <Self as backend::StreamService>::Stream {
        todo!()
    }

    /// Fetch the initial data for a given datasource instance and path.
    ///
    /// This will be used when a new subscriber is registered.
    fn initial_data(
        &self,
        datasource_uid: &DatasourceUid,
        path: &Path,
    ) -> Option<Result<data::Frame, Error>> {
        self.state.get(datasource_uid).map(|s| s.to_frame(path))
    }
}

fn frame_to_initial_data(frame: data::Frame) -> Result<backend::InitialData, Error> {
    let checked = frame.check()?;
    let init = backend::InitialData::from_frame(checked, data::FrameInclude::All)?;
    Ok(init)
}

pub struct ClientDisconnect<T>(T, oneshot::Sender<()>);

impl<T, I> Stream for ClientDisconnect<T>
where
    T: Stream<Item = I> + std::marker::Unpin,
{
    type Item = I;
    fn poll_next(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(ctx)
    }
}

#[backend::async_trait]
impl backend::StreamService for ConsolePlugin {
    type JsonValue = ();

    /// Subscribe to a stream of updates from a Console datasource instance.
    ///
    /// This function will be called every time a user subscribes to a stream.
    /// We have several Grafana streams for each datasource instance (tasks, resources, ...)
    /// but only need a single Console connection. When a subscription request comes in,
    /// we check whether a connection already exists for that datasource instance.
    /// If so, we can return the existing in-memory state as `initial_data`.
    /// If not, we need to create a new connection, load the initial state for the
    /// console in question, and store those so that future subscription requests
    /// reuse it.
    ///
    /// After creating a connection we must also spawn a task which will stream updates
    /// from the console to our in-memory state. This is due to the mismatch between
    /// having 1 console stream and 3 Grafana streams.
    ///
    /// TODO describe that better.
    async fn subscribe_stream(
        &self,
        request: backend::SubscribeStreamRequest,
    ) -> backend::SubscribeStreamResponse {
        let path = match request.path.parse() {
            Ok(p) => p,
            _ => {
                return backend::SubscribeStreamResponse {
                    status: backend::SubscribeStreamStatus::NotFound,
                    initial_data: None,
                }
            }
        };
        let datasource_settings = match request.plugin_context.datasource_instance_settings {
            Some(settings) => settings,
            None => {
                return backend::SubscribeStreamResponse {
                    status: backend::SubscribeStreamStatus::NotFound,
                    initial_data: None,
                }
            }
        };
        let uid = DatasourceUid(datasource_settings.uid.clone());

        // Check if we're already connected to this datasource instance and getting updates.
        // If so, we should just return the current state as `initial_data`.
        // If not, we should spawn a task to start populating the datasource instance's state.
        let initial_data: Result<data::Frame, Error> = match self.initial_data(&uid, &path) {
            Some(s) => s,
            None => self.connect(datasource_settings).await.and_then(|_| {
                self.state
                    .get(&uid)
                    // Invariant: self.connect will create the state for this uid.
                    .expect("state to be present")
                    .to_frame(&path)
            }),
        };

        match initial_data.and_then(frame_to_initial_data) {
            Ok(x) => backend::SubscribeStreamResponse {
                status: backend::SubscribeStreamStatus::Ok,
                initial_data: Some(x),
            },
            Err(_) => backend::SubscribeStreamResponse {
                // TODO - what should this really be?
                status: backend::SubscribeStreamStatus::NotFound,
                initial_data: None,
            },
        }
    }

    type StreamError = Error;
    type Stream = backend::BoxRunStream<Self::StreamError>;

    /// Begin streaming data for a given channel.
    ///
    /// This method is called _once_ for a (datasource, path) combination and the output
    /// is multiplexed to all clients by Grafana's backend. This is in contrast to the
    /// `subscribe_stream` method which is called for every client that wishes to connect.
    ///
    /// As such, this simply needs to stream updates of a specific type from a given datasource
    /// instance's in-memory state, and inform the datasource's console connection when no clients
    /// remain connected. This will allow the connection to disconnect, and the state to be cleared,
    /// when there are no longer any streams running for it.
    async fn run_stream(&self, request: backend::RunStreamRequest) -> Self::Stream {
        // This method will only be called once per path per datasource instance.
        // We need a way to signal to the connection object when the client (Grafana's backend)
        // has disconnected from this stream.

        let path: Path = match request.path.parse() {
            Ok(p) => p,
            _ => {
                return Box::pin(futures::stream::once(async {
                    Err(Error::UnknownPath(request.path))
                }))
            }
        };

        let datasource_settings = match request.plugin_context.datasource_instance_settings {
            Some(settings) => settings,
            None => {
                return Box::pin(futures::stream::once(async {
                    Err(Error::MissingDatasource)
                }))
            }
        };
        let uid = DatasourceUid(datasource_settings.uid.clone());

        let (tx, rx) = oneshot::channel::<()>();
        let p = path.clone();

        let sender = match self.state.get(&uid) {
            Some(s) => s.notification_tx.clone(),
            None => {
                if let Err(e) = self.connect(datasource_settings).await {
                    error!(error = ?e, "error connecting to console");
                };
                self.state
                    .get(&uid)
                    .expect("state to be present")
                    .notification_tx
                    .clone()
            }
        };
        let uid_clone = uid.clone();
        spawn_named("track disconnects", async move {
            let _ = rx.await;
            info!(
                datasource = %uid_clone.0,
                path = ?p,
                "Client disconnected for datasource",
            );
            if sender
                .send(Notification {
                    message: ConnectMessage::Disconnected,
                })
                .await
                .is_err()
            {
                warn!(
                    datasource = %uid_clone.0,
                    path = ?p,
                    "Could not send disconnect notification for datasource",
                );
            };
        });
        let stream = match path {
            Path::Tasks => self.stream_tasks(&uid).await,
            Path::Resources(task_id) => self.stream_resources(&uid, task_id).await,
        };
        Box::pin(ClientDisconnect(stream, tx))
    }

    async fn publish_stream(
        &self,
        _request: backend::PublishStreamRequest,
    ) -> backend::PublishStreamResponse {
        debug!("Publishing to stream is not implemented");
        unimplemented!()
    }
}

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
    async fn query_data(&self, request: backend::QueryDataRequest) -> Self::Iter {
        Box::new(request.queries.into_iter().map(move |x| {
            let uid = request
                .plugin_context
                .datasource_instance_settings
                .as_ref()
                .map(|ds| ds.uid.clone());

            let mut frame = data::Frame::new("");

            if let Some(uid) = &uid {
                if let Ok(path) =
                    serde_json::from_value(x.json).map(|req: ConsoleQueryDataRequest| req.path)
                {
                    frame.set_channel(format!("ds/{}/{}", uid, path).parse().unwrap());
                }
            }

            Ok(backend::DataResponse::new(
                x.ref_id,
                vec![frame.check().unwrap()],
            ))
        }))
    }
}

#[track_caller]
pub fn spawn_named<T>(
    _name: &str,
    task: impl std::future::Future<Output = T> + Send + 'static,
) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    tokio::task::Builder::new().name(_name).spawn(task)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            serde_json::from_str::<ConsoleQueryDataRequest>(r#"{"path": "task", "taskId": 1}"#)
                .unwrap(),
            ConsoleQueryDataRequest {
                path: Path::TaskDetails { task_id: TaskId(1) }
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
