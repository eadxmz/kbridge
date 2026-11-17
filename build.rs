use std::env;
use std::path::Path;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let manifest_path = Path::new("app.manifest").canonicalize();

    if target.contains("msvc") {
        if let Ok(path) = manifest_path {
            println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
            println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", path.display());
            println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:level='requireAdministrator'");
        }
    }
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=build.rs");
}
