//! Move a project folder and keep every AI agent pointed at it.
//!
//! Claude Code, the Claude desktop app and Codex remember projects by absolute
//! path, so dragging a folder in Finder orphans its chat history. We move the
//! folder and rewrite exactly those references:
//!
//! * Claude Code: `~/.claude/projects/<encoded path>/` is renamed and the paths
//!   inside its `.jsonl` transcripts are rewritten.
//! * Claude desktop app: its session files under `<config dir>/Claude/`.
//! * Codex: `state_N.sqlite` (threads and project roots), the rollout files of
//!   those threads (`.jsonl` or `.jsonl.zst`), `config.toml` and friends.
//!
//! A reference is a JSON/TOML string that *starts with* the old path followed
//! by a separator or the closing quote. Prose that only mentions the path is
//! left alone. The rewrite is its own inverse, so undo is simply a move back.

use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR as SEP};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const KEEP_BACKUPS: usize = 10;

#[derive(Clone)]
pub struct Env {
    pub home: PathBuf,
    pub desktop: PathBuf,
    /// The Claude desktop app's data folder.
    pub claude_app: PathBuf,
    /// Our own folder: backups and the history of moves.
    pub data: PathBuf,
}

impl Env {
    pub fn real() -> Option<Env> {
        Some(Env {
            home: dirs::home_dir()?,
            desktop: dirs::desktop_dir()?,
            claude_app: dirs::config_dir()?.join("Claude"),
            data: dirs::data_dir()?.join("app.homing.desktop"),
        })
    }
    fn claude_projects(&self) -> PathBuf {
        self.home.join(".claude").join("projects")
    }
    fn codex(&self) -> PathBuf {
        self.home.join(".codex")
    }
}

// ------------------------------------------------------------------ paths

pub fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Path separators agents may write: Windows tools use either.
const SEPS: &[&str] = if cfg!(windows) { &["\\", "/"] } else { &["/"] };

/// The part of `path` below `root` ("" or "/sub/dir"), if `path` is inside it.
pub fn rest<'a>(path: &'a str, root: &str) -> Option<&'a str> {
    let r = path.strip_prefix(root)?;
    (r.is_empty() || SEPS.iter().any(|s| r.starts_with(s))).then_some(r)
}

/// Claude Code's folder name for a working directory: every character that is
/// not an ASCII letter or digit becomes "-" (one per UTF-16 unit, like JS).
pub fn claude_dir_name(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            (0..c.len_utf16()).for_each(|_| out.push('-'));
        }
    }
    out
}

fn mtime(p: &Path) -> u64 {
    fs::symlink_metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs())
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn children(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(ext))
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for p in children(dir) {
        if p.is_dir() {
            walk(&p, ext, out);
        } else if has_ext(&p, ext) {
            out.push(p);
        }
    }
}

// ------------------------------------------------------------------ text references

fn json_str(s: &str) -> String {
    let q = serde_json::to_string(s).unwrap_or_default();
    q[1..q.len() - 1].to_string()
}

/// Byte ranges of every quoted string that starts with `old` as a whole path.
fn refs(text: &[u8], old: &str) -> Vec<(usize, usize)> {
    let pat = format!("\"{}", json_str(old));
    let seps: Vec<String> = SEPS.iter().map(|s| json_str(s)).collect();
    memchr::memmem::find_iter(text, pat.as_bytes())
        .filter(|&i| {
            let after = &text[i + pat.len()..];
            (i == 0 || text[i - 1] != b'\\') && (after.starts_with(b"\"") || seps.iter().any(|s| after.starts_with(s.as_bytes())))
        })
        .map(|i| (i + 1, i + pat.len()))
        .collect()
}

pub fn mentions(text: &[u8], path: &str) -> bool {
    !refs(text, path).is_empty()
}

/// `text` with every path string under `old` moved under `new`, or None.
pub fn rewrite(text: &[u8], old: &str, new: &str) -> Option<Vec<u8>> {
    let hits = refs(text, old);
    if hits.is_empty() {
        return None;
    }
    let new = json_str(new);
    let mut out = Vec::with_capacity(text.len() + hits.len() * new.len());
    let mut at = 0;
    for (a, b) in hits {
        out.extend_from_slice(&text[at..a]);
        out.extend_from_slice(new.as_bytes());
        at = b;
    }
    out.extend_from_slice(&text[at..]);
    Some(out)
}

/// Rewrite one file line by line into a temporary copy, then swap it in
/// atomically, keeping permissions and zstd compression. Streaming keeps
/// memory flat: Codex rollouts can be gigabytes once decompressed. A JSON
/// string never spans lines, so no path is ever split.
fn rewrite_file(p: &Path, old: &str, new: &str) -> io::Result<bool> {
    let zst = has_ext(p, ".zst");
    let input = fs::File::open(p)?;
    let mut reader: Box<dyn BufRead> = if zst { Box::new(BufReader::new(zstd::Decoder::new(input)?)) } else { Box::new(BufReader::new(input)) };
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = p.with_file_name(format!(".{name}.homing-tmp"));
    let result = (|| {
        let file = BufWriter::new(fs::File::create(&tmp)?);
        let mut out: Box<dyn Write> = if zst { Box::new(zstd::Encoder::new(file, 3)?.auto_finish()) } else { Box::new(file) };
        let (mut line, mut changed) = (Vec::new(), false);
        while reader.read_until(b'\n', &mut line)? > 0 {
            match rewrite(&line, old, new) {
                Some(l) => {
                    changed = true;
                    out.write_all(&l)?;
                }
                None => out.write_all(&line)?,
            }
            line.clear();
        }
        out.flush()?;
        drop(out); // finishes the zstd frame
        Ok(changed)
    })();
    match result {
        Ok(true) => {
            if let Ok(m) = fs::metadata(p) {
                let _ = fs::set_permissions(&tmp, m.permissions());
            }
            fs::rename(&tmp, p).map(|_| true).inspect_err(|_| {
                let _ = fs::remove_file(&tmp);
            })
        }
        other => {
            let _ = fs::remove_file(&tmp);
            other
        }
    }
}

