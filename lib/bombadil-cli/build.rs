use std::fs;
use std::path::Path;
use std::process::Stdio;

use bombadil_driver_plugin::{DriverRegistration, render_typescript};

include!("driver_set.rs");

fn main() {
    let dist_directory = Path::new("../../target/inspect");

    println!("cargo:rerun-if-changed=../bombadil-inspect/src");
    println!("cargo:rerun-if-changed=../bombadil-inspect/Cargo.toml");
    println!("cargo:rerun-if-changed=../bombadil-inspect/index.html");
    println!("cargo:rerun-if-changed=../bombadil-inspect/Trunk.toml");

    build_inspect(dist_directory);
    build_driver_types();
}

fn build_driver_types() {
    println!("cargo:rerun-if-changed=driver_set.rs");
    let drivers: &[DriverRegistration] = builtin_drivers!();
    let declarations = render_typescript(drivers.iter().copied())
        .expect("failed to render built-in driver TypeScript schemas");
    let output = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").expect("OUT_DIR is not set"),
    )
    .join("bombadil-driver-types.d.ts");
    fs::write(output, declarations)
        .expect("failed to write built-in driver TypeScript schemas");
}

fn build_inspect(dist_directory: &Path) {
    let inspect_directory = Path::new("../bombadil-inspect");

    println!("cargo:rerun-if-env-changed=BOMBADIL_SKIP_INSPECT_BUILD");
    if std::env::var_os("BOMBADIL_SKIP_INSPECT_BUILD").is_some() {
        ensure_placeholder(dist_directory);
        return;
    }

    if !inspect_directory.join("Cargo.toml").exists() {
        ensure_placeholder(dist_directory);
        return;
    }

    let dist_absolute = fs::canonicalize("../../")
        .expect("Failed to resolve workspace root")
        .join("target/inspect");

    let wasm_target_directory = fs::canonicalize("../../")
        .expect("Failed to resolve workspace root")
        .join("target/inspect-wasm");

    let mut command = std::process::Command::new("trunk");
    command
        .arg("build")
        .arg("--offline")
        .arg("--dist")
        .arg(&dist_absolute)
        .env("CARGO_TARGET_DIR", &wasm_target_directory)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .current_dir(inspect_directory);

    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile == "release" {
        command.arg("--release");
    }

    let status = command.status().expect("trunk command failed");

    if !status.success() {
        panic!("cargo:warning=trunk build failed");
    }
}

fn ensure_placeholder(dist_directory: &Path) {
    if dist_directory.join("index.html").exists() {
        return;
    }
    fs::create_dir_all(dist_directory)
        .expect("Failed to create inspect dist directory");
    fs::write(
        dist_directory.join("index.html"),
        "<!DOCTYPE html>\
         <html><body>\
         <h1>Bombadil Inspect</h1>\
         <p>Inspect UI not built. \
         Install trunk, then rebuild.</p>\
         </body></html>",
    )
    .expect("Failed to write placeholder index.html");
}
