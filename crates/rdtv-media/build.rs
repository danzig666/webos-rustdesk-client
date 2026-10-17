//! Links the static libvpx/libopus produced by `scripts/build-deps.sh`.
//! Override the location with `RDTV_DEPS_DIR`.
use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap();
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let flavour = if target.starts_with("armv7") {
        "webos"
    } else {
        "host"
    };
    let deps = std::env::var("RDTV_DEPS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| root.join("deps-out").join(flavour));
    println!("cargo:rerun-if-env-changed=RDTV_DEPS_DIR");
    println!("cargo:rerun-if-changed=csrc/vpx_shim.c");
    println!("cargo:rerun-if-changed=csrc/smp_shim.c");
    let inc = deps.join("include");
    if !inc.join("vpx/vpx_decoder.h").exists() {
        panic!(
            "libvpx headers not found in {}. Run scripts/build-deps.sh {flavour} first.",
            inc.display()
        );
    }
    cc::Build::new()
        .file("csrc/vpx_shim.c")
        .include(&inc)
        .warnings(true)
        .compile("rdtv_vpx_shim");
    cc::Build::new()
        .file("csrc/smp_shim.c")
        .warnings(true)
        .compile("rdtv_smp_shim");
    println!("cargo:rustc-link-search=native={}", deps.join("lib").display());
    println!("cargo:rustc-link-lib=static=vpx");
    println!("cargo:rustc-link-lib=static=opus");
    println!("cargo:rustc-link-lib=dylib=m");
    println!("cargo:rustc-link-lib=dylib=pthread");
    println!("cargo:rustc-link-lib=dylib=dl");
}
