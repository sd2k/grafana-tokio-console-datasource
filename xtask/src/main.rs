use std::{env, error::Error, process::Command};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let cmd = args.next();
    match cmd.as_deref() {
        Some("watch") => watch(args)?,
        _ => print_help(),
    };
    Ok(())
}

fn print_help() {
    eprintln!(
        "Tasks:
watch [release]      watch for changes, then compile plugin (optionally in release mode), replace in `dist` directory, and restart plugin process
"
    )
}

fn go_target() -> Result<String, Box<dyn Error>> {
    env::var("GOARCH").or_else(|_| {
        let go_output = Command::new("go")
            .arg("version")
            .output()
            .map_err(|_| "go must be installed to fetch target host and arch; alternatively set GOARCH env var to e.g. darwin_arm64 or linux_amd64")?;
        Ok(String::from_utf8(go_output.stdout)?.trim().split(' ').nth(3).map(|s| s.replace('/', "_")).ok_or("unexpected output from `go version`")?)
    })
}

fn watch(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let go_target = go_target()?;
    let (build_cmd, cargo_target) = if let Some("release") = args.next().as_deref() {
        ("build --release", "release")
    } else {
        ("build", "debug")
    };
    let shell_cmd = format!(
        "rm -rf ./dist/gpx_grafana-tokio-console-datasource_{0} && cp ./target/{1}/gpx_grafana-tokio-console ./dist/gpx_grafana-tokio-console-datasource_{0} && pkill -HUP gpx_grafana-tokio-console-datasource_{0}",
        go_target,
        cargo_target,
    );
    let mut handle = Command::new("cargo")
        .arg("watch")
        .arg("-w")
        .arg("backend")
        .arg("-x")
        .arg("clippy")
        .arg("-x")
        .arg(build_cmd)
        .arg("-s")
        .arg(&shell_cmd)
        .arg("-c")
        .spawn()?;
    Ok(handle.wait().map(|_| ())?)
}
