# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Updated `reqwest` to 0.13 (replaced `rustls-tls` feature with `rustls`).
- Removed standalone `serde_derive` dependency in favor of `serde` with `derive` feature.
- Updated `thiserror` to 2.x, `tokio` to 1.51.

## [0.2.0] - 2022-10-26

### Changed

- Updated to latest versions of `metrics` and `metrics-utils`.

## [0.1.0] - 2022-02-16

### Added

- Initial release.
- Supports `count` and `gauge` metrics, and manual submission of metrics.
- `histogram` metrics _might_ get reported as `summary` metrics, but I've never seen it work.
- Does not support automatic periodic metric submission.
