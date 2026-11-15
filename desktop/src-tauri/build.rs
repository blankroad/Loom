use std::path::PathBuf;

fn main() {
    // UI build identity: `index.html` (served at `/ui/`) of the UI embedded in the bundled loomd, staged next to it
    // by desktop/scripts/stage-loomd.sh (verify-loomd proves the staged daemon serves these exact
    // bytes). Before navigating, the app compares it with the selected daemon's `/ui/` page, so
    // a reused older daemon can never silently show an old UI. Missing → empty (check reports it).
    let staged = PathBuf::from("binaries/loomd.ui-index.html");
    println!("cargo:rerun-if-changed={}", staged.display());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("ui-index.html");
    std::fs::write(&out, std::fs::read(&staged).unwrap_or_default()).expect("write ui-index.html");

    // Only these two app commands exist; capabilities/launch.json grants them to the local
    // launch page alone (no remote URLs), so the daemon-served UI gets no IPC surface.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["launch_state", "retry"])),
    )
    .expect("tauri-build failed");
}
