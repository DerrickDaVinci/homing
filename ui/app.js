// Homing main window. Plain JS, no build step: the real work happens in Rust.
(() => {
  const T = window.__TAURI__;
  const invoke = (cmd, args) => T.core.invoke(cmd, args);
  const $ = (s) => document.querySelector(s);
  const $$ = (s) => Array.from(document.querySelectorAll(s));
  const esc = (s) => String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const sysZh = (navigator.language || '').toLowerCase().startsWith('zh');
  let zh = sysZh;

  // ------------------------------------------------------------ copy
  const STR = {
    tagline: ['归位 · 桌面项目回家', 'Projects off your Desktop'],
    navDesk: ['桌面', 'Desktop'], navHistory: ['记录', 'History'], navSettings: ['设置', 'Settings'], navAbout: ['关于', 'About'],
    deskTitle: ['桌面', 'Desktop'], rescan: ['重新扫描', 'Rescan'], scanning: ['正在扫描…', 'Scanning…'],
    deskSub: (n, root) => (zh ? `桌面上有 ${n} 个项目文件夹。归位会把它们搬到 ${root}，Claude 和 Codex 的对话跟着走。` : `${n} project folders on your Desktop. Homing moves them to ${root} and takes their Claude and Codex conversations along.`),
    deskEmpty: ['桌面上没有项目文件夹了。', 'No project folders left on your Desktop.'],
    noAccess: ['Homing 还没有权限读取桌面。请在 系统设置 → 隐私与安全性 → 文件与文件夹 里允许 Homing 访问"桌面"，然后点重新扫描。', 'Homing cannot read your Desktop yet. Allow it in System Settings → Privacy & Security → Files and Folders → Desktop, then Rescan.'],
    toRoot: ['放进项目文件夹', 'To Projects'], toArchive: ['放进归档', 'To Archive'],
    toRootS: (d) => (zh ? `${d} 天内动过` : `Touched in the last ${d} days`), toArchiveS: (d) => (zh ? `超过 ${d} 天没动` : `Untouched for ${d}+ days`),
    moveToArchive: ['改为放进归档', 'Archive instead'], moveToRoot: ['改为放进项目文件夹', 'Keep active instead'], reveal: ['在访达中显示', 'Show in folder'],
    previewBtn: ['预览归位', 'Preview'], selected: (n) => (zh ? `已选 ${n} 个，先预览，确认后才会移动` : `${n} selected. Nothing moves until you confirm.`),
    today: ['今天', 'today'], daysAgo: (n) => (zh ? `${n} 天前` : `${n}d ago`), monthsAgo: (n) => (zh ? `${n} 个月前` : `${n}mo ago`),
    runClaude: ['Claude 桌面版正在运行。', 'The Claude app is running. '], runCodex: ['Codex 正在运行。', 'Codex is running. '],
    runHint: ['带有这些会话的项目要先退出对应应用才能归位，否则应用会把旧路径写回去。其他项目不受影响。', 'Projects with its sessions need it closed first, or it would write the old paths back. Other projects are fine.'],
    sheetPreview: ['预览', 'Preview'], sheetPreviewS: ['下面是将要发生的全部变化。原文件夹只是移动，不会删除任何东西。', 'This is everything that will change. Folders are moved, never deleted.'],
    sheetDone: ['归位完成', 'Done'], sheetDoneS: ['可以在「记录」里随时撤销。', 'You can undo any move from History.'],
    apply: (n) => (zh ? `归位 ${n} 个` : `Move ${n}`), cancel: ['取消', 'Cancel'], done: ['完成', 'Done'],
    working: ['正在移动并更新会话记录，会话多的项目可能要一分钟…', 'Moving and updating session records. Large projects can take a minute…'],
    sessions: (c, x) => [c ? `Claude ${c}` : '', x ? `Codex ${x}` : ''].filter(Boolean).join(' · ') || (zh ? '没有代理会话' : 'No agent sessions'),
    moved: ['已归位', 'Moved'], notMoved: ['没有移动', 'Not moved'],
    toastMoved: (n) => (zh ? `已归位 ${n} 个项目` : `Moved ${n} project${n === 1 ? '' : 's'}`), toastUndone: ['已撤销', 'Undone'],
    toastFailed: ['撤销没有完成：', 'Could not undo: '],
    undo: ['撤销', 'Undo'], undone: ['已撤销', 'Undone'],
    historySub: ['每次归位都记在这里。撤销会把文件夹和会话记录一起放回原处。', 'Every move is listed here. Undo puts the folder and its sessions back.'],
    historyEmpty: ['还没有归位过任何项目。', 'Nothing moved yet.'],
    root: ['项目文件夹', 'Projects folder'], change: ['更改…', 'Change…'],
    archiveDays: ['多久没动算"冷"项目', 'When a project counts as cold'], archiveDaysS: ['冷项目默认放进项目文件夹下的 Archive。', 'Cold projects go to the Archive folder inside it.'],
    days: (n) => (zh ? `${n} 天` : `${n} days`),
    lang: ['界面语言', 'Language'], theme: ['外观', 'Appearance'], followSystem: ['跟随系统', 'System'], light: ['浅色', 'Light'], dark: ['深色', 'Dark'],
    aboutTag: ['把 AI 代理开的项目从桌面搬走，对话记录一条不丢。', 'Move AI-agent projects off your Desktop without losing a single conversation.'],
    aWhat: ['会更新什么', 'What gets updated'],
    aWhatS: ['Claude Code 的会话目录和记录、Claude 桌面版的会话、Codex 的数据库、会话文件和 config.toml。只改写路径，不改对话内容。', 'Claude Code session folders and transcripts, Claude desktop sessions, the Codex database, rollout files and config.toml. Only paths change, never the conversation text.'],
    aSafe: ['安全', 'Safety'],
    aSafeS: ['每次都先预览；正在使用的文件夹不会动；改动前备份索引文件；每次移动都能撤销。', 'Always previewed first; busy folders are left alone; index files are backed up; every move can be undone.'],
    aOffline: ['离线', 'Offline'], aOfflineS: ['不联网，没有统计，没有账号。', 'No network, no telemetry, no account.'],
    aCli: ['命令行', 'Command line'],
    ringTip: (k) => (zh ? `小技巧：在桌面或访达里按住 <kbd>${k}</kbd> 拖动项目文件夹，光标旁会弹出归位环，拖到「项目」或「归档」松手就搬好了。` : `Tip: hold <kbd>${k}</kbd> while dragging a project folder on the Desktop or in Finder. A ring opens at the cursor: release on Projects or Archive and it is moved.`),
    ringOn: ['按住按键拖动时弹出归位环', 'Homing ring while dragging'],
    ringOnS: (k) => (zh ? `在桌面、访达或任何地方拖动文件夹时按住 ${k}。不需要辅助功能权限。Homing 需要在菜单栏运行。` : `Hold ${k} while dragging folders anywhere. No Accessibility permission needed. Homing must be running in the menu bar.`),
    ringKey: ['触发键', 'Trigger keys'], ringKeyS: ['点右边的按钮，按住想用的键（可以组合，比如 ⌃⇧），松开即保存。只有正好按着这组键才会弹出归位环。', 'Click the button, hold the keys you want (combinations work, like ⌃⇧) and release. The ring opens only for exactly these keys.'],
    recording: ['按住想用的键…', 'Hold the keys…'], onlyMods: ['只能用 ⌃ ⌥ ⇧ ⌘ 这几个键。', 'Only ⌃ ⌥ ⇧ ⌘ can be used.'],
    clash: (who, k) => (zh ? `${who} 也用 ${k} 拖动，两个环会一起弹出。换一个不包含 ${k} 的组合吧。` : `${who} also uses ${k} while dragging, so both would open. Pick keys without ${k}.`),
    used: (list) => (zh ? `这台电脑上已被占用：${list}` : `Already used here: ${list}`),
    dragSuffix: [' + 拖动', ' + drag'],
    ringAfter: ['松手之后', 'After you drop'], ringAfterS: ['"直接归位"会立刻移动，环中间保留撤销；"先预览"会打开主窗口让你确认。', '"Move right away" moves at once and keeps Undo in the ring; "Preview first" opens the window to confirm.'],
    ringDirect: ['直接归位', 'Move right away'], ringAsk: ['先预览', 'Preview first'],
    login: ['开机自动启动', 'Open at login'], loginS: ['登录后在菜单栏待命，归位环随时可用。', 'Waits in the menu bar after you log in, ring ready.'],
    hidden: ['启动时不显示窗口', 'Start without a window'], hiddenS: ['只在菜单栏待命，需要时点图标打开。', 'Stay in the menu bar until you open it.'],
    dropNow: ['松手预览归位', 'Drop to preview'], dropNowS: ['任何文件夹都可以', 'Any folder works'],
    b_missing: ['文件夹已经不在了。', 'The folder is gone.'], b_notFolder: ['这不是文件夹。', 'Not a folder.'],
    b_protected: ['这是系统文件夹，不能移动。', 'A system folder; it stays.'],
    b_exists: ['目标位置已经有同名文件夹。', 'A folder with this name already exists there.'],
    b_inside: ['不能放进它自己里面。', 'Cannot move a folder into itself.'],
    b_busy: (n) => (zh ? `有程序正在这个文件夹里运行：${n}。先关掉它们。` : `Programs are running inside it: ${n}. Close them first.`),
    b_quitClaude: ['先退出 Claude 桌面版，它运行时会把旧路径写回去。', 'Quit the Claude app first, or it writes the old path back.'],
    b_quitCodex: ['先退出 Codex（ChatGPT 桌面版或 codex 命令行）。', 'Quit Codex first (the desktop app or the codex CLI).'],
    b_crossDevice: ['目标在另一个磁盘上，暂不支持。', 'The destination is on another disk; not supported yet.'],
    n_venv: ['里面有 Python 虚拟环境，移动后需要重新创建（.venv 记着旧路径）。', 'Contains a Python virtualenv; recreate it after the move (it stores the old path).'],
    n_worktrees: ['用到了 git worktree，移动后会自动执行 git worktree repair。', 'Uses git worktrees; git worktree repair runs after the move.'],
    e_gitRepair: ['git worktree repair 没有成功，请在项目里手动运行一次。', 'git worktree repair failed; run it in the project.'],
  };
  const t = (k, ...a) => {
    const v = STR[k];
    if (typeof v === 'function') return v(...a);
    return v ? v[zh ? 0 : 1] : k;
  };

  // ------------------------------------------------------------ state
  const S = { info: null, scan: null, archive: new Map(), off: new Set(), lastMoved: [] };
  const settings = () => S.info.settings;
  const tilde = (p) => (p && S.info && p.startsWith(S.info.home) ? '~' + p.slice(S.info.home.length) : p);
  const base = (p) => p.split(/[\\/]/).filter(Boolean).pop() || p;
  const ago = (secs) => {
    const d = Math.floor((Date.now() / 1000 - secs) / 86400);
    return d < 1 ? t('today') : d < 60 ? t('daysAgo', d) : t('monthsAgo', Math.floor(d / 30));
  };
  const cold = (p) => Date.now() / 1000 - p.active > settings().archiveDays * 86400;
  const isArchive = (p) => (S.archive.has(p.path) ? S.archive.get(p.path) : cold(p));
  const KEY = { control: '⌃', option: '⌥', shift: '⇧', command: '⌘' };
  const ORDER = ['control', 'option', 'shift', 'command'];
  const keyLabel = (keys) => ORDER.filter((k) => keys.includes(k)).map((k) => KEY[k]).join('');
  const ringKey = () => keyLabel(settings().ringKeys || ['control']) || '⌃';

  const ICON = {
    swap: '<svg viewBox="0 0 24 24"><path d="M7 7h11l-3-3M17 17H6l3 3"/></svg>',
    reveal: '<svg viewBox="0 0 24 24"><path d="M3.5 7.5a2 2 0 0 1 2-2H10l2 2.5h6.5a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z"/></svg>',
  };

  // ------------------------------------------------------------ i18n + theme
  function applyLang() {
    const l = settings().lang;
    zh = l === 'zh' ? true : l === 'en' ? false : sysZh;
    document.documentElement.lang = zh ? 'zh-CN' : 'en';
    $$('[data-i18n]').forEach((el) => (el.textContent = t(el.dataset.i18n)));
  }
  function applyTheme() {
    const th = settings().theme;
    if (th === 'light' || th === 'dark') document.documentElement.dataset.theme = th;
    else delete document.documentElement.dataset.theme;
  }
  async function save() {
    await invoke('set_settings', { settings: settings() });
  }

  // ------------------------------------------------------------ desktop
  async function rescan() {
    $('#deskSub').textContent = t('scanning');
    $('#bar').hidden = true;
    S.scan = await invoke('scan');
    renderDesk();
  }

  function renderDesk() {
    if (!S.scan) return;
    const ps = S.scan.projects;
    const d = settings().archiveDays;
    $('#ringTip').hidden = !settings().ring;
    $('#ringTip').innerHTML = t('ringTip', ringKey());
    $('#deskSub').textContent = !S.scan.desktop_ok ? t('noAccess') : ps.length ? t('deskSub', ps.length, tilde(settings().root)) : t('deskEmpty');

    const run = S.scan.running;
    const hit = (k) => ps.some((p) => (k === 'claude' ? p.claude_app : p.codex) > 0);
    const parts = [run.claude_app && hit('claude') ? t('runClaude') : '', run.codex && hit('codex') ? t('runCodex') : ''].filter(Boolean);
    $('#runningBanner').hidden = !parts.length;
    $('#runningBanner').textContent = parts.join('') + t('runHint');

    const groups = [
      { archive: false, title: t('toRoot'), sub: t('toRootS', d), items: ps.filter((p) => !isArchive(p)) },
      { archive: true, title: t('toArchive'), sub: t('toArchiveS', d), items: ps.filter(isArchive) },
    ];
    $('#groups').innerHTML = groups
      .filter((g) => g.items.length)
      .map((g) => `<div class="group-title"><b>${esc(g.title)}</b><span>${esc(g.sub)}</span></div>
        <ul class="projects">${g.items.map((p) => row(p, g.archive)).join('')}</ul>`)
      .join('');
    const n = ps.filter((p) => !S.off.has(p.path)).length;
    $('#bar').hidden = !ps.length;
    $('#barText').textContent = t('selected', n);
    $('#previewBtn').disabled = !n;
  }

  function row(p, archive) {
    const on = !S.off.has(p.path);
    const claude = Math.max(p.claude, p.claude_app);
    const chips = [
      `<span>${esc(ago(p.active))}</span>`,
      claude ? `<span class="chip claude"><i></i>Claude ${claude}</span>` : '',
      p.codex ? `<span class="chip codex"><i></i>Codex ${p.codex}</span>` : '',
      p.git ? '<span>git</span>' : '',
    ].join('');
    return `<li data-path="${esc(p.path)}" class="${on ? '' : 'off'}">
      <input type="checkbox" ${on ? 'checked' : ''} tabindex="-1">
      <div><div class="nm">${esc(p.name)}</div><div class="meta">${chips}</div></div>
      <div class="acts">
        <button type="button" class="icon-btn" data-act="swap" title="${esc(t(archive ? 'moveToRoot' : 'moveToArchive'))}">${ICON.swap}</button>
        <button type="button" class="icon-btn" data-act="reveal" title="${esc(t('reveal'))}">${ICON.reveal}</button>
      </div></li>`;
  }

  $('#groups').addEventListener('click', (e) => {
    const li = e.target.closest('li[data-path]');
    if (!li) return;
    const path = li.dataset.path;
    const act = e.target.closest('[data-act]')?.dataset.act;
    if (act === 'reveal') return invoke('reveal', { path });
    if (act === 'swap') {
      const p = S.scan.projects.find((x) => x.path === path);
      S.archive.set(path, !isArchive(p));
    } else if (S.off.has(path)) S.off.delete(path);
    else S.off.add(path);
    renderDesk();
  });

  $('#rescan').addEventListener('click', rescan);
  $('#previewBtn').addEventListener('click', () => {
    const moves = S.scan.projects.filter((p) => !S.off.has(p.path)).map((p) => ({ from: p.path, archive: isArchive(p) }));
    openPreview(moves);
  });

  // ------------------------------------------------------------ preview sheet
  let pending = [];
  const why = (codes, kind) =>
    codes
      .map((c) => {
        const [k, v] = c.split(/:(.*)/s);
        const key = { block: 'b_', warn: 'n_', err: 'e_' }[kind] + k;
        const text = STR[key] ? t(key, v) : c;
        return `<p class="why ${kind === 'warn' ? 'warn' : 'block'}">${esc(text)}</p>`;
      })
      .join('');

  function planRow(p, result) {
    const blocked = p.blockers.length > 0;
    const status = result ? `<b>${esc(t(blocked ? 'notMoved' : 'moved'))} · </b>` : '';
    return `<li class="${blocked ? 'blocked' : ''}">
      <div class="nm">${status}${esc(base(p.from))}</div>
      <div class="route">${esc(tilde(p.from))} → ${esc(tilde(p.to))}</div>
      <div class="meta">${esc(t('sessions', Math.max(p.claude, p.claude_app), p.codex))}</div>
      ${why(p.blockers, 'block')}${why(p.notes, 'warn')}${why(p.errors || [], 'err')}</li>`;
  }

  async function openPreview(moves) {
    if (!moves.length) return;
    $('#sheet').hidden = false;
    $('#sheetTitle').textContent = t('sheetPreview');
    $('#sheetSub').textContent = t('scanning');
    $('#plans').innerHTML = '';
    $('#applyBtn').hidden = true;
    $('#cancelBtn').textContent = t('cancel');
    const plans = await invoke('preview', { moves });
    pending = moves.filter((m, i) => !plans[i].blockers.length);
    $('#sheetSub').textContent = t('sheetPreviewS');
    $('#plans').innerHTML = plans.map((p) => planRow(p, false)).join('');
    $('#applyBtn').hidden = false;
    $('#applyBtn').textContent = t('apply', pending.length);
    $('#applyBtn').disabled = !pending.length;
  }

  $('#cancelBtn').addEventListener('click', () => ($('#sheet').hidden = true));
  $('#applyBtn').addEventListener('click', async () => {
    $('#applyBtn').disabled = true;
    $('#cancelBtn').disabled = true;
    $('#busy').hidden = false;
    let results = [];
    try {
      results = await invoke('relocate', { moves: pending });
    } finally {
      $('#busy').hidden = true;
      $('#cancelBtn').disabled = false;
    }
    const ok = results.filter((r) => !r.blockers.length);
    S.lastMoved = ok.map((r) => r.to);
    $('#sheetTitle').textContent = t('sheetDone');
    $('#sheetSub').textContent = t('sheetDoneS');
    $('#plans').innerHTML = results.map((p) => planRow(p, true)).join('');
    $('#applyBtn').hidden = true;
    $('#cancelBtn').textContent = t('done');
    if (ok.length) toast(t('toastMoved', ok.length), undoLast);
    S.off.clear();
    rescan();
  });

  // ------------------------------------------------------------ undo + history
  async function undoLast() {
    const h = await invoke('history');
    const recs = h.filter((r) => !r.undone && S.lastMoved.includes(r.plan.to));
    const fails = [];
    for (const r of recs) {
      const back = await invoke('undo', { time: r.time });
      if (back.blockers.length) fails.push(`${base(r.plan.to)}: ${back.blockers.join(', ')}`);
    }
    S.lastMoved = [];
    $('#sheet').hidden = true;
    toast(fails.length ? t('toastFailed') + fails.join('; ') : t('toastUndone'));
    rescan();
    renderHistory();
  }

  async function renderHistory() {
    const h = await invoke('history');
    $('#history').innerHTML = h.length
      ? h
          .map((r) => {
            const p = r.plan;
            const when = new Date(r.time * 1000).toLocaleString(zh ? 'zh-CN' : 'en-US', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
            const act = r.undone
              ? `<span class="when">${esc(t('undone'))}</span>`
              : `<button type="button" class="btn" data-undo="${r.time}">${esc(t('undo'))}</button>`;
            return `<li class="${r.undone ? 'undone' : ''}">
              <div><div class="nm">${esc(base(p.to))}</div>
                <div class="route">${esc(tilde(p.from))} → ${esc(tilde(p.to))}</div>
                <div class="meta"><span>${esc(when)}</span><span>${esc(t('sessions', Math.max(p.claude, p.claude_app), p.codex))}</span></div>
                ${why(p.errors || [], 'err')}</div>
              <div class="acts"><button type="button" class="icon-btn" data-reveal="${esc(r.undone ? p.from : p.to)}" title="${esc(t('reveal'))}">${ICON.reveal}</button>${act}</div></li>`;
          })
          .join('')
      : `<li><span class="empty">${esc(t('historyEmpty'))}</span></li>`;
  }

  $('#history').addEventListener('click', async (e) => {
    const r = e.target.closest('[data-reveal]');
    if (r) return invoke('reveal', { path: r.dataset.reveal });
    const u = e.target.closest('[data-undo]');
    if (!u) return;
    u.disabled = true;
    const back = await invoke('undo', { time: Number(u.dataset.undo) });
    toast(back.blockers.length ? t('toastFailed') + why(back.blockers, 'block').replace(/<[^>]+>/g, ' ') : t('toastUndone'));
    renderHistory();
    rescan();
  });

  // ------------------------------------------------------------ settings
  function renderSettings() {
    const s = settings();
    $('#rootPath').textContent = tilde(s.root);
    $('#archiveDays').value = s.archiveDays;
    $('#archiveDaysOut').textContent = t('days', s.archiveDays);
    $$('.seg[data-set]').forEach((seg) => seg.querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === String(s[seg.dataset.set])))));
    $$('input[data-set]').forEach((i) => (i.checked = !!s[i.dataset.set]));
    $$('[data-needs]').forEach((el) => el.classList.toggle('off', !s[el.dataset.needs]));
    $('#ringOnS').textContent = t('ringOnS', ringKey());
    if (!recording) $('#recorder').textContent = ringKey() + t('dragSuffix');
    // Another tool opens too when all of its keys are among ours.
    const others = S.info.otherRings || [];
    const mine = s.ringKeys || [];
    const clash = others.find((o) => o.keys.every((k) => mine.includes(k)));
    $('#keyWarn').hidden = !clash;
    if (clash) $('#keyWarn').textContent = t('clash', clash.name === 'Peekless' ? (zh ? '遮遮' : 'Peekless') : clash.name, keyLabel(clash.keys));
    $('#keysUsed').hidden = !others.length;
    $('#keysUsed').textContent = t('used', others.map((o) => `${o.name === 'Peekless' && zh ? '遮遮' : o.name} ${keyLabel(o.keys)}`).join(' · '));
  }

  // Record the trigger: hold modifier keys, release to save. Esc cancels.
  let recording = null;
  const MODS = { Control: 'control', Alt: 'option', Shift: 'shift', Meta: 'command' };
  $('#recorder').addEventListener('click', () => {
    recording = new Set();
    $('#recorder').classList.add('rec');
    $('#recorder').textContent = t('recording');
  });
  function stopRecording() {
    recording = null;
    $('#recorder').classList.remove('rec');
    renderSettings();
  }
  window.addEventListener('keydown', (e) => {
    if (!recording) return;
    e.preventDefault();
    if (e.key === 'Escape') return stopRecording();
    if (MODS[e.key]) {
      recording.add(MODS[e.key]);
      $('#recorder').textContent = keyLabel([...recording]) + t('dragSuffix');
    } else {
      $('#recorder').textContent = t('onlyMods');
    }
  });
  window.addEventListener('keyup', async (e) => {
    if (!recording || !MODS[e.key]) return;
    const held = e.ctrlKey || e.altKey || e.shiftKey || e.metaKey;
    if (held || !recording.size) return; // wait until every key is released
    settings().ringKeys = [...recording];
    stopRecording();
    await save();
    renderAll();
  });

  $$('input[data-set]').forEach((i) =>
    i.addEventListener('change', async () => {
      settings()[i.dataset.set] = i.checked;
      await save();
      renderAll();
    }),
  );

  $('#pickRoot').addEventListener('click', async () => {
    const dir = await invoke('pick_root');
    if (!dir) return;
    settings().root = dir;
    await save();
    renderSettings();
    rescan();
  });
  $('#archiveDays').addEventListener('input', (e) => {
    settings().archiveDays = Number(e.target.value);
    $('#archiveDaysOut').textContent = t('days', settings().archiveDays);
    renderDesk();
  });
  $('#archiveDays').addEventListener('change', save);
  $$('.seg[data-set]').forEach((seg) =>
    seg.addEventListener('click', async (e) => {
      const b = e.target.closest('button');
      if (!b) return;
      const v = b.dataset.v;
      settings()[seg.dataset.set] = v === 'true' ? true : v === 'false' ? false : v;
      await save();
      applyLang();
      applyTheme();
      renderAll();
    }),
  );

  // ------------------------------------------------------------ shell
  function go(page) {
    $('#app').dataset.page = page;
    $$('#nav button').forEach((b) => b.setAttribute('aria-current', b.dataset.page === page ? 'page' : 'false'));
    if (page === 'history') renderHistory();
  }
  $('#nav').addEventListener('click', (e) => {
    const b = e.target.closest('button[data-page]');
    if (b) go(b.dataset.page);
  });

  let toastTimer;
  function toast(text, onUndo) {
    $('#toastText').textContent = text;
    $('#toastUndo').hidden = !onUndo;
    $('#toastUndo').onclick = onUndo ? () => ($('#toast').classList.remove('show'), onUndo()) : null;
    $('#toast').classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => $('#toast').classList.remove('show'), onUndo ? 12000 : 4000);
  }

  function renderAll() {
    renderDesk();
    renderSettings();
    if ($('#app').dataset.page === 'history') renderHistory();
  }

  // Folders dropped on the window go straight to a preview.
  T.webview.getCurrentWebview().onDragDropEvent((ev) => {
    const type = ev.payload.type;
    if (type === 'enter' || type === 'over') $('#drop').hidden = false;
    else $('#drop').hidden = true;
    if (type === 'drop' && ev.payload.paths?.length && $('#sheet').hidden) {
      openPreview(ev.payload.paths.map((from) => ({ from, archive: false })));
    }
  });

  // The ring and the menu bar act while this window may be hidden.
  T.event.listen('open-preview', (e) => openPreview(e.payload));
  T.event.listen('moved', () => {
    rescan();
    if ($('#app').dataset.page === 'history') renderHistory();
  });
  T.event.listen('undone', (e) => toast(e.payload.blockers.length ? t('toastFailed') + e.payload.blockers.join(', ') : t('toastUndone')));

  (async () => {
    S.info = await invoke('info');
    if (S.info.platform === 'windows') document.documentElement.classList.add('os-win');
    $('#ver').textContent = 'v' + S.info.version;
    $('#aboutVer').textContent = 'v' + S.info.version;
    if (S.info.platform === 'windows') STR.reveal = ['在资源管理器中显示', 'Show in Explorer'];
    applyLang();
    applyTheme();
    go('desk');
    renderSettings();
    rescan();
  })();
})();
