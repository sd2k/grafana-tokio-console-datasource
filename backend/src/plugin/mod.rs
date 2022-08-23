use std::{
    collections::{HashMap, HashSet},
    fmt,
    io::Cursor,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use chrono::prelude::*;
use console_api::{
    instrument::Update,
    tasks::{task_details::PollTimesHistogram, TaskDetails},
};
use dashmap::DashMap;
use futures::{StreamExt, TryStreamExt};
use grafana_plugin_sdk::{backend, data, prelude::*};
use humantime::format_duration;
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, trace, warn};

use crate::{
    connection::Connection,
    metadata::{MetaId, Metadata},
    spawn_named,
    task::{Task, TaskId},
};

#[path = "data.rs"]
mod data_service_impl;
mod diagnostics;
mod resource;
mod stream;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DatasourceUid(String);

impl fmt::Display for DatasourceUid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("stream already running")]
    StreamAlreadyRunning,

    #[error("missing task ID")]
    MissingTaskId,
    #[error("invalid task ID: {0}")]
    InvalidTaskId(String),
    #[error("task with ID {} not found", (.0).0)]
    TaskNotFound(TaskId),

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
    #[serde(rename = "taskHistogram", rename_all = "camelCase")]
    TaskHistogram { task_id: TaskId },
    #[serde(rename = "resources")]
    Resources,
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tasks => write!(f, "tasks"),
            Self::TaskDetails { task_id } => write!(f, "task/{}", task_id),
            Self::TaskHistogram { task_id } => write!(f, "taskHistogram/{}", task_id),
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
            (Some("taskHistogram"), None) => Err(Error::MissingTaskId),
            (Some("taskHistogram"), Some(task_id)) => task_id
                .parse()
                .map(|id| Self::TaskHistogram {
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

#[derive(Debug)]
enum TaskDetailsStream {
    NotFound,
    Connected(JoinHandle<()>),
    Removed,
}

/// An instance of a Console datasource.
///
/// This is moved into a spawned task, and communicates back
/// to the plugin using channels.
#[derive(Debug)]
struct ConsoleInstance {
    connection: Connection,

    task_detail_tasks: HashMap<TaskId, TaskDetailsStream>,

    notifications: mpsc::Receiver<Notification>,
    stream_count: usize,
}

impl ConsoleInstance {
    fn should_unsubscribe(&self, task: &Task) -> bool {
        task.is_completed()
    }
}

#[derive(Debug)]
struct DatasourceState {
    uid: DatasourceUid,

    metas: HashMap<MetaId, Metadata>,
    tasks: HashMap<TaskId, Task>,
    // resources: HashMap<u64, Resource>,
    // resource_stats: HashMap<u64, Stats>,
    last_updated: Option<SystemTime>,

    /// The incoming stream of task updates, to be forwarded to subscribers as `Frame`s.
    ///
    /// This will be `None` if the stream has been taken by `stream_tasks`.
    tasks_frame_tx: Option<mpsc::Sender<Result<data::Frame, Error>>>,
    tasks_frame_rx: Option<mpsc::Receiver<Result<data::Frame, Error>>>,

    /// Map from task ID to sender of incoming task details.
    task_details_frame_txs: HashMap<TaskId, mpsc::Sender<Result<data::Frame, Error>>>,

    /// Map from task ID to sender of task histogram frames.
    task_details_histogram_frame_txs: HashMap<TaskId, mpsc::Sender<Result<data::Frame, Error>>>,

    /// The incoming stream of resource updates, to be forwarded to subscribers as `Frame`s.
    // resources_stream_tx: Option<mpsc::Sender<Result<data::Frame, Error>>>,
    // resources_stream_rx: Option<mpsc::Receiver<Result<data::Frame, Error>>>,
    notification_tx: mpsc::Sender<Notification>,
}

impl DatasourceState {
    /// Create a new `DatasourceState` for a datasource from an initial update.
    async fn new(
        datasource_uid: DatasourceUid,
        update: Update,
        retain_for: Option<Duration>,
    ) -> (Self, mpsc::Receiver<Notification>) {
        let (tasks_frame_tx, tasks_frame_rx) = mpsc::channel(128);
        let (notification_tx, notification_rx) = mpsc::channel(128);
        let mut s = DatasourceState {
            uid: datasource_uid,
            last_updated: None,
            metas: Default::default(),
            tasks: Default::default(),
            // resources: Default::default(),
            // resource_stats: Default::default(),
            tasks_frame_tx: Some(tasks_frame_tx),
            tasks_frame_rx: Some(tasks_frame_rx),
            task_details_frame_txs: Default::default(),
            task_details_histogram_frame_txs: Default::default(),
            // resources_stream_tx: Default::default(),
            // resources_stream_rx: Default::default(),
            notification_tx,
        };
        s.update(update, retain_for).await;
        (s, notification_rx)
    }

    /// Process and update using a general `Update`, which contains information about new and updated
    /// metadata and tasks.
    #[tracing::instrument(level = "debug", skip(self, update))]
    async fn update(&mut self, update: Update, retain_for: Option<Duration>) {
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
            let mut updated_ids = HashSet::with_capacity(task_update.new_tasks.len());
            debug!(new_tasks = task_update.new_tasks.len(), "Adding new tasks");
            for new_task in task_update.new_tasks {
                if let Some(task) = Task::from_proto(&self.metas, &mut stats_update, new_task) {
                    updated_ids.insert(task.id);
                    self.tasks.insert(task.id, task);
                }
            }
            debug!(updated_tasks = stats_update.len(), "Updating task stats");
            for (id, stats) in stats_update {
                if let Some(task) = self.tasks.get_mut(&TaskId(id)) {
                    updated_ids.insert(task.id);
                    task.stats = stats.into();
                }
            }

            // Add any tasks that are not yet done to the set of tasks to be sent.
            // This is required so the frontend doesn't remove them from the UI due to
            // apparent inactivity.
            for (id, task) in &self.tasks {
                if !task.is_completed() {
                    updated_ids.insert(*id);
                }
            }

            // Send updates to the main 'tasks' stream.
            let tasks_frame = self.get_tasks_frame(Some(&updated_ids), retain_for);
            if let Some(tx) = &self.tasks_frame_tx {
                if let Err(e) = tx.send(tasks_frame).await {
                    error!(datasource_uid = %self.uid.0, error = %e, "Error sending tasks frame; replacing channel");
                    let (tx, rx) = mpsc::channel(128);
                    self.tasks_frame_tx = Some(tx);
                    self.tasks_frame_rx = Some(rx);
                }
            }
            // Send updates to any single-task streams.
            for task_id in updated_ids {
                if let Some(tx) = self.task_details_frame_txs.get(&task_id) {
                    if tx.send(self.get_task_details_frame(task_id)).await.is_err() {
                        info!(
                            datasource_uid = %self.uid.0,
                            task_id = %task_id.0,
                            "Dropping task details transmitter for task",
                        );
                        self.task_details_frame_txs.remove(&task_id);
                    }
                }
            }
        }
    }

    /// Process a single task's `TaskDetails` update, which contains an updated
    /// histogram of poll times.
    #[tracing::instrument(level = "debug", skip(self, update))]
    async fn update_details(&mut self, update: TaskDetails) {
        if let TaskDetails {
            task_id: Some(id),
            poll_times_histogram: Some(histogram),
            ..
        } = update
        {
            let task_id = TaskId(id.id);
            // Use a bool to track this so we don't hold a mutable reference to
            // `self.tasks` after we're done with it.
            let mut should_send_histogram = false;

            if let Some(task) = self.tasks.get_mut(&task_id) {
                // Update our own state.
                trace!(task_id = %task_id, "Updating poll times histogram for task");
                task.histogram = match histogram {
                    PollTimesHistogram::LegacyHistogram(data) => {
                        hdrhistogram::serialization::Deserializer::new()
                            .deserialize(&mut Cursor::new(&data))
                            .ok()
                    }
                    PollTimesHistogram::Histogram(x) => {
                        hdrhistogram::serialization::Deserializer::new()
                            .deserialize(&mut Cursor::new(&x.raw_histogram))
                            .ok()
                    }
                };
                should_send_histogram = true;
            }

            // Send updates to any downstream consumers.
            if should_send_histogram {
                if let Some(tx) = self.task_details_histogram_frame_txs.get(&task_id) {
                    if tx
                        .send(self.get_task_histogram_frame(task_id))
                        .await
                        .is_err()
                    {
                        info!(
                            datasource_uid = %self.uid.0,
                            task_id = %task_id.0,
                            "Dropping task histogram transmitter for task",
                        );
                        self.task_details_histogram_frame_txs.remove(&task_id);
                    }
                }
            }
        }
    }

    /// Get a `Frame` containing the latest task data, optionally only for a subset of tasks.
    fn get_tasks_frame<'a, T>(
        &self,
        updated_ids: Option<T>,
        retain_for: Option<Duration>,
    ) -> Result<data::Frame, Error>
    where
        T: IntoIterator<Item = &'a TaskId>,
        T::IntoIter: ExactSizeIterator,
    {
        let now = SystemTime::now();
        let updated_ids = updated_ids.map(|x| x.into_iter());
        let len = updated_ids
            .as_ref()
            .map_or_else(|| self.tasks.len(), |x| x.len());
        let iter: Box<dyn Iterator<Item = &Task>> = match updated_ids {
            Some(ids) => Box::new(ids.filter_map(|id| self.tasks.get(id))),
            None => Box::new(self.tasks.values()),
        };
        let iter: Box<dyn Iterator<Item = &Task>> = match retain_for {
            None => iter,
            Some(retain_for) => Box::new(iter.filter(move |task| {
                task.stats
                    .dropped_at
                    .map(|d| {
                        let dropped_for = now.duration_since(d).unwrap_or_default();
                        retain_for > dropped_for
                    })
                    .unwrap_or(true)
            })),
        };

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
        let mut waker_counts = Vec::with_capacity(len);
        let mut waker_clones = Vec::with_capacity(len);
        let mut waker_drops = Vec::with_capacity(len);
        let mut last_wakes = Vec::with_capacity(len);
        let mut since_wakes = Vec::with_capacity(len);
        let mut self_wakes = Vec::with_capacity(len);
        let mut self_wake_percents = Vec::with_capacity(len);

        for task in iter {
            timestamps.push(now);
            ids.push(task.id.0);
            names.push(task.name.clone());
            targets.push(task.target.clone());
            fields.push(
                task.fields
                    .iter()
                    .map(|f| format!("{}={}", f.name, f.value))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            states.push(task.state().icon());
            locations.push(task.location.clone());

            polls.push(task.total_polls());
            poll_times_histograms.push(
                task.histogram
                    .as_ref()
                    .map(|_| serde_json::to_string(&task.make_chart_data(100).0).unwrap())
                    .unwrap_or_else(|| "[]".to_string()),
            );
            created_at.push(to_datetime(task.stats.created_at));
            dropped_at.push(task.stats.dropped_at.map(to_datetime));
            busy.push(self.last_updated.map(|x| task.busy(x)).map(as_nanos));
            last_poll_started.push(task.stats.last_poll_started.map(to_datetime));
            last_poll_ended.push(task.stats.last_poll_ended.map(to_datetime));
            idle.push(self.last_updated.map(|x| task.idle(x)).map(as_nanos));
            total.push(self.last_updated.map(|x| task.total(x)).map(as_nanos));
            wakes.push(task.wakes());
            waker_counts.push(task.waker_count());
            waker_clones.push(task.waker_clones());
            waker_drops.push(task.waker_drops());
            last_wakes.push(task.last_wake().map(to_datetime));
            since_wakes.push(
                self.last_updated
                    .and_then(|x| task.since_wake(x))
                    .map(as_nanos),
            );
            self_wakes.push(task.self_wakes());
            self_wake_percents.push(task.self_wake_percent());
        }
        let mut wake_percent_config = data::FieldConfig::default();
        wake_percent_config.description =
            Some("The percentage of this task's total wakeups that are self wakes.".to_string());
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
            waker_counts.into_field("Waker count"),
            waker_clones.into_field("Waker clones"),
            waker_drops.into_field("Waker drops"),
            last_wakes.into_opt_field("Last wake"),
            since_wakes.into_opt_field("Since last wake"),
            self_wakes.into_field("Self wakes"),
            self_wake_percents
                .into_field("Self wake percent")
                .with_config(wake_percent_config),
        ]);
        Ok(frame)
    }

    /// Get a `Frame` for a single task.
    fn get_task_details_frame(&self, id: TaskId) -> Result<data::Frame, Error> {
        self.get_tasks_frame(Some(&[id]), None)
    }

    /// Get a `Frame` holding the buckets and counts of the poll times histogram for
    /// a single task.
    fn get_task_histogram_frame(&self, id: TaskId) -> Result<data::Frame, Error> {
        let task = self.tasks.get(&id).ok_or(Error::TaskNotFound(id))?;
        let (chart_data, chart_metadata) = task.make_chart_data(101);
        let width = (chart_metadata.max_value - chart_metadata.min_value) as f64 / 101.0;
        let x: Vec<_> = chart_data
            .iter()
            .enumerate()
            .map(|x| {
                (x.0 % 5 == 0)
                    .then(|| {
                        let nanos = chart_metadata.min_value as f64 + (width * x.0 as f64);
                        format_duration(Duration::from_nanos(nanos as u64)).to_string()
                    })
                    .unwrap_or_default()
            })
            .collect();
        let fields = [x.into_field("x"), chart_data.into_field("y")];
        Ok(fields.into_frame(id.to_string()))
    }

    /// Convert this state into an owned `Frame`.
    fn to_frame(&self, path: &Path, retain_for: Option<Duration>) -> Result<data::Frame, Error> {
        match path {
            Path::Tasks => self.get_tasks_frame::<&[TaskId]>(None, retain_for),
            Path::TaskDetails { task_id } => self.get_task_details_frame(*task_id),
            Path::TaskHistogram { task_id } => self.get_task_histogram_frame(*task_id),
            Path::Resources => todo!(),
        }
    }
}

