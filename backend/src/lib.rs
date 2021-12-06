mod connection;
mod metadata;
mod plugin;
mod task;
mod util;

pub use plugin::ConsolePlugin;

#[track_caller]
fn spawn_named<T>(
    _name: &str,
    task: impl std::future::Future<Output = T> + Send + 'static,
) -> tokio::task::JoinHandle<T>
where
    T: Send + 'static,
{
    tokio::task::Builder::new().name(_name).spawn(task)
}
