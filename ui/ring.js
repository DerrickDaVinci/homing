// The homing ring: opens under the cursor while you drag folders with the
// trigger key held. Drop on a segment to send them there; drop in the middle
// to cancel. (Geometry shared with Peekless's action ring.)
(() => {
  const T = window.__TAURI__;
  const invoke = (c, a) => T.core.invoke(c, a);
  const svg = document.getElementById('ring');
  const C = 150, R = 138, r = 54;
  const sysZh = (navigator.language || '').toLowerCase().startsWith('zh');

  // Icons are 24x24 strokes drawn around (0,0).
  const FOLDER = '<path d="M-10 -4a2 2 0 0 1 2-2h5l2 2.5h9a2 2 0 0 1 2 2V7a2 2 0 0 1-2 2H-8a2 2 0 0 1-2-2z"/><path d="M-2 3.5l3 -3 3 3M1 0.5v6"/>';
  const EYE = '<path d="M-10 0c3-5 6.5-7 10-7s7 2 10 7c-3 5-6.5 7-10 7s-7-2-10-7z"/><circle cx="0" cy="0" r="3"/>';
  // Segments: the user's places, then Preview. Ids are what ring_drop expects.
  let segs = [];
  const setPlaces = (places) => (segs = [...(places || []).map((p, i) => ({ id: String(i), label: p.label, icon: FOLDER })), { id: 'preview', label: null, icon: EYE }]);
  const ZH = {
    preview: '预览', drop: '拖到一项上', cancel: '松手取消', moved: (n) => `已归位 ${n} 个`, undo: '撤销', undone: '已撤销', failed: '没有移动',
    why: { quitClaude: '先退出 Claude', quitCodex: '先退出 Codex', exists: '目标已有同名', busy: '有程序在用', protected: '系统文件夹', notFolder: '不是文件夹', inside: '不能放进自己', crossDevice: '不同磁盘' },
  };
  const EN = {
    preview: 'Preview', drop: 'Drop on one', cancel: 'Release to cancel', moved: (n) => `${n} moved`, undo: 'Undo', undone: 'Undone', failed: 'Not moved',
    why: { quitClaude: 'Quit Claude first', quitCodex: 'Quit Codex first', exists: 'Name taken', busy: 'Folder in use', protected: 'System folder', notFolder: 'Not a folder', inside: 'Into itself', crossDevice: 'Other disk' },
  };

  let zh = sysZh;
  let L = zh ? ZH : EN;
  let paths = [];
  let hot = -2; // -2 none, -1 hub, 0..n-1 segment
  let busy = false;
  let dropped = false;
  let result = null; // { ok, line, sub, moved: [to] }
  let hideTimer;
  // Near a screen edge the ring is shifted, so the cursor may start on top of
  // a segment. Segments only arm once the cursor has clearly moved.
  let start = null;
  let armed = false;
  const ARM_PX = 12;

  const pt = (deg, rad) => [C + rad * Math.cos((deg * Math.PI) / 180), C + rad * Math.sin((deg * Math.PI) / 180)];
  const mid = (i, n) => -90 + (i * 360) / n;
  function arc(i, n) {
    const span = 360 / n, gap = 2.2;
    const a0 = mid(i, n) - span / 2 + gap, a1 = mid(i, n) + span / 2 - gap;
    const big = a1 - a0 > 180 ? 1 : 0;
    const [x0, y0] = pt(a0, R), [x1, y1] = pt(a1, R), [x2, y2] = pt(a1, r), [x3, y3] = pt(a0, r);
    return `M${x0} ${y0}A${R} ${R} 0 ${big} 1 ${x1} ${y1}L${x2} ${y2}A${r} ${r} 0 ${big} 0 ${x3} ${y3}Z`;
  }
  function segmentAt(x, y) {
    const dx = x - C, dy = y - C, d = Math.hypot(dx, dy);
    if (d < r) return -1;
    if (d > R + 8) return -2;
    const span = 360 / segs.length;
    const deg = ((Math.atan2(dy, dx) * 180) / Math.PI + 90 + span / 2 + 720) % 360;
    return Math.floor(deg / span) % segs.length;
  }

  function hub() {
    const disc = `<circle cx="${C}" cy="${C}" r="${r - 6}"/>`;
    if (busy) return disc + `<circle class="spin" cx="${C}" cy="${C}" r="18" stroke-dasharray="70 200"/>`;
    if (result) {
      const undo = result.moved?.length ? `<text class="undo" id="undo" x="${C}" y="${C + 31}">${L.undo}</text>` : '';
      return `${disc}<text class="big ${result.ok ? 'ok' : 'err'}" x="${C}" y="${C - 8}">${result.ok ? '✓' : '!'}</text>
        <text x="${C}" y="${C + 11}">${result.line}</text>${result.sub ? `<text x="${C}" y="${C + 26}">${result.sub}</text>` : undo}`;
    }
    return disc + `<text x="${C}" y="${C + 4}">${hot === -1 ? L.cancel : L.drop}</text>`;
  }

  function render() {
    let s = '';
    const n = segs.length;
    segs.forEach((a, i) => {
      const [lx, ly] = pt(mid(i, n), (R + r) / 2);
      const label = String(a.label ?? L.preview).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
      s += `<g class="rs ${hot === i ? 'hot' : ''}"><path d="${arc(i, n)}"/><g class="ic" transform="translate(${lx} ${ly - 10})">${a.icon}</g><text x="${lx}" y="${ly + 18}">${label}</text></g>`;
    });
    s += `<g class="rh ${hot === -1 ? 'hot' : ''}">${hub()}</g>`;
    if (paths.length > 1 && !busy && !result) {
      const bx = C + 98, by = C - 98;
      s += `<circle class="count" cx="${bx}" cy="${by}" r="13"/><text class="count-t" x="${bx}" y="${by + 4}">${paths.length}</text>`;
    }
    svg.innerHTML = s;
  }

  function which(x, y) {
    if (!start) start = [x, y];
    if (!armed && Math.hypot(x - start[0], y - start[1]) > ARM_PX) armed = true;
    return armed ? segmentAt(x, y) : -1;
  }

  function reset() {
    clearTimeout(hideTimer);
    paths = [];
    hot = -2;
    busy = false;
    dropped = false;
    result = null;
    start = null;
    armed = false;
    render();
    // Restart the pop-in without relying on animation frames (paused while hidden).
    svg.classList.remove('in');
    void svg.getBoundingClientRect();
    svg.classList.add('in');
  }

  const hide = (ms = 0) => {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => invoke('ring_hide'), ms);
  };

  async function run(i) {
    const id = segs[i].id;
    busy = true;
    render();
    const plans = await invoke('ring_drop', { action: id, paths }).catch(() => null);
    busy = false;
    if (!plans || !plans.length) return hide(0); // preview opened the main window
    const ok = plans.filter((p) => !p.blockers.length);
    const bad = plans.filter((p) => p.blockers.length);
    const reason = bad.length ? L.why[bad[0].blockers[0].split(':')[0]] || L.failed : '';
    result = ok.length
      ? { ok: true, line: L.moved(ok.length), sub: bad.length ? `${L.failed} ${bad.length} · ${reason}` : '', moved: ok.map((p) => p.to) }
      : { ok: false, line: L.failed, sub: reason };
    render();
    hide(ok.length ? 5000 : 3000);
  }

  async function undo() {
    const moved = result?.moved || [];
    clearTimeout(hideTimer);
    busy = true;
    render();
    const h = await invoke('history');
    for (const r of h.filter((r) => !r.undone && moved.includes(r.plan.to))) await invoke('undo', { time: r.time });
    busy = false;
    result = { ok: true, line: L.undone, sub: '' };
    render();
    hide(1200);
  }

  svg.addEventListener('click', (e) => {
    if (e.target.id === 'undo') undo();
  });
  // Keep the result up while the pointer rests on it.
  svg.addEventListener('mouseenter', () => result && result.moved && clearTimeout(hideTimer));
  svg.addEventListener('mouseleave', () => result && !busy && hide(1500));

  const toLocal = (p) => [p.x / devicePixelRatio, p.y / devicePixelRatio];

  T.webview.getCurrentWebview().onDragDropEvent((ev) => {
    const p = ev.payload;
    if (busy || result) return;
    if (p.type === 'enter') paths = p.paths || [];
    if (p.type === 'enter' || p.type === 'over') {
      const [x, y] = toLocal(p.position);
      const h = which(x, y);
      if (h !== hot) {
        hot = h;
        render();
      }
    } else if (p.type === 'leave') {
      hot = -2;
      render();
    } else if (p.type === 'drop') {
      dropped = true;
      paths = p.paths || paths;
      const [x, y] = toLocal(p.position);
      const h = which(x, y);
      if (h >= 0) run(h);
      else hide(0);
    }
  });

  function applySettings(s) {
    if (!s) return;
    setPlaces(s.places);
    zh = s.lang === 'zh' ? true : s.lang === 'en' ? false : sysZh;
    L = zh ? ZH : EN;
    if (s.theme === 'light' || s.theme === 'dark') document.documentElement.dataset.theme = s.theme;
    else delete document.documentElement.dataset.theme;
    render();
  }

  // The mouse went up: if it was not over the ring, close.
  T.event.listen('ring-release', () => setTimeout(() => { if (!dropped && !busy) invoke('ring_hide'); }, 300));
  T.event.listen('ring-show', reset);
  T.event.listen('settings-changed', (e) => applySettings(e.payload));
  reset();
  invoke('info').then((i) => applySettings(i.settings)).catch(() => {});
})();
