fn main() {
    // Only these two app commands exist; capabilities/launch.json grants them to the local
    // launch page alone (no remote URLs), so the daemon-served UI gets no IPC surface.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["launch_state", "retry"])),
    )
    .expect("tauri-build failed");
}
