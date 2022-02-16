# `metrics-exporter-newrelic`

A basic [`metrics`-compatible][metrics] exporter for NewRelic, using the [NewRelic Report Metrics API][newrelic-metrics-api]. This is available as an out-of-the-box option with [`opinionated_metrics`][opinionated_metrics].

[[**API documentation**](https://docs.rs/metrics-exporter-newrelic/)]

[metrics]: https://github.com/metrics-rs/metrics
[newrelic-metrics-api]: https://docs.newrelic.com/docs/data-apis/ingest-apis/metric-api/report-metrics-metric-api/
[opinionated_metrics]: https://docs.rs/opinionated_metrics/

## Who should consider using this

Overall, I have not been impressed by NewRelic's "custom metrics" support. NewRelic's documentation for custom metrics is insufficient, and many things fail in silent and impossible-to-diagnose ways.

You should only consider using this plugin if:

- You already use NewRelic for a bunch of other things.
- You want to include some basic metrics from Rust.

If you're setting up a greenfield deployment, I strongly recommend looking at [`metrics-exporter-prometheus`](https://docs.rs/metrics-exporter-prometheus/latest/metrics_exporter_prometheus/) and [Grafana](https://grafana.com/). They look like they have much richer support for converting custom metrics into high-quality dashboards.

## Current status

Here's where things stand:

- **Nice features**
  - [x] Global labels.
- **Export types**
  - [x] Manual exporting on demand.
  - [ ] Periodic exporting. (But you can write an async task that loops.)
- **Metric types**
  - [x] `count`
  - [x] `gauge`
  - ❓ `histogram` (This is mysterious. We collect the data and upload it to NewRelic as per the [docs][newrelic-metrics-api]. But it never shows up on any of their dashboards, and no errors get reported in `NrIntegrationError`. Also, NewRelic doesn't allow reporting real histograms via the Metrics API, only "summary" data like min/max/total/count. I am done trying to debug this for now.)

## Development note

I would happily contribute this exporter to the main `metrics` project, if they want to take it!
