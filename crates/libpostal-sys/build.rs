//! Build script which builds libpostal from source and links it.
//!
//! We're not currently interested in trying to link a pre-existing system
//! version, because this is a fairly obscure library and it's not availble even
//! as an Ubuntu PPE.

use std::env;

/// The main entry point to our build script.
fn main() {
    build_libpostal();
    //bindgen_libpostal();
}

/// Use `autotools` to compile `libpostal` as a static library.
fn build_libpostal() {
    // Build `libpostal` and install it in `$OUT_DIR`.
    let mut config = autotools::Config::new("libpostal");

    // Instead of doing this, we ran `./bootstrap.sh` in `libpostal` manually,
    // and committed the output. This helps guarantee our source tree never
    // changes during `cargo publish`.
    //
    // if !Path::new("libpostal/configure").exists() {
    //     // Build `./configure` if it doesn't exist.
    //     config.reconf("-fi");
    // }

    // Get our Rust target and host.
    let rust_target = env::var("TARGET").expect("cargo should always define TARGET");
    let rust_host = env::var("HOST").expect("cargo should always define HOST");

    // When cross-compiling for M1 Macs, set `--host` appropriately. Keep in
    // mind that:
    //
    // - Rust `TARGET` is `./configure --host=`.
    // - Rust `HOST` is `./configure --build=`.
    // - `./configure --target=` is only used when building a compiler on
    //   `--build` that will run on `--host` and generate code for `--target`.
    //   But Rust _always_ supports generating code for multiple targets, so it
    //   doesn't think like this.
    //
    // I'm not sure why we need to set this manually, but it seems to be
    // necessary. We might need to do this for other combinations, but let's add
    // them as we discover them.
    if rust_target == "aarch64-apple-darwin" && rust_host != "aarch64-apple-darwin" {
        config.config_option("host", Some(&rust_target));
    }

    // If we're not on Intel, don't try to use Intel processor extensions. We
    // need to disable this manually, apparently.
    if !rust_target.starts_with("x86_64-") {
        config.disable("sse2", None);
    }

    let dst = config
        // You'll need to edit any `-Wall` out of the source tree,
        // unfortunately.
        .cflag("-Wno-error")
        // You'll need to download this manually and stick it in
        // `/usr/local/shared` or wherever the library expects it.
        .disable("data-download", None)
        // Don't allow automake, etc., to re-run. If this is allowed, it will
        // cause the input source tree to change and cause `cargo` to abort
        // publishing process. This required us to add `AM_MAINTAINER_MODE` to
        // `libpostal/configure.ac`.
        .disable("maintainer-mode", None)
        .config_option("datadir", Some("/usr/local/share/libpostal"))
        .build();

    // Emit linker arguments for `cargo`.
    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("lib").display(),
    );
    println!("cargo:rustc-link-lib=static=postal");
}

// We're doing this manually for now. See lib.rs.
//
// /// Use `bindgen` to generate a Rust version of `libpostal.h`. Note that this is
// /// very low-level, and it will require `unsafe` and the Rust C FFI to use. But
// /// at least we won't need to _declare_ the C header details.
// ///
// /// This is copied from https://rust-lang.github.io/rust-bindgen/tutorial-3.html
// /// and adapted only slightly.
// fn bindgen_libpostal() {
//     // Tell cargo to invalidate the built crate whenever the wrapper changes
//     println!("cargo:rerun-if-changed=wrapper.h");

//     // The bindgen::Builder is the main entry point
//     // to bindgen, and lets you build up options for
//     // the resulting bindings.
//     let bindings = bindgen::Builder::default()
//         // The input header we would like to generate
//         // bindings for.
//         .header("wrapper.h")
//         // Tell cargo to invalidate the built crate whenever any of the
//         // included header files changed.
//         .parse_callbacks(Box::new(bindgen::CargoCallbacks))
//         // Finish the builder and generate the bindings.
//         .generate()
//         // Unwrap the Result and panic on failure.
//         .expect("Unable to generate bindings");

//     // Write the bindings to the $OUT_DIR/bindings.rs file.
//     let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
//     bindings
//         .write_to_file(out_path.join("bindings.rs"))
//         .expect("Couldn't write bindings!");
// }
