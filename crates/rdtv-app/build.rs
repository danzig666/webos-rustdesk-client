//! On the TV target, link against the bundled SDL-webOS backport (shipped in the IPK's
//! lib/ and found through the `$ORIGIN/../lib` rpath set in .cargo/config.toml).
use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap();
    println!("cargo:rerun-if-env-changed=RDTV_DEPS_DIR");
    if target.starts_with("armv7") {
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
        let deps = std::env::var("RDTV_DEPS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root.join("deps-out/webos"));
        println!("cargo:rustc-link-search=native={}", deps.join("lib").display());
    }
}