fn to_datetime(s: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(s)
}

fn as_nanos(d: Duration) -> Option<u64> {
    d.as_nanos()
        .try_into()
        .map_err(|e| error!(error = ?e, "Error getting duration as nanos"))
        .ok()
}

#[derive(Clone, Debug, Default)]
pub struct ConsolePlugin {
    state: Arc<DashMap<DatasourceUid, DatasourceState>>,
}

impl ConsolePlugin {
    /// Connect to a console backend and begin streaming updates from the console.
    ///
    /// This spawns a single task named 'manage connection' which takes ownership of the connection
    /// and handles any new data received from it.
    async fn connect(&self, datasource: backend::DataSourceInstanceSettings) -> Result<(), Error> {
        let datasource_uid = DatasourceUid(datasource.uid);
        let url = datasource
            .url
            .parse()
            .map_err(|_| Error::InvalidDatasourceUrl(datasource.url))?;
        let retain_for = datasource
            .json_data
            .get("retainFor")
            .and_then(|x| x.as_u64())
            .map(Duration::from_secs);

        info!(url = %url, retain_for = ?retain_for, "Connecting to console");
        let mut connection = Connection::new(url);
        // Get some initial state.
        let update = connection.next_update().await;
        let (instance_state, notification_rx) =
            DatasourceState::new(datasource_uid.clone(), update, retain_for).await;
        self.state.insert(datasource_uid.clone(), instance_state);

        // Spawn a task to continuously fetch updates from the console, and
        // update the datasource. Each update will also send messages to the
        // listeners associated with the state, if there are any.
        let state = Arc::clone(&self.state);
        let uid_clone = datasource_uid.clone();

        spawn_named("manage connection", async move {
            let mut instance = ConsoleInstance {
                connection,
                task_detail_tasks: Default::default(),
                notifications: notification_rx,
                stream_count: 0,
            };
            let (task_details_tx, mut task_details_rx) = mpsc::channel(256);
            loop {
                tokio::select! {

                    instrument_update = instance.connection.next_update() => {
                        if let Some(mut s) = state.get_mut(&uid_clone) {
                            // First, update the state with the new and updated task data.
                            s.update(instrument_update, retain_for).await;

                            // Next check if we need to subscribe to any new task details streams,
                            // or unsubscribe from any finished tasks.
                            debug!(n_tasks = s.tasks.len(), "Checking for old or new tasks");

                            for (task_id, task) in &s.tasks {

                                let stream_running = s.task_details_frame_txs.contains_key(task_id) ||
                                    s.task_details_histogram_frame_txs.contains_key(task_id);
                                let has_handle = instance.task_detail_tasks.contains_key(task_id);
                                let should_definitely_subscribe = stream_running && !has_handle;

                                // Remove any completed tasks.
                                if !stream_running && instance.should_unsubscribe(task) {
                                    instance.task_detail_tasks.entry(*task_id).and_modify(|t| {
                                        if let TaskDetailsStream::Connected(handle) = t {
                                            debug!(task_id = %task_id, "Unsubscribing from completed task details");
                                            handle.abort();
                                        }
                                        *t = TaskDetailsStream::Removed;
                                    });
                                } else if /* instance.should_subscribe(task) ||  */should_definitely_subscribe {
                                    match instance.connection.watch_details(task_id.0).await {
                                        Ok(stream) => {
                                            let tid = *task_id;
                                            let task_details_tx = task_details_tx.clone();
                                            debug!(task_id = %tid, ?should_definitely_subscribe, "Subscribing to task details");
                                            let handle = spawn_named("manage task details", async move {
                                                let mut stream = stream.map_err(|e| (e, tid));
                                                loop {
                                                    match stream.next().await {
                                                        Some(x) => if let Err(e) = task_details_tx.send(x).await {
                                                            warn!(task_id = %tid, error = %e, "Could not send task details; dropping");
                                                            return;
                                                        },
                                                        None => {
                                                            debug!(task_id = %tid, "Task details stream completed");
                                                            return
                                                        },
                                                    }
                                                }
                                            });
                                            instance.task_detail_tasks.insert(*task_id, TaskDetailsStream::Connected(handle));
                                        },
                                        Err(e) => {
                                            warn!(task_id = %task_id, error = %e, "Error subscribing to task details stream");
                                            instance.task_detail_tasks.insert(*task_id, TaskDetailsStream::NotFound);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Some(task_details_update) = task_details_rx.recv() => {
                        match task_details_update {
                            Ok(update) => {
                                if let Some(mut s) = state.get_mut(&uid_clone) {
                                    s.update_details(update).await;
                                }
                            },
                            Err((_, task_id)) => {
                                instance.task_detail_tasks.entry(task_id).and_modify(|t| {
                                    if let TaskDetailsStream::Connected(handle) = t {
                                        debug!(task_id = %task_id, "Shutting down task details stream");
                                        handle.abort();
                                    }
                                    *t = TaskDetailsStream::Removed;
                                });
                            }
                        }
                    }

                    notification = instance.notifications.recv() => {
                        if let Some(n) = notification {
                            use ConnectMessage::{Connected, Disconnected};
                            match n.message {
                                Connected => {
                                    instance.stream_count += 1;
                                    info!(stream_count = instance.stream_count, "New stream registered");
                                },
                                Disconnected => {
                                    instance.stream_count = instance.stream_count.saturating_sub(1);
                                    info!(stream_count = instance.stream_count, "Stream deregistered");
                                },
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

    async fn get_tasks(&self, datasource_uid: &DatasourceUid) -> Result<Vec<TaskId>, Error> {
        self.state
            .get(datasource_uid)
            .map(|s| s.tasks.keys().copied().collect())
            .ok_or(Error::DatasourceInstanceNotFound)
    }

    async fn stream_tasks(
        &self,
        datasource_uid: &DatasourceUid,
    ) -> Result<<Self as backend::StreamService>::Stream, <Self as backend::StreamService>::Error>
    {
        let state = self.state.get_mut(datasource_uid);
        state
            .and_then(|mut x| x.tasks_frame_rx.take())
            .ok_or(Error::DatasourceInstanceNotFound)
            .map(|x| {
                Box::pin(ReceiverStream::new(x).map(|res| {
                    res.and_then(|frame| {
                        frame
                            .check()
                            .map_err(Error::Data)
                            .and_then(|f| Ok(backend::StreamPacket::from_frame(f)?))
                    })
                })) as <Self as backend::StreamService>::Stream
            })
    }

    async fn stream_task_details(
        &self,
        datasource_uid: &DatasourceUid,
        task_id: TaskId,
    ) -> <Self as backend::StreamService>::Stream {
        let state = self.state.get_mut(datasource_uid);

        let (tx, rx) = mpsc::channel(128);
        state
            .ok_or(Error::DatasourceInstanceNotFound)
            .expect("state should be present for datasource")
            .task_details_frame_txs
            .insert(task_id, tx);
        Box::pin(ReceiverStream::new(rx).map(|res| {
            res.and_then(|frame| {
                frame
                    .check()
                    .map_err(Error::Data)
                    .and_then(|f| Ok(backend::StreamPacket::from_frame(f)?))
            })
        }))
    }

    async fn stream_task_histogram(
        &self,
        datasource_uid: &DatasourceUid,
        task_id: TaskId,
    ) -> <Self as backend::StreamService>::Stream {
        let state = self.state.get_mut(datasource_uid);

        let (tx, rx) = mpsc::channel(128);
        state
            .ok_or(Error::DatasourceInstanceNotFound)
            .expect("state should be present for datasource")
            .task_details_histogram_frame_txs
            .insert(task_id, tx);
        info!(
            ?task_id,
            "Inserted tx into task_details_histogram_frame_txs"
        );
        Box::pin(ReceiverStream::new(rx).map(|res| {
            res.and_then(|frame| {
                frame
                    .check()
                    .map_err(Error::Data)
                    .and_then(|f| Ok(backend::StreamPacket::from_frame(f)?))
            })
        }))
    }

    async fn stream_resources(
        &self,
        _datasource_uid: &DatasourceUid,
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
        retain_for: Option<Duration>,
    ) -> Option<Result<data::Frame, Error>> {
        self.state
            .get(datasource_uid)
            .map(|s| s.to_frame(path, retain_for))
    }
}
