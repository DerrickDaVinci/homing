# Security

Homing moves folders and rewrites other apps' records, so anything that can lose or corrupt data gets priority.

## Reporting a problem

Please **do not open a public issue** for a vulnerability. Use GitHub's private reporting instead:
**Security → Report a vulnerability** on this repository.

Helpful reports include the Homing version, the operating system, the agent versions (Claude Code, the Claude app, Codex), what you moved and what broke.

## What counts

- A folder, transcript or database ending up lost, truncated or unreadable.
- Undo not restoring the original state.
- A path being rewritten that does not belong to the moved project.
- Homing sending anything over the network. It must never do this.

## Design guarantees

- No network access, no telemetry, no update checks.
- Nothing changes without a confirmed preview (`--apply` on the command line).
- Folders are renamed, never copied and deleted. Moves across disks are refused.
- Files are replaced atomically (written to a temporary file, then renamed).
- The small index files are backed up before each move; the rewrite of transcripts is exactly reversible.
