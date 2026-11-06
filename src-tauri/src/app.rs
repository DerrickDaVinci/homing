//! The desktop app: settings, the menu bar item, the homing ring and the
//! commands the UI calls. All real work lives in `core`.

use crate::core::{self, Env, Plan};
use crate::ring::{self, Modifier};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    /// Where active projects go; cold ones go to `<root>/Archive`.
    root: String,
    /// Projects untouched for this many days are suggested for the archive.
    archive_days: u32,
    lang: String,
    theme: String,
    /// Hold a key while dragging folders to open the homing ring (macOS).
    ring: bool,
    ring_modifier: Modifier,
    /// Dropping on the ring opens a preview instead of moving right away.
    ring_preview: bool,
    launch_at_login: bool,
    start_hidden: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let root = dirs::home_dir().map(|h| core::s(&h.join("Projects"))).unwrap_or_default();
        Settings {
            root,
            archive_days: 30,
            lang: "auto".into(),
            theme: "auto".into(),
            ring: true,
            ring_modifier: Modifier::Option,
            ring_preview: false,
            launch_at_login: false,
            start_hidden: false,
        }
    }
}

struct Store {
    env: Env,
    settings: Mutex<Settings>,
    tray: Mutex<Option<[MenuItem<tauri::Wry>; 3]>>,
}

impl Store {
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
    /// Destination for a folder: the root, or its Archive subfolder.
    fn target(&self, from: &str, archive: bool) -> String {
        let root = PathBuf::from(self.settings().root);
        let dir = if archive { root.join("Archive") } else { root };
        core::s(&dir.join(Path::new(from).file_name().unwrap_or_default()))
    }
    fn zh(&self) -> bool {
        match self.settings().lang.as_str() {
            "zh" => true,
            "en" => false,
            _ => sys_locale_zh(),
        }
    }
}

fn sys_locale_zh() -> bool {
    ["LC_ALL", "LANG"].iter().any(|k| std::env::var(k).is_ok_and(|v| v.starts_with("zh"))) || {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("defaults")
                .args(["read", "-g", "AppleLanguages"])
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).split('"').nth(1).is_some_and(|l| l.starts_with("zh")))
        }
        #[cfg(not(target_os = "macos"))]
        false
    }
}

fn settings_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

#[derive(Deserialize, Serialize, Clone)]
struct Move {
    from: String,
    archive: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    version: String,
    platform: &'static str,
    home: String,
    desktop: String,
    settings: Settings,
}

#[derive(Serialize)]
struct Scan {
    desktop_ok: bool,
    projects: Vec<core::Project>,
    running: core::Running,
}

// ------------------------------------------------------------------ commands

#[tauri::command]
fn info(app: AppHandle, store: State<Store>) -> Info {
    Info {
        version: app.package_info().version.to_string(),
        platform: if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "linux" },
        home: core::s(&store.env.home),
        desktop: core::s(&store.env.desktop),
        settings: store.settings(),
    }
}

#[tauri::command]
fn set_settings(app: AppHandle, store: State<Store>, settings: Settings) {
    let old = store.settings();
    if let Some(p) = settings_file(&app) {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(p, serde_json::to_vec_pretty(&settings).unwrap_or_default());
    }
    if old.launch_at_login != settings.launch_at_login {
        let al = app.autolaunch();
        let _ = if settings.launch_at_login { al.enable() } else { al.disable() };
    }
    *store.settings.lock().unwrap() = settings.clone();
    relabel_tray(&store);
    let _ = app.emit("settings-changed", &settings);
}

#[tauri::command]
async fn scan(store: State<'_, Store>) -> Result<Scan, ()> {
    let ix = core::Index::load(&store.env);
    let root = store.settings().root;
    let projects = core::projects(&store.env, &ix).into_iter().filter(|p| p.path != root).collect();
    Ok(Scan { desktop_ok: core::desktop_readable(&store.env), projects, running: core::running() })
}

#[tauri::command]
async fn preview(store: State<'_, Store>, moves: Vec<Move>) -> Result<Vec<Plan>, ()> {
    let ix = core::Index::load(&store.env);
    let live = core::live();
    Ok(moves.iter().map(|m| core::plan(&store.env, &ix, &live, &m.from, &store.target(&m.from, m.archive))).collect())
}

