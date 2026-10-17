//! On the TV target: link against the bundled SDL-webOS backport (shipped in the IPK's
//! lib/, found through the `$ORIGIN/../lib` rpath set in .cargo/config.toml) and add the
//! glibc compatibility object for the SDK's old sysroot.
use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap();
    println!("cargo:rerun-if-env-changed=RDTV_DEPS_DIR");
    println!("cargo:rerun-if-changed=csrc/glibc_compat.c");
    if target.starts_with("armv7") {
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
        let deps = std::env::var("RDTV_DEPS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root.join("deps-out/webos"));
        println!("cargo:rustc-link-search=native={}", deps.join("lib").display());
        // Compile to an object and pass it as a link argument so it comes after libstd in
        // the single-pass link (a static archive would be dropped before it is needed).
        let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let obj = out.join("glibc_compat.o");
        let cc = cc::Build::new().get_compiler();
        let status = cc
            .to_command()
            .args(["-O2", "-fPIC", "-c", "csrc/glibc_compat.c", "-o"])
            .arg(&obj)
            .status()
            .expect("compile glibc_compat.c");
        assert!(status.success(), "glibc_compat.c failed to compile");
        println!("cargo:rustc-link-arg={}", obj.display());
        println!("cargo:rustc-link-arg=-ldl");
    }
}
