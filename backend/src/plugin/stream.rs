/// The `grafana_plugin_sdk::backend::StreamService` implementation for the Console plugin.
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::Stream;
use grafana_plugin_sdk::{backend, data};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::spawn_named;

use super::{ConnectMessage, ConsolePlugin, DatasourceUid, Error, Notification, Path};

/// Struct used to notify a receiver that a client has disconnected.
///
/// This wraps a returned `Stream` implementation (here, the one returned from
/// `backend::StreamService::run_stream`), and spawns a task which will notify a
/// channel when the returned struct is dropped.
struct ClientDisconnect<T>(T, oneshot::Sender<()>);

impl<T> ClientDisconnect<T> {
    fn new<F: FnOnce() + Send + 'static>(
        stream: T,
        notifier: mpsc::Sender<Notification>,
        on_disconnect: F,
    ) -> Self {
        let (tx, rx) = oneshot::channel();

        spawn_named(
            "grafana_tokio_console_app::plugin::stream::ClientDisconnect",
            async move {
                let _ = rx.await;
                on_disconnect();
                if notifier
                    .send(Notification {
                        message: ConnectMessage::Disconnected,
                    })
                    .await
                    .is_err()
                {
                    warn!("Could not send disconnect notification for datasource");
                };
            },
        );
        Self(stream, tx)
    }
}

impl<T, I> Stream for ClientDisconnect<T>
where
    T: Stream<Item = I> + std::marker::Unpin,
{
    type Item = I;
    fn poll_next(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(ctx)
    }
}

fn frame_to_initial_data(frame: data::Frame) -> Result<backend::InitialData, Error> {
    let checked = frame.check()?;
    let init = backend::InitialData::from_frame(checked, data::FrameInclude::All)?;
    Ok(init)
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
    ) -> Result<backend::SubscribeStreamResponse, Self::Error> {
        let path = request.path()?;
        let uid = request.datasource_uid()?;
        let datasource_settings = request
            .plugin_context
            .datasource_instance_settings
            .ok_or(Error::MissingDatasource)?;
        let retain_for = datasource_settings
            .json_data
            .get("retainFor")
            .and_then(|x| x.as_u64())
            .map(Duration::from_secs);

        // Check if we're already connected to this datasource instance and getting updates.
        // If so, we should just return the current state as `initial_data`.
        // If not, we should spawn a task to start populating the datasource instance's state.
        let initial_data = match self.initial_data(&uid, &path, retain_for) {
            Some(s) => s,
            None => {
                info!("No state found; connecting to console");
                self.connect(datasource_settings).await.map_err(|e| {
                    error!(error = ?e, "error connecting to console");
                    e
                })?;
                self.state
                    .get(&uid)
                    // Invariant: self.connect will create the state for this uid.
                    .expect("state to be present")
                    .to_frame(&path, retain_for)
            }
        };

        initial_data
            .and_then(frame_to_initial_data)
            .map(|x| backend::SubscribeStreamResponse::ok(Some(x)))
    }

    type Error = Error;
    type Stream = backend::BoxRunStream<Self::Error>;

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
    async fn run_stream(
        &self,
        request: backend::RunStreamRequest,
    ) -> Result<Self::Stream, Self::Error> {
        let path = request.path()?;
        let uid = request.datasource_uid()?;
        let datasource_settings = request
            .plugin_context
            .datasource_instance_settings
            .ok_or(Error::MissingDatasource)?;

        let sender = match self.state.get(&uid) {
            Some(s) => s.notification_tx.clone(),
            None => {
                self.connect(datasource_settings).await.map_err(|e| {
                    error!(error = ?e, "error connecting to console");
                    e
                })?;
                self.state
                    .get(&uid)
                    .expect("state to be present")
                    .notification_tx
                    .clone()
            }
        };
        let stream = match path {
            Path::Tasks => self.stream_tasks(&uid).await?,
            Path::TaskDetails { task_id } => self.stream_task_details(&uid, task_id).await,
            Path::TaskHistogram { task_id } => self.stream_task_histogram(&uid, task_id).await,
            Path::Resources => self.stream_resources(&uid).await,
        };
        if let Some(ref x) = self.state.get_mut(&uid) {
            if x.notification_tx
                .send(Notification {
                    message: ConnectMessage::Connected,
                })
                .await
                .is_err()
            {
                warn!(
                    datasource = %uid.0,
                    path = "tasks",
                    "Could not send connect notification",
                );
            };
        }
        Ok(Box::pin(ClientDisconnect::new(stream, sender, move || {
            info!(
                datasource = %uid,
                path = %path,
                "Client disconnected for datasource",
            )
        })))
    }

    async fn publish_stream(
        &self,
        _request: backend::PublishStreamRequest,
    ) -> Result<backend::PublishStreamResponse, Self::Error> {
        debug!("Publishing to stream is not implemented");
        unimplemented!()
    }
}

/// Extension trait providing some convenience methods for getting the `path` and `datasource_uid`.
trait StreamRequestExt {
    /// The path passed as part of the request, as a `&str`.
    fn raw_path(&self) -> &str;
    /// The datasource instance settings passed in the request.
    fn datasource_instance_settings(&self) -> Option<&backend::DataSourceInstanceSettings>;

    /// The parsed `Path`, or an `Error` if parsing failed.
    fn path(&self) -> Result<Path, Error> {
        let path = self.raw_path();
        path.parse()
            .map_err(|_| Error::UnknownPath(path.to_string()))
    }

    /// The datasource UID of the request, or an `Error` if the request didn't include
    /// any datasource settings.
    fn datasource_uid(&self) -> Result<DatasourceUid, Error> {
        self.datasource_instance_settings()
            .ok_or(Error::MissingDatasource)
            .map(|x| DatasourceUid(x.uid.clone()))
    }
}

macro_rules! impl_stream_request_ext {
    ($request: path) => {
        impl StreamRequestExt for $request {
            fn raw_path(&self) -> &str {
                self.path.as_str()
            }

            fn datasource_instance_settings(&self) -> Option<&backend::DataSourceInstanceSettings> {
                self.plugin_context.datasource_instance_settings.as_ref()
            }
        }
    };
}

impl_stream_request_ext!(backend::RunStreamRequest);
impl_stream_request_ext!(backend::SubscribeStreamRequest);
impl_stream_request_ext!(backend::PublishStreamRequest);
