# `libpostal-sys`

Low-level Rust bindings for the excellent address-parsing library [libpostal](https://github.com/openvenues/libpostal). Several other Rust wrappers for this library exist. This one includes the following features, which may or may not be available elsewhere:

- Bundled `libpostal` source code.
- Support for building static Rust binaries.
- Support for thread-safe initialization of `libpostal`, using provided global locks.
- Packing as a low-level `libpostal-sys` crate that can be shared between one or more high-level crates, as per standard Rust conventions.
- Support from cross-compiling from `x86_64` Macs to `aarch64` (Apple Silicon).

## Development notes

```sh
# Check out libpostal source code as a git submodule.
git submodule update --init

# Update our Rust API bindings manually.
bindgen wrapper.h -o src/bindings.rs
```
