# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] - 2021-12-13

### Added

- New binary builds for ARM/M1 Macs.

### Changed

- The downloadable `*.zip` files now include both the CPU and the OS in the name. Downloading scripts will need to be adjusted.
- We now use `rustls` instead of OpenSSL internally. This shouldn't change anything, but it's a significant change.

### Fixed

- Restored missing binaries for existing platforms by switching to GitHub CI.

## [1.0.0] - 2021-12-13

Bumping number to v1.0.0 because this has been running fine in production for quite.

### Added

- Support `--match enhanced`. This only works for appropriate SmartyStreets plans.

### Fixed

- Updated dependencies to fix several CVEs reported by `cargo deny`. I do not believe that any of these CVEs actually affected `geocode-csv` in practice, but better safe than sorry.

## [0.3.0-beta.5] - 2021-05-21

### Changed

- Updated to latest `tokio` and `hyper` libraries. This represents a major change to some of our core libraries, but it means that we're finally on stable `tokio` and not a pre-release.
- Replaced error-reporting code, so some error output might look different.

## [0.3.0-beta.4] - 2021-05-20 [YANKED]

This build has released binaries, but the version number wasn't updated, at it still claims to be v0.3.0-beta.3. Since the code is identical, this isn't the end of the world.

### Fixed

- Fixed Travis CI build.

## [0.3.0-beta.3] - 2021-05-20

### Fixed

- Rebuild with modern Rust toolchain.

## [0.3.0-beta.2] - 2021-05-20

### Added

- Added a `--license` option to enable use of rooftop geocoding.

### Security

- Fixed a number of security advisories in supporting libraries. None of these appear to have been easily exploitable using an invalid CSV file as input.
