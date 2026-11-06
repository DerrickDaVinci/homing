# Contributing

Thanks for helping. A few things keep Homing small and trustworthy.

## Setup

You need [Rust](https://rustup.rs), Node.js 18+, and on macOS the Xcode Command Line Tools.

```bash
npm install
npm run dev                 # run the app
cd src-tauri && cargo test  # run the tests
```

The UI is plain HTML, CSS and JS in `ui/` with no build step. Open `ui/index.html` in any browser to work on it: `ui/preview-mock.js` fakes the backend with sample data when the page runs outside the app.

## Where things live

| File | What it does |
|---|---|
| `src-tauri/src/core.rs` | Finding agent records, planning, moving, rewriting, undo, and the tests |
| `src-tauri/src/app.rs` | Settings and the commands the UI calls |
| `src-tauri/src/main.rs` | The `homing` command line |

## Supporting another agent

1. Find where it stores project paths, on macOS and Windows.
2. Load them in `Index::load`, count them in `projects` and `plan`, rewrite them in `apply`.
3. Extend the fake home in the tests (`fake()`) and make sure `move_then_undo_restores_everything` still restores every byte.
4. If the agent keeps its index in memory while running, add it to `running()` so Homing asks the user to quit it first.

## Ground rules

- **No network code.** Not for updates, analytics or crash reports.
- **Never delete the user's data.** Move, rewrite atomically, keep undo exact.
- **Prefer less code.** Use the standard library and the OS before adding a dependency. A new dependency needs a reason in the pull request.
- Keep pull requests focused, and run `cargo test` before opening one.
