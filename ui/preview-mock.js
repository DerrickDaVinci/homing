// Browser preview only: when the page is opened outside the app (no
// window.__TAURI__), fake the backend with sample data so the UI can be
// designed in any browser. Inside the app this file does nothing.
(() => {
  if (window.__TAURI__) return;
  const now = Math.floor(Date.now() / 1000);
  const day = 86400;
  const home = '/Users/me';
  const p = (name, days, claude, claude_app, codex, git) => ({ name, path: `${home}/Desktop/${name}`, active: now - days * day, claude, claude_app, codex, git });
  const projects = [
    p('weather-bot', 0, 41, 38, 0, true),
    p('landing-page', 1, 3, 3, 12, true),
    p('invoice-parser', 4, 0, 0, 69, false),
    p('scratch-2026-10', 9, 0, 1, 0, false),
    p('esp32-clock', 25, 0, 0, 8, false),
    p('stock-notes', 57, 0, 0, 232, false),
    p('old-portfolio', 108, 0, 0, 0, true),
  ];
  let settings = { places: [{ label: 'Projects', path: `${home}/Projects` }, { label: 'Archive', path: `${home}/Projects/Archive` }], archiveDays: 30, lang: 'auto', theme: 'auto', accent: '', ring: true, ringKeys: ['shift'], ringPreview: false, launchAtLogin: false, startHidden: false };
  let history = [];
  const plan = (m) => {
    const pr = projects.find((x) => x.path === m.from) || { claude: 0, claude_app: 0, codex: 0, name: m.from.split('/').pop() };
    const to = `${(settings.places[m.place] || settings.places[0]).path}/${pr.name}`;
    const blockers = pr.name === 'weather-bot' ? ['quitClaude'] : [];
    const notes = pr.name === 'stock-notes' ? ['venv'] : [];
    return { from: m.from, to, claude: pr.claude, claude_app: pr.claude_app, codex: pr.codex, blockers, notes, errors: [] };
  };
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const handlers = {
    info: () => ({ version: '0.1.0', platform: 'macos', home, desktop: `${home}/Desktop`, settings, otherRings: [{ name: 'Filee', keys: ['command'] }, { name: 'Peekless', keys: ['option'] }] }),
    set_settings: ({ settings: s }) => (settings = s),
    scan: async () => (await wait(300), { desktop_ok: true, projects, running: { claude_app: true, codex: false } }),
    preview: async ({ moves }) => (await wait(300), moves.map(plan)),
    relocate: async ({ moves }) => {
      await wait(900);
      const done = moves.map(plan);
      done.forEach((d) => history.unshift({ time: now + history.length, plan: d, undone: false }));
      return done;
    },
    history: () => history,
    undo: ({ time }) => {
      const r = history.find((h) => h.time === time);
      if (r) r.undone = true;
      return { ...r.plan, from: r.plan.to, to: r.plan.from, blockers: [] };
    },
    pick_folder: () => `${home}/Volumes/Backup/Old projects`,
    ring_drop: async ({ action, paths }) => (await wait(600), paths.map((from) => plan({ from, place: Number(action) || 0 }))),
    ring_hide: () => {},
    reveal: () => {},
  };
  window.__TAURI__ = {
    core: { invoke: async (cmd, args) => handlers[cmd](args || {}) },
    webview: { getCurrentWebview: () => ({ onDragDropEvent: () => Promise.resolve(() => {}) }) },
    event: { listen: () => Promise.resolve(() => {}) },
  };
})();
