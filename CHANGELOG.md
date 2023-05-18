# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.3.5] - 2023-05-18

### Fixed

- Fix broken release build caused by incorrect use of `cfg!` macro.

## [1.3.4] - 2023-05-18

### Changed

- Added lots of debugging assertions to help detect potential memory overflows. None of these were the actual cause of the recent bugs, but the ones that we left in were nice to have.

### Fixed

- Fixed hang/memory overflow where we forwarded `&[Address]` bocks with no addresses to the cache layer.
- Updated to latest `cross` and installed `protoc` inside the Docker container.

## [1.3.3] - 2023-05-11

### Security

- Update all dependencies to see if we can get a clean build with no security advisories. None of the advisories looked like they should affect normal usage, but it's better to fix them.

## [1.3.2] - 2023-05-11

### Fixed

- Do not attempt to geocode empty addresses. This has recently started causing SmartyStreets to return an error.

## [1.3.1] - 2022-10-26

### Changed

- The build process now requires at least `cmake 3.12` or so, which breaks Ubuntu 18.04 unless you use a PPA.
- We now require `protobuf-compiler` to build.

### Fixed

- Update dependencies to fix minor security advisories.
- Updated a bunch of dependencies, including BigTable client used for caching.

## [1.3.0] - 2022-06-29

### Fixed

- Retries now use exponential backoff. This means that we should now give up after about 30 seconds, instead of 10 or 12 like before.

### Added

- `--max-retries` can now be used to control how many times we retry before giving up.

## [1.2.0] - 2022-06-28

### Added

- `--max-addresses-per-second` allows specifying a rate limit for any external geocoder. This is particularly useful with services like Smarty that impose their own rate limits.

## [1.1.0] - 2022-02-23

### Added

- Report metric `geocodecsv.selected_errors.count`, which breaks down particularly interesting errors by `component` and `cause`. We focus on reporting this information for remote APIs, currently including BigTable and Smarty. This is meant to supplement the existing `geocodecsv.chunks_retried.total` and `geocodecsv.chunks_failed.total` metrics, which already report generic error statistics, but which can't say _what_ failed or _why_.

## [1.1.0-rc.1] - 2022-02-17

### Fixed

- Renabled Mac M1 builds after fixing cross-compilation of `libpostal`.

## [1.1.0-alpha.2] - 2022-02-16

### Changed

- Disabled Mac M1 builds until we get cross-compilation working with `libpostal`. This will allow us to test other platforms.

## [1.1.0-alpha.1] - 2022-02-16

### Changed

- Renamed `--cache-record-keys` to `--cache-output-keys`.

### Fixed

- Cleaned up packaging, build and CI.

## Internal-only releases - 2022-02-03

Internal-only releases identified as either 1.0.2 or 2.0.0-alpha.1. No official binaries were ever built, and this exact release does not exist in `git` history. Included for completeness.

### Added

- Support for "geocoding" using `libpostal`. This returns parsed and normalized address fields, but no lat/lon data. To use this, pass `--geocoder=libpostal`.
- Optionally normalize addresses using `libpostal` when using other geocoders.
- We can now cache data using either Redis or BigTable.
  - You can use `--cache-record-keys` to output the cache keys.
- We now calculate extensive geocoding metrics, and can either print them to standard output, or send them to NewRelic.

### Changed

- `SMARTY_AUTH_ID` is now preferred over `SMARTYSTREETS_AUTH_ID` (though both will be supported).
- `SMARTY_AUTH_TOKEN` is now preferred over `SMARTYSTREETS_AUTH_TOKEN` (though both will be supported).
- `--smarty-license` is now preferred over `--license` (though both will be supported).

## [1.0.2] - 2021-12-16

### Changed

- New naming convention for release ZIP files.

## [1.0.1] - 2021-12-15

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
