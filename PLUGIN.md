# Grafana Tokio Console Data Source

This is a streaming Grafana data source which can connect to the Tokio [`console`] subscriber. It provides similar functionality to the `console` [TUI frontend][console-frontend] (albeit more limited at the moment!). It currently supports the 'tasks list' and 'task details' data streams.

## Usage

You can create a new Tokio Console data source for each process that you want to connect to. The 

### Configuring the data source

Only two parameters are available for the data source:

- **URL** - The address of a console-enabled process to connect to. Equivalent to the `<TARGET_ADDR>` argument of the `console` TUI.
- **Retain for** - how long to continue displaying completed tasks and dropped resources after they have been closed. Equivalent to the `--retain-for` flag of the `console` TUI.

### Using the data source

The easiest way to use the data source is to import the default dashboards that come with it. To do so, navigate to the data source's settings page and click the 'Dashboards' tab, then click the 'Import' button for all available dashboards.

You can also create arbitrary visualisations of the (streaming) data using Grafana's standard panels + transforms. The recommended path to doing so is to take a look at the panels in the imported dashboards and make incremental changes to a copy of the dashboard.

[`console`]: https://github.com/tokio-rs/console
[console-frontend]: https://github.com/tokio-rs/console#extremely-cool-and-amazing-screenshots
