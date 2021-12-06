# Grafana Tokio Console Data Source

This is a Grafana data source which can connect to the Tokio [`console`] subscriber. It provides similar functionality to the `console` [TUI frontend][console-frontend].

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

You'll need to clone Grafana and run a specific branch to get some nice extra things working (namely the poll time histograms inside the main task list table):

1. Clone Grafana

   ```bash
   git clone git@github.com/grafana/grafana
   ```

2. Check out the custom branch

   ```bash
   git checkout table-charts-with-convert-to-json
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
   allow_loading_unsigned_plugins = grafana-tokio-console-app
   plugin_admin_enabled = true
   ```

5. Run the Grafana backend

   ```bash
   make run
   ```

6. Add a Tokio Console datasource

   In your browser, navigate to http://localhost:3000/datasources/new, find the 'Tokio Console' datasource, and add a new instance Using the placeholder value of http://127.0.0.1:6669 should work; this datasource instance will connect to the plugin backend process itself, which is serving the console subscriber service.

   You can then head to the datasource's Dashboards tab and import the provided dashboards.

[`console`]: https://github.com/tokio-rs/console
[console-frontend]: https://github.com/tokio-rs/console#extremely-cool-and-amazing-screenshots
[`cargo-xtask`]: https://github.com/matklad/cargo-xtask
[`cargo-watch`]: https://github.com/watchexec/cargo-watch/

