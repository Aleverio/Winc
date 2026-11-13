//! Language-server settings: automatic defaults and custom commands.

use super::*;

#[test]
fn automatic_server_selection_is_saved_per_language() {
    let dir = sample_folder("settings-lsp-auto");
    let path = dir.join("main.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();
    let mut app = app_with(&path, false);

    app.open_lsp_settings();
    let auto = Setting::LspAuto("rust");
    assert!(!app.switch_on(auto));

    app.settings_toggle(auto);
    assert!(app.switch_on(auto));
    assert_eq!(app.config.lsp_auto, ["rust"]);
    assert!(Config::parse(&app.config.to_json()).0.lsp_auto == ["rust"]);

    app.settings_toggle(auto);
    assert!(!app.switch_on(auto));
    assert!(app.config.lsp_auto.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
