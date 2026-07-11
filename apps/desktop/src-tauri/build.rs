fn main() {
    println!("cargo:rerun-if-changed=tauri.conf.json");
    println!("cargo:rerun-if-changed=binaries");
    println!("cargo:rerun-if-changed=icons/32x32.png");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    println!("cargo:rerun-if-changed=icons/128x128@2x.png");
    println!("cargo:rerun-if-changed=icons/icon.icns");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    if std::env::var_os("JYOWO_BUILDING_DAEMON_SIDECAR").is_some() {
        return;
    }
    validate_daemon_sidecar();
    tauri_build::build();
}

fn validate_daemon_sidecar() {
    let target = std::env::var("TARGET").expect("TARGET must be set by Cargo");
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by Cargo"),
    );
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let sidecar = manifest_dir
        .join("binaries")
        .join(format!("jyowo-harness-daemon-{target}{suffix}"));
    if !sidecar.is_file() {
        panic!(
            "missing Jyowo task daemon sidecar: {}. Run `pnpm build:daemon-sidecar` before desktop Cargo checks or Tauri builds.",
            sidecar.display()
        );
    }
}
