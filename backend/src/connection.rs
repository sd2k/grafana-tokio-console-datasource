use console_api::instrument::{
    instrument_client::InstrumentClient, InstrumentRequest, PauseRequest, ResumeRequest,
    TaskDetailsRequest, Update,
};
use console_api::tasks::TaskDetails;
use futures::stream::StreamExt;
use std::{error::Error, pin::Pin, time::Duration};
use tonic::{transport::Channel, transport::Uri, Streaming};

#[derive(Debug)]
pub struct Connection {
    target: Uri,
    state: State,
}

#[derive(Debug)]
pub(crate) enum State {
    Connected {
        client: InstrumentClient<Channel>,
        stream: Box<Streaming<Update>>,
    },
    Disconnected(Duration),
}

macro_rules! with_client {
    ($me:ident, $client:ident, $block:expr) => ({
        loop {
            match $me.state {
                State::Connected { client: ref mut $client, .. } => {
                    match $block {
                        Ok(resp) => break Ok(resp),
                        // If the error is a `h2::Error`, that indicates
                        // something went wrong at the connection level, rather
                        // than the server returning an error code. In that
                        // case, let's try reconnecting...
                        Err(error) if error.source().iter().any(|src| src.is::<h2::Error>()) => {
                            tracing::warn!(
                                error = %error,
                                "connection error sending command"
                            );
                            $me.state = State::Disconnected(Self::BACKOFF);
                        }
                        // Otherwise, return the error.
                        Err(e) => {
                            break Err(e);
                        }
                    }
                }
                State::Disconnected(_) => $me.connect().await,
            }
        }
    })
}

impl Connection {
    const BACKOFF: Duration = Duration::from_millis(500);
    pub fn new(target: Uri) -> Self {
        Self {
            target,
            state: State::Disconnected(Duration::from_secs(0)),
        }
    }

    pub fn target(&self) -> &Uri {
        &self.target
    }

    pub(crate) async fn try_connect(target: Uri) -> Result<State, Box<dyn Error + Send + Sync>> {
        let mut client = InstrumentClient::connect(target.clone()).await?;
        let request = tonic::Request::new(InstrumentRequest {});
        let stream = Box::new(client.watch_updates(request).await?.into_inner());
        Ok(State::Connected { client, stream })
    }

    async fn connect(&mut self) {
        const MAX_BACKOFF: Duration = Duration::from_secs(5);

        while let State::Disconnected(backoff) = self.state {
            if backoff == Duration::from_secs(0) {
                tracing::debug!(to = %self.target, "connecting");
            } else {
                tracing::debug!(reconnect_in = ?backoff, "reconnecting");
                tokio::time::sleep(backoff).await;
            }
            self.state = match Self::try_connect(self.target.clone()).await {
                Ok(connected) => {
                    tracing::debug!("connected successfully!");
                    connected
                }
                Err(error) => {
                    tracing::warn!(%error, "error connecting");
                    let backoff = std::cmp::max(backoff + Self::BACKOFF, MAX_BACKOFF);
                    State::Disconnected(backoff)
                }
            };
        }
    }

    pub async fn next_update(&mut self) -> Update {
        loop {
            match self.state {
                State::Connected { ref mut stream, .. } => match Pin::new(stream).next().await {
                    Some(Ok(update)) => return update,
                    Some(Err(status)) => {
                        tracing::warn!(%status, "error from stream");
                        self.state = State::Disconnected(Self::BACKOFF);
                    }
                    None => {
                        tracing::error!("stream closed by server");
                        self.state = State::Disconnected(Self::BACKOFF);
                    }
                },
                State::Disconnected(_) => self.connect().await,
            }
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn watch_details(
        &mut self,
        task_id: u64,
    ) -> Result<Streaming<TaskDetails>, tonic::Status> {
        with_client!(self, client, {
            let request = tonic::Request::new(TaskDetailsRequest {
                id: Some(task_id.into()),
            });
            client.watch_task_details(request).await
        })
        .map(tonic::Response::into_inner)
    }

    #[tracing::instrument(skip(self))]
    pub async fn pause(&mut self) {
        let res = with_client!(self, client, {
            let request = tonic::Request::new(PauseRequest {});
            client.pause(request).await
        });

        if let Err(e) = res {
            tracing::error!(error = %e, "rpc error sending pause command");
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn resume(&mut self) {
        let res = with_client!(self, client, {
            let request = tonic::Request::new(ResumeRequest {});
            client.resume(request).await
        });

        if let Err(e) = res {
            tracing::error!(error = %e, "rpc error sending resume command");
        }
    }
}
