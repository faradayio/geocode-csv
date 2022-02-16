# `opinionated_metrics`: Automatically configure `metrics` exporters

**The goal:** This library should provide reasonable defaults for collecting and exporting data from the Rust [`metrics`](https://github.com/metrics-rs/metrics) facade.

```rust
// Set up metrics reporting for a CLI tool, using environment variables.
let metrics_handle = opinionated_metrics::initialize_cli()?;

// Do stuff.

// Send a metrics report when the CLI tool has finished running.
metrics_handle.report().await?;
```

**The reality:** Right now, this works for CLI programs and it exports metrics to NewRelic and/or the logs. And we would honestly recommend against reporting to NewRelic unless you're already invested in it. (Hey, nobody ever promised this libraries opinions were _good_.) We do want to add Prometheus support, and better support for servers (and not just CLI programs that run and exit).

**Current users:** This library is intended for immediate production use at Faraday.

For more details, see the [API documentation](https://docs.rs/opinionated_metrics/).
