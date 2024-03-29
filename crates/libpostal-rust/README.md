# `libpostal-rust`: Yet Another `libpostal` Wrapper

This is another set of high-level bindings for the [libpostal](https://github.com/openvenues/libpostal) library. Several other Rust wrappers for this library exist. This one includes the following features, which may or may not be available elsewhere:

- No need to have `libpostal` installed.
- Support for building static Rust binaries.
- Support for thread-safe initialization of `libpostal`.
- Support from cross-compiling from `x86_64` Macs to `aarch64` (Apple Silicon), for use with GitHub CI builders and similar setups.

[API Documentation](https://docs.rs/libpostal-rust/)