/// Rewrite many files on all cores. Reports (done, total) after each file and
/// returns the failures.
fn rewrite_all(files: &[PathBuf], old: &str, new: &str, progress: &(dyn Fn(usize, usize) + Sync)) -> Vec<String> {
    let (next, done, errors) = (AtomicUsize::new(0), AtomicUsize::new(0), Mutex::new(Vec::new()));
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(files.len());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(f) = files.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Err(e) = rewrite_file(f, old, new) {
                        errors.lock().unwrap().push(format!("{}: {e}", s(f)));
                    }
                    progress(done.fetch_add(1, Ordering::Relaxed) + 1, files.len());
                }
            });
        }
    });
    errors.into_inner().unwrap()
}

/// The first JSON string value of `key` in `text`.
fn json_value(text: &[u8], key: &str) -> Option<String> {
    json_value_at(text, key, memchr::memmem::find)
}

/// The last JSON string value of `key` in `text`.
fn json_value_last(text: &[u8], key: &str) -> Option<String> {
    json_value_at(text, key, memchr::memmem::rfind)
}

fn json_value_at(text: &[u8], key: &str, find: fn(&[u8], &[u8]) -> Option<usize>) -> Option<String> {
    let k = format!("\"{key}\":\"");
    let start = find(text, k.as_bytes())? + k.len() - 1;
    let mut i = start + 1;
    while i < text.len() {
        match text[i] {
            b'\\' => i += 2,
            b'"' => return serde_json::from_slice(&text[start..=i]).ok(),
            _ => i += 1,
        }
    }
    None
}

// ------------------------------------------------------------------ what the agents know

struct ClaudeDir {
    dir: PathBuf,
    /// The latest working directory in the newest transcript. A session can
    /// start in a scratch folder and be carried into a project later.
    cwd: String,
    sessions: usize,
    latest: u64,
}

struct Thread {
    cwd: String,
    rollout: Option<PathBuf>,
    updated: u64,
}

/// Every place on this machine where an agent stored a project path.
#[derive(Default)]
pub struct Index {
    claude: Vec<ClaudeDir>,
    /// Claude desktop app session files: (file, its cwd, content).
    app: Vec<(PathBuf, String, Vec<u8>)>,
    /// Claude desktop app indexes that may mention any project.
    app_index: Vec<(PathBuf, Vec<u8>)>,
    threads: Vec<Thread>,
    codex_db: Option<PathBuf>,
    codex_roots: Vec<String>,
    /// Codex config.toml and other small text files.
    codex_text: Vec<(PathBuf, Vec<u8>)>,
}

/// The last `n` bytes of a file (transcripts can be hundreds of MB).
fn read_tail(p: &Path, n: u64) -> Vec<u8> {
    use std::io::{Seek, SeekFrom};
    let mut buf = Vec::new();
    if let Ok(mut f) = fs::File::open(p) {
        let len = f.metadata().map_or(0, |m| m.len());
        if f.seek(SeekFrom::Start(len.saturating_sub(n))).is_ok() {
            let _ = f.take(n).read_to_end(&mut buf);
        }
    }
    buf
}

fn rollout_file(p: &str) -> Option<PathBuf> {
    let p = PathBuf::from(p);
    if p.exists() {
        return Some(p);
    }
    let zst = PathBuf::from(format!("{}.zst", p.display()));
    zst.exists().then_some(zst)
}

/// The newest `state_N.sqlite` Codex keeps.
fn codex_db(codex: &Path) -> Option<PathBuf> {
    children(codex)
        .into_iter()
        .filter_map(|p| {
            let n = p.file_name()?.to_str()?.strip_prefix("state_")?.strip_suffix(".sqlite")?.parse::<u32>().ok()?;
            Some((n, p))
        })
        .max_by_key(|(n, _)| *n)
        .map(|(_, p)| p)
}

