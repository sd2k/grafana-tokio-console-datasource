use std::{
    env::{self, args},
    error::Error,
    process::Command,
};

fn main() -> Result<(), Box<dyn Error>> {
    match args().nth(1).as_deref() {
        Some("watch") => watch()?,
        _ => print_help(),
    };
    Ok(())
}

fn print_help() {
    eprintln!(
        "Tasks:
watch            watch for changes, then compile plugin, replace in `dist` directory, and restart plugin process
"
    )
}

fn go_target() -> Result<String, Box<dyn Error>> {
    env::var("GOARCH").or_else(|_| {
        let go_output = Command::new("go")
            .arg("version")
            .output()
            .map_err(|_| "go must be installed to fetch target host and arch; alternatively set GOARCH env var to e.g. darwin_arm64 or linux_amd64")?;
        Ok(String::from_utf8(go_output.stdout)?.trim().split(' ').nth(3).map(|s| s.replace("/", "_")).ok_or("unexpected output from `go version`")?)
    })
}

fn watch() -> Result<(), Box<dyn Error>> {
    let target = go_target()?;
    let shell_cmd = format!(
        "rm -rf ../dist/gpx_grafana-tokio-console-app_{0} && cp ./target/debug/gpx_grafana-tokio-console ../dist/gpx_grafana-tokio-console-app_{0} && pkill -HUP gpx_grafana-tokio-console-app_{0}",
        target,
    );
    let mut handle = Command::new("cargo")
        .arg("watch")
        .arg("-w")
        .arg("plugin")
        .arg("-x")
        .arg("clippy")
        .arg("-x")
        .arg("build")
        .arg("-s")
        .arg(&shell_cmd)
        .arg("-c")
        .spawn()?;
    Ok(handle.wait().map(|_| ())?)
}