/// Moves one by one. A move that cannot happen comes back with its blockers.
#[tauri::command]
async fn relocate(app: AppHandle, store: State<'_, Store>, moves: Vec<Move>) -> Result<Vec<Plan>, ()> {
    let out = do_relocate(&store, &moves);
    let _ = app.emit("moved", ());
    Ok(out)
}

fn do_relocate(store: &Store, moves: &[Move]) -> Vec<Plan> {
    let live = core::live();
    moves
        .iter()
        .map(|m| {
            let to = store.target(&m.from, m.archive);
            core::relocate(&store.env, &live, &m.from, &to).unwrap_or_else(|e| failed(&m.from, &to, e))
        })
        .collect()
}

fn failed(from: &str, to: &str, e: String) -> Plan {
    Plan { from: from.into(), to: to.into(), blockers: e.split("; ").map(Into::into).collect(), ..Default::default() }
}

#[tauri::command]
fn history(store: State<Store>) -> Vec<core::Record> {
    core::history(&store.env)
}

#[tauri::command]
async fn undo(app: AppHandle, store: State<'_, Store>, time: u64) -> Result<Plan, ()> {
    let out = do_undo(&store, time);
    let _ = app.emit("moved", ());
    Ok(out)
}

fn do_undo(store: &Store, time: u64) -> Plan {
    let h = core::history(&store.env);
    let (from, to) = h.iter().find(|r| r.time == time).map(|r| (r.plan.to.clone(), r.plan.from.clone())).unwrap_or_default();
    core::undo(&store.env, &core::live(), time).unwrap_or_else(|e| failed(&from, &to, e))
}

#[tauri::command]
async fn pick_root(app: AppHandle) -> Option<String> {
    app.dialog().file().blocking_pick_folder().and_then(|f| f.into_path().ok()).map(|p| core::s(&p))
}

#[tauri::command]
fn reveal(path: String) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg("-R").arg(&path).spawn();
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = path;
}

// ------------------------------------------------------------------ the ring

const RING: f64 = 300.0;

fn build_ring(app: &AppHandle) {
    let _ = WebviewWindowBuilder::new(app, "ring", WebviewUrl::App("ring.html".into()))
        .title("Homing")
        .inner_size(RING, RING)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .accept_first_mouse(true)
        .visible(false)
        .build();
}

fn monitor_at(app: &AppHandle, x: f64, y: f64) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().ok().unwrap_or_default();
    let inside = |m: &tauri::Monitor| {
        let (p, s) = (m.position(), m.size());
        x >= p.x as f64 && x < p.x as f64 + s.width as f64 && y >= p.y as f64 && y < p.y as f64 + s.height as f64
    };
    match monitors.iter().position(inside) {
        Some(i) => monitors.into_iter().nth(i),
        None => app.primary_monitor().ok().flatten(),
    }
}

fn show_ring(app: &AppHandle) {
    let Some(w) = app.get_webview_window("ring") else { return };
    let Ok(p) = app.cursor_position() else { return };
    let monitor = monitor_at(app, p.x, p.y);
    let sf = monitor.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
    let area = monitor.as_ref().map(|m| {
        let wa = m.work_area();
        [wa.position.x as f64, wa.position.y as f64, wa.size.width as f64, wa.size.height as f64]
    });
    let (x, y) = ring_origin(p.x, p.y, RING * sf, area);
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = w.emit("ring-show", ());
    let _ = w.show();
}

/// Top-left of the ring window: centred on the cursor, but pushed back inside
/// the screen's usable area (menu bar and Dock excluded) near an edge.
fn ring_origin(cx: f64, cy: f64, size: f64, area: Option<[f64; 4]>) -> (f64, f64) {
    let (mut x, mut y) = (cx - size / 2.0, cy - size / 2.0);
    if let Some([l, t, w, h]) = area {
        x = x.max(l).min((l + w - size).max(l));
        y = y.max(t).min((t + h - size).max(t));
    }
    (x, y)
}

/// Folders dropped on the ring: "root", "archive" or "preview".
#[tauri::command]
async fn ring_drop(app: AppHandle, store: State<'_, Store>, action: String, paths: Vec<String>) -> Result<Vec<Plan>, ()> {
    let moves: Vec<Move> = paths.into_iter().map(|from| Move { from, archive: action == "archive" }).collect();
    if action == "preview" || store.settings().ring_preview {
        show_main(&app);
        let _ = app.emit_to("main", "open-preview", &moves);
        return Ok(vec![]);
    }
    let out = do_relocate(&store, &moves);
    let _ = app.emit("moved", ());
    Ok(out)
}