impl Index {
    pub fn load(env: &Env) -> Index {
        let mut ix = Index::default();

        for dir in children(&env.claude_projects()).into_iter().filter(|p| p.is_dir()) {
            let mut logs: Vec<PathBuf> = children(&dir).into_iter().filter(|p| has_ext(p, ".jsonl")).collect();
            logs.sort_by_key(|p| std::cmp::Reverse(mtime(p)));
            let Some(cwd) = logs.iter().find_map(|p| json_value_last(&read_tail(p, 1 << 20), "cwd")) else { continue };
            let latest = logs.first().map_or(0, |p| mtime(p));
            ix.claude.push(ClaudeDir { dir, cwd, sessions: logs.len(), latest });
        }

        let mut app = Vec::new();
        walk(&env.claude_app.join("claude-code-sessions"), ".json", &mut app);
        ix.app = app.into_iter().filter_map(|p| { let t = fs::read(&p).ok()?; Some((p, json_value(&t, "cwd")?, t)) }).collect();
        let index = ["git-worktrees.json", "ccd-session-presets.json"].map(|n| env.claude_app.join(n));
        ix.app_index = index.into_iter().filter_map(|p| Some((p.clone(), fs::read(&p).ok()?))).collect();

        let codex = env.codex();
        if let Some(db) = codex_db(&codex) {
            if let Ok(c) = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY) {
                if let Ok(mut st) = c.prepare("SELECT cwd, rollout_path, updated_at FROM threads") {
                    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)));
                    for (cwd, rollout, updated) in rows.into_iter().flatten().flatten() {
                        ix.threads.push(Thread { cwd, rollout: rollout_file(&rollout), updated: updated.max(0) as u64 });
                    }
                }
                if let Ok(mut st) = c.prepare("SELECT path FROM project_roots") {
                    ix.codex_roots = st.query_map([], |r| r.get::<_, String>(0)).into_iter().flatten().flatten().collect();
                }
            }
            ix.codex_db = Some(db);
        }
        let mut text = vec![codex.join("config.toml"), codex.join(".codex-global-state.json")];
        for a in children(&codex.join("automations")) {
            text.extend(children(&a).into_iter().filter(|p| has_ext(p, ".toml")));
        }
        ix.codex_text = text.into_iter().filter_map(|p| Some((p.clone(), fs::read(&p).ok()?))).collect();
        ix
    }

    /// Transcript folders of `path` and its subfolders, each with the part of
    /// its name after the project's own (empty, or "-sub-dir"). The name says
    /// where a session started; a subfolder also needs a session inside `path`,
    /// since "My App" and "My-App" encode alike.
    fn claude_for<'a>(&'a self, path: &'a str) -> impl Iterator<Item = (&'a ClaudeDir, String)> + 'a {
        let own = claude_dir_name(path);
        self.claude.iter().filter_map(move |d| {
            let name = d.dir.file_name()?.to_string_lossy().into_owned();
            let tail = name.strip_prefix(&own)?;
            (tail.is_empty() || (tail.starts_with('-') && rest(&d.cwd, path).is_some())).then(|| (d, tail.to_string()))
        })
    }
    /// Session files that ran inside `path`.
    fn app_sessions<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.app.iter().filter(move |(_, cwd, _)| rest(cwd, path).is_some()).map(|(f, _, _)| f)
    }
    /// Every Claude desktop file to rewrite. A path that only appears in a
    /// session's "recent folders" list is left as is: harmless, and those
    /// files belong to unrelated sessions.
    fn app_for<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a PathBuf> + 'a {
        self.app_sessions(path).chain(self.app_index.iter().filter(move |(_, t)| mentions(t, path)).map(|(f, _)| f))
    }
    fn threads_for<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a Thread> + 'a {
        self.threads.iter().filter(move |t| rest(&t.cwd, path).is_some())
    }
    fn codex_text_for<'a>(&'a self, path: &'a str) -> impl Iterator<Item = &'a (PathBuf, Vec<u8>)> + 'a {
        self.codex_text.iter().filter(move |(_, t)| mentions(t, path))
    }
    fn codex_refs(&self, path: &str) -> bool {
        self.threads_for(path).next().is_some()
            || self.codex_roots.iter().any(|r| rest(r, path).is_some())
            || self.codex_text_for(path).next().is_some()
    }
}

// ------------------------------------------------------------------ desktop projects

#[derive(Serialize, Clone)]
pub struct Project {
    pub name: String,
    pub path: String,
    /// Last activity (seconds since 1970): files or any agent session.
    pub active: u64,
    pub claude: usize,
    pub claude_app: usize,
    pub codex: usize,
    pub git: bool,
}

/// False when the OS has not given us access to the Desktop (macOS asks once).
pub fn desktop_readable(env: &Env) -> bool {
    fs::read_dir(&env.desktop).is_ok()
}

pub fn projects(env: &Env, ix: &Index) -> Vec<Project> {
    let mut out: Vec<Project> = children(&env.desktop)
        .into_iter()
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let real_dir = fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());
            real_dir && !name.starts_with('.') && !name.ends_with(".app")
        })
        .map(|p| {
            let path = s(&p);
            let visible = children(&p).into_iter().filter(|c| !c.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
            let files = visible.map(|c| mtime(&c)).max().unwrap_or_else(|| mtime(&p));
            let claude: Vec<_> = ix.claude_for(&path).map(|(d, _)| d).collect();
            let threads: Vec<_> = ix.threads_for(&path).collect();
            let active = [files, claude.iter().map(|d| d.latest).max().unwrap_or(0), threads.iter().map(|t| t.updated).max().unwrap_or(0)];
            Project {
                name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                active: active.into_iter().max().unwrap_or(0),
                claude: claude.iter().map(|d| d.sessions).sum(),
                claude_app: ix.app_sessions(&path).count(),
                codex: threads.len(),
                git: p.join(".git").exists(),
                path,
            }
        })
        .collect();
    out.sort_by(|a, b| b.active.cmp(&a.active));
    out
}

// ------------------------------------------------------------------ what else is running

#[derive(Serialize, Clone, Copy, Default)]
pub struct Running {
    pub claude_app: bool,
    pub codex: bool,
}

#[cfg(windows)]
fn command(prog: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut c = std::process::Command::new(prog);
    c.creation_flags(0x0800_0000); // no console window
    c
}
#[cfg(not(windows))]
fn command(prog: &str) -> std::process::Command {
    std::process::Command::new(prog)
}

