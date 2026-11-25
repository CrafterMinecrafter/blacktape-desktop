fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().plugin(
        "torrent",
        tauri_build::InlinedPlugin::new().commands(&[
            "search_torrent_releases",
            "inspect_torrent_release",
            "resolve_torrent_album",
            "cancel_torrent_inspection",
        ]),
    ))
    .expect("failed to build Tauri application metadata");
}