#[tauri::command]
fn ring_hide(app: AppHandle) {
    if let Some(w) = app.get_webview_window("ring") {
        let _ = w.hide();
    }
}

// ------------------------------------------------------------------ menu bar

fn tray_labels(zh: bool) -> [&'static str; 3] {
    if zh {
        ["打开 Homing", "撤销上一次归位", "退出 Homing"]
    } else {
        ["Open Homing", "Undo Last Move", "Quit Homing"]
    }
}

fn relabel_tray(store: &Store) {
    if let Some(items) = store.tray.lock().unwrap().as_ref() {
        for (item, label) in items.iter().zip(tray_labels(store.zh())) {
            let _ = item.set_text(label);
        }
    }
}

fn undo_last(app: &AppHandle) {
    let store = app.state::<Store>();
    if let Some(r) = core::history(&store.env).into_iter().find(|r| !r.undone) {
        let back = do_undo(&store, r.time);
        let _ = app.emit("moved", ());
        let _ = app.emit("undone", back);
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn run() {
    let Some(env) = Env::real() else {
        eprintln!("cannot find the home folder");
        return;
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let settings: Settings =
                settings_file(&handle).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
            if std::env::args().any(|a| a == "--hidden") || settings.start_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            } else {
                // A menu bar app is not activated on launch; bring the window forward.
                show_main(&handle);
            }
            app.manage(Store { env, settings: Mutex::new(settings), tray: Mutex::new(None) });
            let store = app.state::<Store>();

            let [l_open, l_undo, l_quit] = tray_labels(store.zh());
            let open = MenuItem::with_id(app, "open", l_open, true, None::<&str>)?;
            let undo = MenuItem::with_id(app, "undo", l_undo, true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", l_quit, true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &undo, &PredefinedMenuItem::separator(app)?, &quit])?;
            TrayIconBuilder::with_id("main")
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?)
                .icon_as_template(true)
                .tooltip("Homing")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "open" => show_main(app),
                    "undo" => undo_last(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            *store.tray.lock().unwrap() = Some([open, undo, quit]);

            build_ring(&handle);
            let on_ring = handle.clone();
            let ring_cfg = handle.clone();
            ring::spawn(
                move |signal| match signal {
                    ring::Signal::Show => {
                        let app = on_ring.clone();
                        let _ = on_ring.run_on_main_thread(move || show_ring(&app));
                    }
                    ring::Signal::Release => {
                        let _ = on_ring.emit_to("ring", "ring-release", ());
                    }
                },
                move || {
                    let s = ring_cfg.state::<Store>().settings();
                    s.ring.then_some(s.ring_modifier)
                },
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![info, set_settings, scan, preview, relocate, history, undo, pick_root, reveal, ring_drop, ring_hide])
        .on_window_event(|window, event| {
            // Closing the window keeps Homing in the menu bar, ready for the ring.
            if let (WindowEvent::CloseRequested { api, .. }, "main") = (event, window.label()) {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running Homing")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main(app);
            }
            let _ = (app, event);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    const AREA: Option<[f64; 4]> = Some([0.0, 50.0, 3000.0, 1800.0]); // 50px menu bar

    #[test]
    fn ring_stays_on_screen() {
        assert_eq!(ring_origin(1500.0, 900.0, 600.0, AREA), (1200.0, 600.0));
        assert_eq!(ring_origin(2995.0, 900.0, 600.0, AREA), (2400.0, 600.0)); // right edge
        assert_eq!(ring_origin(1500.0, 55.0, 600.0, AREA), (1200.0, 50.0)); // under the menu bar
        assert_eq!(ring_origin(2990.0, 1845.0, 600.0, AREA), (2400.0, 1250.0)); // bottom-right corner
    }

    #[test]
    fn old_settings_files_still_load() {
        let s: Settings = serde_json::from_str(r#"{"root":"/x","archiveDays":10}"#).unwrap();
        assert!(s.ring && !s.ring_preview && s.archive_days == 10);
    }
}
