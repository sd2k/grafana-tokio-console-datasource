# Grafana Tokio Console Data Source

This is a streaming Grafana data source which can connect to the Tokio [`console`] subscriber. It provides similar functionality to the `console` [TUI frontend][console-frontend] (albeit more limited at the moment!).

## Screenshots

![image](https://user-images.githubusercontent.com/5464991/146618978-16a094cf-f313-46c3-86ca-c364b7865130.png)

![image](https://user-images.githubusercontent.com/5464991/146619018-bc1bbe41-7bf3-4a20-8732-a4855ad29bcc.png)

## Getting started

### Plugin frontend

At the repository root:

1. Install dependencies

   ```bash
   yarn install
   ```

2. Build plugin in development mode or run in watch mode

   ```bash
   yarn dev
   ```

   or

   ```bash
   yarn watch
   ```

3. Build plugin in production mode

   ```bash
   yarn build
   ```

### Plugin backend

Make sure you have a recent version of Rust (run `rustup update stable`), and install [`cargo-watch`].

Then run:

```bash
cargo xtask watch
```

This will run the `watch` task using the [`cargo-xtask`] pattern, which rebuilds the backend component on changes, copies the binary into the correct location, and restarts the plugin process (which Grafana subsequently restarts).

### Running Grafana

You'll need to clone a fork Grafana and run a specific branch to get some nice extra things working (namely the poll time histograms inside the main task list table):

1. Clone Grafana

   ```bash
   git clone git@github.com/sd2k/grafana
   ```

2. Check out the custom branch

   ```bash
   git checkout table-charts
   ```

3. Build the frontend

   ```bash
   yarn && yarn dev
   ```

   or, to watch for changes

   ```bash
   yarn && yarn watch
   ```

4. Change some config - make sure to change the 'plugins' path to the parent directory of this
   repo.

   ```bash
   cat <<EOF > conf/custom.ini
   app_mode = development
   [log]
   level = debug

   [paths]
   plugins = /Users/ben/repos/grafana-plugins  # or wherever you cloned this repo
   
   [plugins]
   plugin_admin_enabled = true
   ```

5. Run the Grafana backend

   ```bash
   make run
   ```

6. Add a Tokio Console datasource

   In your browser, navigate to http://localhost:3000/datasources/new, find the 'Tokio Console' datasource, and add a new instance Using the placeholder value of http://127.0.0.1:6669 should work; this datasource instance will connect to the plugin backend process itself, which is serving the console subscriber service.

   You can then head to the datasource's Dashboards tab and import the provided dashboards.

## Cross compiling

### From MacOS

1. Install the relevant cross compiler toolchains. Using Homebrew:

   ```bash
   brew tap messense/macos-cross-toolchains
   brew install armv7-unknown-linux-musleabihf
   brew install aarch64-unknown-linux-musl
   brew install x86_64-unknown-linux-musl
   brew install mingw-w64
   ```

2. Install the relevant Rust targets. Using `rustup`:

   ```bash
   rustup target add armv7-unknown-linux-musleabihf
   rustup target add aarch64-apple-darwin
   rustup target add x86_64-apple-darwin
   rustup target add aarch64-unknown-linux-musl
   rustup target add x86_64-unknown-linux-musl
   rustup target add x86_64-pc-windows-gnu
   ```

3. Run the following to compile the plugin in release mode for each target:

   ```bash
   CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER=armv7-unknown-linux-musleabihf-ld cargo build --release --target armv7-unknown-linux-musleabihf
   cargo build --release --target aarch64-apple-darwin
   cargo build --release --target x86_64-apple-darwin
   CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-unknown-linux-gnu-gcc cargo build --release --target aarch64-unknown-linux-gnu
   CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=aarch64-unknown-linux-musl-gcc cargo build --release --target aarch64-unknown-linux-musl
   CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo build --release --target x86_64-pc-windows-gnu
   ```

[`console`]: https://github.com/tokio-rs/console
[console-frontend]: https://github.com/tokio-rs/console#extremely-cool-and-amazing-screenshots
[`cargo-xtask`]: https://github.com/matklad/cargo-xtask
[`cargo-watch`]: https://github.com/watchexec/cargo-watch/