fn process_names() -> Vec<String> {
    #[cfg(windows)]
    let (out, strip) = (command("tasklist").args(["/FO", "CSV", "/NH"]).output(), true);
    #[cfg(not(windows))]
    let (out, strip) = (command("ps").args(["-axo", "comm="]).output(), false);
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    text.lines()
        .map(|l| {
            if strip {
                l.split("\",").next().unwrap_or("").trim_matches('"').trim_end_matches(".exe").to_string()
            } else {
                l.trim().rsplit('/').next().unwrap_or("").to_string()
            }
        })
        .collect()
}

/// The desktop apps hold their indexes in memory, so they must be closed
/// before we change them. The `codex` CLI writes the same database.
pub fn running() -> Running {
    let names = process_names();
    let has = |n: &str| names.iter().any(|x| x == n);
    Running { claude_app: has("Claude"), codex: has("ChatGPT") || has("Codex") || has("codex") }
}

/// Working directories of running programs (shells, agents, dev servers), as
/// (program, directory). On Windows a busy folder simply refuses to move.
fn process_cwds() -> Vec<(String, String)> {
    if cfg!(windows) {
        return vec![];
    }
    let out = command("lsof").args(["-nP", "-d", "cwd", "-Fpcn"]).output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let me = std::process::id().to_string();
    let (mut pid, mut cmd, mut found) = (String::new(), String::new(), Vec::new());
    for line in text.lines() {
        match line.split_at_checked(1) {
            Some(("p", v)) => pid = v.into(),
            Some(("c", v)) => cmd = v.into(),
            Some(("n", v)) if pid != me && cmd != "lsof" => found.push((cmd.clone(), v.to_string())),
            _ => {}
        }
    }
    found
}

/// What is running right now, read once per preview or move.
#[derive(Default)]
pub struct Live {
    pub run: Running,
    cwds: Vec<(String, String)>,
}

pub fn live() -> Live {
    Live { run: running(), cwds: process_cwds() }
}

impl Live {
    fn busy_in(&self, dir: &str) -> Vec<String> {
        let mut names: Vec<String> = self.cwds.iter().filter(|(_, c)| rest(c, dir).is_some()).map(|(n, _)| n.clone()).collect();
        names.sort();
        names.dedup();
        names
    }
}

// ------------------------------------------------------------------ plan

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Plan {
    pub from: String,
    pub to: String,
    /// Claude Code transcripts that follow the folder.
    pub claude: usize,
    /// Claude desktop app session files that are updated.
    pub claude_app: usize,
    /// Codex conversations that are updated.
    pub codex: usize,
    /// Why the move cannot happen now. Codes, translated by the UI.
    pub blockers: Vec<String>,
    /// Things worth knowing that don't stop the move.
    pub notes: Vec<String>,
    /// Problems met while applying (the folder itself did move).
    #[serde(default)]
    pub errors: Vec<String>,
}

pub fn plan(env: &Env, ix: &Index, live: &Live, from: &str, to: &str) -> Plan {
    let mut p = Plan { from: from.into(), to: to.into(), ..Default::default() };
    p.claude = ix.claude_for(from).map(|(d, _)| d.sessions).sum();
    p.claude_app = ix.app_sessions(from).count();
    p.codex = ix.threads_for(from).count();

    let src = Path::new(from);
    let protected = [env.home.clone(), env.desktop.clone(), env.home.join("Documents"), env.home.join("Downloads")];
    let b = &mut p.blockers;
    if !src.exists() {
        b.push("missing".into());
    } else if !fs::symlink_metadata(src).is_ok_and(|m| m.is_dir()) {
        b.push("notFolder".into());
    } else if protected.iter().any(|x| x == src) || src.parent().is_none() {
        b.push("protected".into());
    } else {
        if Path::new(to) == src {
            b.push("alreadyThere".into());
            return p;
        }
        if Path::new(to).exists() {
            b.push("exists".into());
        }
        if rest(to, from).is_some() {
            b.push("inside".into());
        }
        let busy = live.busy_in(from);
        if !busy.is_empty() {
            b.push(format!("busy:{}", busy.join(", ")));
        }
        if live.run.claude_app && ix.app_for(from).next().is_some() {
            b.push("quitClaude".into());
        }
        if live.run.codex && ix.codex_refs(from) {
            b.push("quitCodex".into());
        }
        if children(src).iter().any(|c| c.join("pyvenv.cfg").exists()) {
            p.notes.push("venv".into());
        }
        if is_git_linked(src) {
            p.notes.push("worktrees".into());
        }
    }
    p
}

/// A worktree, or a repository with linked worktrees: both store absolute paths.
fn is_git_linked(dir: &Path) -> bool {
    let g = dir.join(".git");
    g.is_file() || !children(&g.join("worktrees")).is_empty()
}

// ------------------------------------------------------------------ apply

/// Reports (files done, files total) while session records are rewritten.
pub type Progress<'a> = &'a (dyn Fn(usize, usize) + Sync);

/// Move `from` to `to`, bring every agent along and remember it for undo.
pub fn relocate(env: &Env, live: &Live, from: &str, to: &str, progress: Progress) -> Result<Plan, String> {
    let p = apply(env, live, from, to, progress)?;
    let mut h = history(env);
    h.insert(0, Record { time: now(), plan: p.clone(), undone: false });
    h.truncate(200);
    save_history(env, &h);
    Ok(p)
}

