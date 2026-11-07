// Gimmich desktop: a window on your own Gimmich (Immich + Studio) server.
// First start shows a small setup page (src/index.html) that asks for the server address;
// after that the app opens the server directly. The menu has the Studio tools and the camera.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::path::PathBuf;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};

fn config_file(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().expect("config dir").join("server.json")
}

fn saved_server(app: &AppHandle) -> Option<String> {
    let text = fs::read_to_string(config_file(app)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v["server"].as_str().map(str::to_string)
}

/// Accept "imac.tailnet.ts.net", "https://imac:2283/photos" ... and keep only scheme + host + port.
fn normalise(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let u = Url::parse(&with_scheme).ok()?;
    if !matches!(u.scheme(), "https" | "http") || u.host_str().is_none() {
        return None;
    }
    Some(u.origin().ascii_serialization())
}

#[tauri::command]
fn save_server(app: AppHandle, server: String) -> Result<String, String> {
    let server = normalise(&server).ok_or("That doesn't look like a web address.")?;
    let path = config_file(&app);
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&path, serde_json::json!({ "server": server }).to_string()).map_err(|e| e.to_string())?;
    open(&app, "/");
    Ok(server)
}

#[tauri::command]
fn current_server(app: AppHandle) -> Option<String> {
    saved_server(&app)
}

fn open(app: &AppHandle, path: &str) {
    let Some(window) = app.get_webview_window("main") else { return };
    match saved_server(app).and_then(|s| Url::parse(&s).ok()).and_then(|s| s.join(path).ok()) {
        Some(url) => {
            let _ = window.navigate(url);
        }
        None => setup(app),
    }
}

fn setup(app: &AppHandle) {
    // back to the local setup page bundled inside the app (Windows serves app pages over http://tauri.localhost)
    let page = if cfg!(windows) { "http://tauri.localhost/index.html?change=1" } else { "tauri://localhost/index.html?change=1" };
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.navigate(Url::parse(page).expect("valid app url"));
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![save_server, current_server])
        .setup(|app| {
            let handle = app.handle().clone();
            let start = match saved_server(&handle).and_then(|s| Url::parse(&s).ok()) {
                Some(url) => WebviewUrl::External(url),
                None => WebviewUrl::App("index.html".into()),
            };
            let window = WebviewWindowBuilder::new(app, "main", start)
                .title("Gimmich")
                .inner_size(1280.0, 820.0)
                .min_inner_size(420.0, 500.0)
                .build()?;

            let item = |id: &str, text: &str, keys: Option<&str>| MenuItem::with_id(app, id, text, true, keys);
            let gimmich = Submenu::with_items(app, "Gimmich", true, &[
                &item("photos", "Photos", Some("CmdOrCtrl+1"))?,
                &item("studio", "Studio tools", Some("CmdOrCtrl+2"))?,
                &item("camera", "Camera", Some("CmdOrCtrl+3"))?,
                &item("book", "Photo book", None)?,
                &PredefinedMenuItem::separator(app)?,
                &item("reload", "Reload", Some("CmdOrCtrl+R"))?,
                &item("server", "Change server…", None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, Some("Quit Gimmich"))?,
            ])?;
            let edit = Submenu::with_items(app, "Edit", true, &[
                &PredefinedMenuItem::undo(app, None)?,
                &PredefinedMenuItem::redo(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::cut(app, None)?,
                &PredefinedMenuItem::copy(app, None)?,
                &PredefinedMenuItem::paste(app, None)?,
                &PredefinedMenuItem::select_all(app, None)?,
            ])?;
            window.set_menu(Menu::with_items(app, &[&gimmich, &edit])?)?;
            app.set_menu(Menu::with_items(app, &[&gimmich, &edit])?)?; // macOS shows the app-wide menu
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "photos" => open(app, "/photos"),
            "studio" => open(app, "/studio/"),
            "camera" => open(app, "/studio/camera/"),
            "book" => open(app, "/studio/?view=book"),
            "reload" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.eval("location.reload()");
                }
            }
            "server" => setup(app),
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running Gimmich");
}

#[cfg(test)]
mod tests {
    use super::normalise;

    #[test]
    fn addresses() {
        assert_eq!(normalise("imac.tail1234.ts.net").as_deref(), Some("https://imac.tail1234.ts.net"));
        assert_eq!(normalise(" https://imac.tail1234.ts.net/photos ").as_deref(), Some("https://imac.tail1234.ts.net"));
        assert_eq!(normalise("http://192.168.1.20:2283/studio/").as_deref(), Some("http://192.168.1.20:2283"));
        assert_eq!(normalise("ftp://x"), None);
        assert_eq!(normalise(""), None);
    }
}
