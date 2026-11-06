//! The desktop app: settings and the commands the UI calls. All real work
//! lives in `core`.

use crate::core::{self, Env, Plan};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
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
}

impl Default for Settings {
    fn default() -> Self {
        let root = dirs::home_dir().map(|h| core::s(&h.join("Projects"))).unwrap_or_default();
        Settings { root, archive_days: 30, lang: "auto".into(), theme: "auto".into() }
    }
}

struct Store {
    env: Env,
    settings: Mutex<Settings>,
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
}

fn settings_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

#[derive(Deserialize)]
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
    if let Some(p) = settings_file(&app) {
        let _ = std::fs::create_dir_all(p.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(p, serde_json::to_vec_pretty(&settings).unwrap_or_default());
    }
    *store.settings.lock().unwrap() = settings;
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
async fn relocate(store: State<'_, Store>, moves: Vec<Move>) -> Result<Vec<Plan>, ()> {
    let live = core::live();
    Ok(moves
        .iter()
        .map(|m| {
            let to = store.target(&m.from, m.archive);
            core::relocate(&store.env, &live, &m.from, &to).unwrap_or_else(|e| failed(&m.from, &to, e))
        })
        .collect())
}

fn failed(from: &str, to: &str, e: String) -> Plan {
    Plan { from: from.into(), to: to.into(), blockers: e.split("; ").map(Into::into).collect(), ..Default::default() }
}

#[tauri::command]
fn history(store: State<Store>) -> Vec<core::Record> {
    core::history(&store.env)
}

#[tauri::command]
async fn undo(store: State<'_, Store>, time: u64) -> Result<Plan, ()> {
    let h = core::history(&store.env);
    let r = h.iter().find(|r| r.time == time);
    let (from, to) = r.map(|r| (r.plan.to.clone(), r.plan.from.clone())).unwrap_or_default();
    Ok(core::undo(&store.env, &core::live(), time).unwrap_or_else(|e| failed(&from, &to, e)))
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

pub fn run() {
    let Some(env) = Env::real() else {
        eprintln!("cannot find the home folder");
        return;
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let settings = settings_file(app.handle())
                .and_then(|p| std::fs::read(p).ok())
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            app.manage(Store { env, settings: Mutex::new(settings) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![info, set_settings, scan, preview, relocate, history, undo, pick_root, reveal])
        .run(tauri::generate_context!())
        .expect("error while running Homing");
}