/// Move and rewrite. Re-checks everything first (pass a fresh `live()`), so a
/// stale preview can never cause harm.
fn apply(env: &Env, live: &Live, from: &str, to: &str, progress: Progress) -> Result<Plan, String> {
    let ix = Index::load(env);
    let mut p = plan(env, &ix, live, from, to);
    if !p.blockers.is_empty() {
        return Err(p.blockers.join("; "));
    }
    let name = Path::new(from).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    backup(env, &ix, from, &name).map_err(|e| format!("backup: {e}"))?;

    if let Some(parent) = Path::new(to).parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let cross_device = if cfg!(windows) { 17 } else { 18 };
    fs::rename(from, to).map_err(|e| if e.raw_os_error() == Some(cross_device) { "crossDevice".into() } else { e.to_string() })?;

    // Claude Code: rename each transcript folder; its files are rewritten below.
    let mut files = Vec::new();
    for (d, tail) in ix.claude_for(from) {
        let target = env.claude_projects().join(format!("{}{tail}", claude_dir_name(to)));
        let moved = if target.exists() { merge_dir(&d.dir, &target) } else { fs::rename(&d.dir, &target) };
        if let Err(e) = moved {
            p.errors.push(format!("{}: {e}", s(&d.dir)));
        }
        walk(if target.exists() { &target } else { &d.dir }, ".jsonl", &mut files); // subagents too
    }
    // The Claude app's sessions, Codex conversations and config, then the database.
    files.extend(ix.app_for(from).cloned());
    files.extend(ix.threads_for(from).filter_map(|t| t.rollout.clone()));
    files.extend(ix.codex_text_for(from).map(|(f, _)| f.clone()));
    p.errors.extend(rewrite_all(&files, from, to, progress));
    if let (Some(db), true) = (&ix.codex_db, ix.codex_refs(from)) {
        if let Err(e) = update_codex_db(db, from, to) {
            p.errors.push(format!("{}: {e}", s(db)));
        }
    }

    if is_git_linked(Path::new(to)) && !repair_worktrees(from, to) {
        p.errors.push("gitRepair".into());
    }
    Ok(p)
}

/// Point git at the new paths. A plain `git worktree repair` fixes the links
/// from inside the moved folder; worktrees that moved along with it (nested in
/// it) must be named by their new path or git marks them prunable.
fn repair_worktrees(from: &str, to: &str) -> bool {
    let git = |args: &[&str]| command("git").arg("-C").arg(to).args(args).output().ok().filter(|o| o.status.success());
    let list = git(&["worktree", "list", "--porcelain"]).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    // git reports resolved paths (/var is /private/var on macOS), so compare
    // against the resolved old location too.
    let from_path = Path::new(from);
    let real_from = from_path.parent().and_then(|d| fs::canonicalize(d).ok()).map(|d| s(&d.join(from_path.file_name().unwrap_or_default())));
    // git prints forward slashes on Windows too.
    let norm = |p: &str| if cfg!(windows) { p.replace('\\', "/") } else { p.to_string() };
    let (from_n, real_n) = (norm(from), real_from.as_deref().map(norm));
    let moved: Vec<String> = list
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .filter_map(|w| rest(w, &from_n).or_else(|| rest(w, real_n.as_deref()?)).map(|r| format!("{to}{r}")))
        .collect();
    let mut args = vec!["worktree", "repair"];
    args.extend(moved.iter().map(String::as_str));
    git(&args).is_some()
}

fn update_codex_db(db: &Path, from: &str, to: &str) -> io::Result<()> {
    let run = || -> rusqlite::Result<()> {
        let mut c = Connection::open(db)?;
        c.busy_timeout(Duration::from_secs(5))?;
        let tx = c.transaction()?;
        for (table, col) in [("threads", "cwd"), ("project_roots", "path")] {
            tx.execute(
                &format!("UPDATE {table} SET {col} = ?2 || substr({col}, length(?1) + 1) WHERE {col} = ?1 OR substr({col}, 1, length(?1) + 1) IN (?1 || '/', ?1 || ?3)"),
                (from, to, SEP),
            )?;
        }
        tx.commit()
    };
    run().map_err(io::Error::other)
}

/// Move the entries of `from` into the existing `to`, keeping what is there.
fn merge_dir(from: &Path, to: &Path) -> io::Result<()> {
    for c in children(from) {
        let dest = to.join(c.file_name().unwrap_or_default());
        if !dest.exists() {
            fs::rename(&c, &dest)?;
        }
    }
    let _ = fs::remove_dir(from); // only if now empty
    Ok(())
}

/// Copies of the small index files we are about to change. Transcripts are not
/// copied: they are large, replaced atomically, and the rewrite is reversible.
fn backup(env: &Env, ix: &Index, from: &str, label: &str) -> io::Result<()> {
    let base = format!("{}-{}", now(), label.replace(['/', '\\'], "_"));
    let dir = (1..).map(|i| env.data.join("backups").join(format!("{base}-{i}"))).find(|d| !d.exists()).unwrap_or_default();
    fs::create_dir_all(&dir)?;
    for (i, f) in ix.app_for(from).chain(ix.codex_text_for(from).map(|(f, _)| f)).enumerate() {
        let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        fs::copy(f, dir.join(format!("{i}-{name}")))?;
    }
    if let (Some(db), true) = (&ix.codex_db, ix.codex_refs(from)) {
        let c = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(io::Error::other)?;
        c.execute("VACUUM INTO ?1", [s(&dir.join("codex-state.sqlite"))]).map_err(io::Error::other)?;
    }
    let mut all = children(&env.data.join("backups"));
    all.sort();
    for old in all.iter().rev().skip(KEEP_BACKUPS) {
        let _ = fs::remove_dir_all(old);
    }
    Ok(())
}

