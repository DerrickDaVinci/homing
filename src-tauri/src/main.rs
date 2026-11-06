// Hide the console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod app;
mod core;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("scan" | "move" | "undo" | "help" | "--help" | "-h") => std::process::exit(cli(&args)),
        _ => app::run(),
    }
}

const HELP: &str = "Homing: move AI-agent projects off the Desktop without losing their history.

  homing scan                          list Desktop projects and their agent sessions
  homing move <folder> [--to <dir>]    preview moving a folder (default: ~/Projects)
  homing move <folder> --apply         ...and do it
  homing undo                          put the last moved folder back

Nothing changes without --apply. Run with no arguments to open the app.";

fn cli(args: &[String]) -> i32 {
    let Some(env) = core::Env::real() else {
        eprintln!("cannot find the home folder");
        return 1;
    };
    match args[0].as_str() {
        "scan" => {
            let ix = core::Index::load(&env);
            for p in core::projects(&env, &ix) {
                let days = core::now().saturating_sub(p.active) / 86_400;
                println!("{:<36} {:>4}d ago  claude {:>3}  claude-app {:>3}  codex {:>4}", p.name, days, p.claude, p.claude_app, p.codex);
            }
            0
        }
        "move" => {
            let Some(from) = args.get(1) else {
                eprintln!("{HELP}");
                return 2;
            };
            let from = std::fs::canonicalize(from).map(|p| core::s(&p)).unwrap_or_else(|_| from.clone());
            let dir = args.iter().position(|a| a == "--to").and_then(|i| args.get(i + 1)).map(Into::into).unwrap_or_else(|| env.home.join("Projects"));
            let name = std::path::Path::new(&from).file_name().unwrap_or_default();
            let to = core::s(&dir.join(name));
            if args.iter().any(|a| a == "--apply") {
                match core::relocate(&env, &core::live(), &from, &to) {
                    Ok(p) => {
                        print_plan(&p);
                        p.errors.iter().for_each(|e| eprintln!("warning: {e}"));
                        0
                    }
                    Err(e) => {
                        eprintln!("not moved: {e}");
                        1
                    }
                }
            } else {
                let p = core::plan(&env, &core::Index::load(&env), &core::live(), &from, &to);
                print_plan(&p);
                println!("{}", if p.blockers.is_empty() { "preview only: add --apply to move" } else { "cannot move now" });
                i32::from(!p.blockers.is_empty())
            }
        }
        "undo" => match core::history(&env).into_iter().find(|r| !r.undone) {
            Some(r) => match core::undo(&env, &core::live(), r.time) {
                Ok(p) => {
                    print_plan(&p);
                    0
                }
                Err(e) => {
                    eprintln!("not undone: {e}");
                    1
                }
            },
            None => {
                eprintln!("nothing to undo");
                1
            }
        },
        _ => {
            println!("{HELP}");
            0
        }
    }
}

fn print_plan(p: &core::Plan) {
    println!("{}\n  -> {}", p.from, p.to);
    println!("  Claude Code sessions {}, Claude app sessions {}, Codex conversations {}", p.claude, p.claude_app, p.codex);
    p.blockers.iter().for_each(|b| println!("  blocked: {b}"));
    p.notes.iter().for_each(|n| println!("  note: {n}"));
}
