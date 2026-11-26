fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().plugin(
        "torrent",
        tauri_build::InlinedPlugin::new().commands(&[
            "search_torrent_releases",
            "inspect_torrent_release",
            "resolve_torrent_album",
            "cancel_torrent_inspection",
            "list_torrent_indexers",
            "set_torrent_indexer_enabled",
            "test_torrent_indexer",
            "open_rutracker_login",
        ]),
    ))
    .expect("failed to build Tauri application metadata");
}