// ------------------------------------------------------------------ history

#[derive(Serialize, Deserialize, Clone)]
pub struct Record {
    pub time: u64,
    pub plan: Plan,
    #[serde(default)]
    pub undone: bool,
}

pub fn history(env: &Env) -> Vec<Record> {
    fs::read(env.data.join("history.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save_history(env: &Env, h: &[Record]) {
    let _ = fs::create_dir_all(&env.data);
    let _ = fs::write(env.data.join("history.json"), serde_json::to_vec_pretty(h).unwrap_or_default());
}

/// Put a moved folder back where it was, agents included.
pub fn undo(env: &Env, live: &Live, time: u64, progress: Progress) -> Result<Plan, String> {
    let h = history(env);
    let r = h.iter().find(|r| r.time == time && !r.undone).ok_or("notFound")?;
    let back = apply(env, live, &r.plan.to, &r.plan.from, progress)?;
    let mut h = h;
    if let Some(r) = h.iter_mut().find(|r| r.time == time) {
        r.undone = true;
    }
    save_history(env, &h);
    Ok(back)
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    /// A path written with this platform's separator.
    fn p(s: &str) -> String {
        s.replace('/', SEP)
    }

    /// `{"k":"v",...}` with the values as paths.
    fn obj(pairs: &[(&str, &str)]) -> String {
        let body: Vec<String> = pairs.iter().map(|(k, v)| format!("\"{k}\":{}", serde_json::to_string(&p(v)).unwrap())).collect();
        format!("{{{}}}", body.join(","))
    }

    fn read_any(p: &Path) -> io::Result<Vec<u8>> {
        let b = fs::read(p)?;
        if has_ext(p, ".zst") {
            zstd::decode_all(&b[..])
        } else {
            Ok(b)
        }
    }

    const QUIET: Progress = &|_, _| {};

    /// Manual timing on a copy of a real rollout (it gets rewritten twice):
    /// HOMING_BENCH=<file> HOMING_DIR=<project> cargo test --release bench_rewrite -- --ignored --nocapture
    #[test]
    #[ignore]
    fn bench_rewrite() {
        let (f, dir) = (PathBuf::from(std::env::var("HOMING_BENCH").unwrap()), std::env::var("HOMING_DIR").unwrap());
        for (a, b) in [(dir.as_str(), "/NEW"), ("/NEW", dir.as_str())] {
            let t = std::time::Instant::now();
            assert!(rewrite_file(&f, a, b).unwrap());
            println!("{a} -> {b}: {:.1}s", t.elapsed().as_secs_f64());
        }
    }

    #[test]
    fn streaming_rewrite_keeps_compression_and_untouched_files() {
        let dir = std::env::temp_dir().join(format!("homing-stream-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let lines = "{\"cwd\":\"/D/a\"}\n{\"x\":1}\n{\"cwd\":\"/D/a/b\"}"; // no final newline
        let (z, plain, other) = (dir.join("r.jsonl.zst"), dir.join("r.jsonl"), dir.join("o.jsonl"));
        fs::write(&z, zstd::encode_all(lines.as_bytes(), 3).unwrap()).unwrap();
        fs::write(&plain, lines).unwrap();
        fs::write(&other, "{\"cwd\":\"/E\"}\n").unwrap();
        let seen = AtomicUsize::new(0);
        let errs = rewrite_all(&[z.clone(), plain.clone(), other.clone()], "/D/a", "/P/a", &|d, t| {
            assert_eq!(t, 3);
            seen.fetch_max(d, Ordering::Relaxed);
        });
        assert!(errs.is_empty() && seen.load(Ordering::Relaxed) == 3);
        let want = lines.replace("/D/a", "/P/a");
        assert_eq!(String::from_utf8(read_any(&z).unwrap()).unwrap(), want);
        assert_eq!(fs::read_to_string(&plain).unwrap(), want);
        assert_eq!(fs::read_to_string(&other).unwrap(), "{\"cwd\":\"/E\"}\n");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3, "no temporary files left behind");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrites_only_whole_paths() {
        let t = obj(&[("cwd", "/D/AI"), ("a", "/D/AI/x.rs"), ("b", "/D/AI Automation"), ("c", "see /D/AI")]);
        let out = String::from_utf8(rewrite(t.as_bytes(), &p("/D/AI"), &p("/P/AI")).unwrap()).unwrap();
        assert_eq!(out, obj(&[("cwd", "/P/AI"), ("a", "/P/AI/x.rs"), ("b", "/D/AI Automation"), ("c", "see /D/AI")]));
        assert!(rewrite(t.as_bytes(), &p("/D/A"), "/X").is_none());
        let quoted = br#"{"d":"\"/D/AI"}"#;
        assert!(rewrite(quoted, "/D/AI", "/P").is_none(), "an escaped quote is not the start of a string");
    }

    #[test]
    fn rewrite_is_reversible_with_odd_names() {
        for old in ["/D/AI Automation ", "/D/Commerce & Ops", "/D/工作台透视", "/D/a\"b"] {
            let (old, new) = (p(old), p("/P/new"));
            let t = format!("{{\"cwd\":{},\"x\":[{}]}}", serde_json::to_string(&old).unwrap(), serde_json::to_string(&format!("{old}{SEP}f")).unwrap());
            let there = rewrite(t.as_bytes(), &old, &new).unwrap();
            assert!(!String::from_utf8_lossy(&there).contains("D"), "{old}");
            assert_eq!(rewrite(&there, &new, &old).unwrap(), t.as_bytes(), "{old}");
        }
    }

    #[test]
    fn toml_keys_count_as_references() {
        let t = b"[projects.\"/D/MooMoo\"]\ntrust_level = \"trusted\"\n";
        assert_eq!(rewrite(t, "/D/MooMoo", "/P/MooMoo").unwrap(), b"[projects.\"/P/MooMoo\"]\ntrust_level = \"trusted\"\n");
    }

    #[test]
    fn claude_names_match_claude_code() {
        assert_eq!(claude_dir_name("/Users/x/Desktop/AI Automation "), "-Users-x-Desktop-AI-Automation-");
        assert_eq!(claude_dir_name("/Users/x/工作台"), "-Users-x----");
        assert_eq!(claude_dir_name("C:\\Users\\x\\a.b"), "C--Users-x-a-b");
    }

    #[test]
    fn rest_respects_boundaries() {
        assert_eq!(rest(&p("/a/b"), &p("/a/b")), Some(""));
        assert_eq!(rest(&p("/a/b/c"), &p("/a/b")), Some(p("/c").as_str()));
        assert_eq!(rest(&p("/a/bc"), &p("/a/b")), None);
    }

    #[test]
    fn reads_json_values() {
        assert_eq!(json_value(br#"{"type":"user","cwd":"/a/\"q\"/b","x":1}"#, "cwd").as_deref(), Some("/a/\"q\"/b"));
        assert_eq!(json_value(b"{}", "cwd"), None);
    }

    // ---------------------------------------------------------- end to end

    struct Fake {
        root: PathBuf,
        env: Env,
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn put(p: &Path, data: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }

    /// A home folder with one desktop project known to all three agents.
    fn fake(tag: &str) -> Fake {
        let root = std::env::temp_dir().join(format!("homing-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let home = root.join("home");
        let env = Env { desktop: home.join("Desktop"), claude_app: home.join("AppData/Claude"), data: home.join("AppData/homing"), home };
        let proj = env.desktop.join("My App");
        let p = s(&proj);
        put(&proj.join("README.md"), b"hi");

        let line = |cwd: &str| format!("{{\"type\":\"user\",\"cwd\":{},\"message\":\"edit {cwd}/x\"}}\n", serde_json::to_string(cwd).unwrap());
        put(&env.claude_projects().join(claude_dir_name(&p)).join("s1.jsonl"), line(&p).as_bytes());
        put(&env.claude_projects().join(claude_dir_name(&p)).join("memory/m.md"), b"note");
        put(&env.claude_projects().join(claude_dir_name(&p)).join("s1/subagents/a.jsonl"), line(&p).as_bytes());
        put(&env.claude_app.join("claude-code-sessions/a/b/local_3.json"), format!("{{\"cwd\":\"/elsewhere\",\"recents\":[{}]}}", serde_json::to_string(&p).unwrap()).as_bytes());
        let carried = format!("{}{}", line("/scratch/x"), line(&p));
        put(&env.claude_projects().join(claude_dir_name(&p)).join("s0.jsonl"), carried.as_bytes());
        let sub = s(&proj.join("web"));
        let deeper = s(&proj.join("web").join("src"));
        put(&env.claude_projects().join(claude_dir_name(&sub)).join("s2.jsonl"), line(&deeper).as_bytes());
        put(&env.claude_projects().join("-other").join("s3.jsonl"), line("/elsewhere").as_bytes());

        put(&env.claude_app.join("claude-code-sessions/a/b/local_1.json"), format!("{{\"cwd\":{}}}", serde_json::to_string(&p).unwrap()).as_bytes());
        put(&env.claude_app.join("claude-code-sessions/a/b/local_2.json"), b"{\"cwd\":\"/elsewhere\"}");

        let codex = env.codex();
        let rollout = codex.join("sessions/r1.jsonl");
        let zst = codex.join("sessions/r2.jsonl.zst");
        put(&rollout, line(&p).as_bytes());
        put(&zst, &zstd::encode_all(line(&sub).as_bytes(), 3).unwrap());
        put(&codex.join("config.toml"), format!("[projects.{}]\ntrust_level = \"trusted\"\n", serde_json::to_string(&p).unwrap()).as_bytes());
        let c = Connection::open(codex.join("state_5.sqlite")).unwrap();
        c.execute_batch("CREATE TABLE threads (id TEXT, rollout_path TEXT, cwd TEXT, updated_at INTEGER); CREATE TABLE project_roots (project_id TEXT, position INTEGER, path TEXT);").unwrap();
        c.execute("INSERT INTO threads VALUES ('t1', ?1, ?2, 5)", (s(&rollout), &p)).unwrap();
        c.execute("INSERT INTO threads VALUES ('t2', ?1, ?2, 6)", (s(&codex.join("sessions/r2.jsonl")), &sub)).unwrap();
        c.execute("INSERT INTO threads VALUES ('t3', 'none', '/D/My App 2', 7)", ()).unwrap();
        c.execute("INSERT INTO project_roots VALUES ('p1', 0, ?1)", [&p]).unwrap();
        Fake { root, env }
    }

    /// Every file under `dir` with its content, for before/after comparison.
    fn snapshot(env: &Env) -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        walk(&env.home, "", &mut files);
        let dir = &env.home;
        let mut out: Vec<_> = files
            .into_iter()
            .filter(|f| !f.starts_with(&env.data))
            .filter(|f| !has_ext(f, ".sqlite") && !has_ext(f, "-wal") && !has_ext(f, "-shm"))
            .map(|f| (s(f.strip_prefix(dir).unwrap()), read_any(&f).unwrap()))
            .collect();
        out.sort();
        out
    }

    fn db_rows(env: &Env) -> Vec<String> {
        let c = Connection::open(env.codex().join("state_5.sqlite")).unwrap();
        let mut st = c.prepare("SELECT cwd FROM threads UNION ALL SELECT path FROM project_roots").unwrap();
        st.query_map([], |r| r.get(0)).unwrap().flatten().collect()
    }

    #[test]
    fn finds_the_project_and_its_history() {
        let f = fake("scan");
        let ix = Index::load(&f.env);
        let ps = projects(&f.env, &ix);
        assert_eq!(ps.len(), 1);
        let p = &ps[0];
        assert_eq!((p.name.as_str(), p.claude, p.claude_app, p.codex), ("My App", 3, 1, 2));
    }

    #[test]
    fn move_then_undo_restores_everything() {
        let f = fake("move");
        let from = s(&f.env.desktop.join("My App"));
        let to = s(&f.env.home.join("Projects").join("My App"));
        let before = snapshot(&f.env);
        let rows_before = db_rows(&f.env);

        let ix = Index::load(&f.env);
        let preview = plan(&f.env, &ix, &Live::default(), &from, &to);
        assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
        assert_eq!((preview.claude, preview.claude_app, preview.codex), (3, 1, 2));
        assert!(Path::new(&from).exists(), "preview must not touch anything");

        let done = relocate(&f.env, &Live::default(), &from, &to, QUIET).unwrap();
        assert!(done.errors.is_empty(), "{:?}", done.errors);
        assert!(!Path::new(&from).exists() && Path::new(&to).join("README.md").exists());

        // Every agent now points at the new place.
        let ix = Index::load(&f.env);
        let moved = &projects(&Env { desktop: f.env.home.join("Projects"), ..f.env.clone() }, &ix)[0];
        assert_eq!((moved.claude, moved.claude_app, moved.codex), (3, 1, 2));
        assert!(f.env.claude_projects().join(claude_dir_name(&to)).join("memory/m.md").exists());
        let web = f.env.claude_projects().join(claude_dir_name(&s(&Path::new(&to).join("web"))));
        assert!(web.join("s2.jsonl").exists(), "named after where the session started, not where it ended");
        let sub = fs::read_to_string(f.env.claude_projects().join(claude_dir_name(&to)).join("s1/subagents/a.jsonl")).unwrap();
        assert!(sub.contains(&to), "subagent transcripts follow too");
        let toml = fs::read_to_string(f.env.codex().join("config.toml")).unwrap();
        assert!(toml.contains(&to) && !toml.contains(&from));
        assert!(db_rows(&f.env).contains(&s(&Path::new(&to).join("web"))));
        assert!(db_rows(&f.env).contains(&"/D/My App 2".to_string()), "neighbours untouched");
        assert!(fs::read_dir(f.env.data.join("backups")).unwrap().next().is_some());

        let h = history(&f.env);
        assert_eq!(h.len(), 1);
        undo(&f.env, &Live::default(), h[0].time, QUIET).unwrap();
        assert_eq!(snapshot(&f.env), before);
        assert_eq!(db_rows(&f.env), rows_before);
        let h = history(&f.env);
        assert!(h[0].undone && h.len() == 1, "undo marks the record instead of adding one");
    }

    /// A worktree inside the moved repository must follow it (it was left
    /// "prunable" before). Skipped when git is not installed.
    #[test]
    fn nested_worktrees_follow_the_repository() {
        let f = fake("worktree");
        let repo = f.env.desktop.join("My App");
        let git = |args: &[&str]| command("git").arg("-C").arg(&repo).args(args).output().map(|o| o.status.success()).unwrap_or(false);
        let ok = git(&["init", "-q"])
            && git(&["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "x"])
            && git(&["worktree", "add", "-q", ".wt/feature", "-b", "feature"]);
        if !ok {
            return; // no git here
        }
        let to = s(&f.env.home.join("Projects").join("My App"));
        relocate(&f.env, &Live::default(), &s(&repo), &to, QUIET).unwrap();
        let out = command("git").args(["-C", &to, "worktree", "list", "--porcelain"]).output().unwrap();
        let list = String::from_utf8_lossy(&out.stdout).replace('\\', "/");
        assert!(list.contains(&s(&Path::new(&to).join(".wt").join("feature")).replace('\\', "/")), "{list}");
        assert!(!list.contains("prunable"), "{list}");
    }

    #[test]
    fn refuses_unsafe_moves() {
        let f = fake("refuse");
        let ix = Index::load(&f.env);
        let from = s(&f.env.desktop.join("My App"));
        let blockers = |from: &str, to: &str, run| plan(&f.env, &ix, &Live { run, cwds: vec![] }, from, to).blockers;
        assert_eq!(blockers(&from, &s(&Path::new(&from).join("inner")), Running::default()), ["inside"]);
        assert_eq!(blockers(&from, &s(&f.env.desktop), Running::default()), ["exists"]);
        assert_eq!(blockers(&s(&f.env.desktop), "/x", Running::default()), ["protected"]);
        assert_eq!(blockers("/no/such", "/x", Running::default()), ["missing"]);
        assert_eq!(blockers(&from, &from, Running::default()), ["alreadyThere"]);
        assert_eq!(blockers(&from, "/x/y", Running { claude_app: true, codex: true }), ["quitClaude", "quitCodex"]);
    }
}
