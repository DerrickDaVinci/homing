// Shared by the main window and the ring: light/dark theme and accent color.
window.Theme = {
  apply(s) {
    const root = document.documentElement;
    if (s.theme === 'light' || s.theme === 'dark') root.dataset.theme = s.theme;
    else delete root.dataset.theme;
    const vars = ['--accent', '--accent-2', '--accent-soft', '--on-accent'];
    if (!/^#[0-9a-f]{6}$/i.test(s.accent || '')) return vars.forEach((v) => root.style.removeProperty(v));
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(s.accent.slice(i, i + 2), 16));
    const light = (0.299 * r + 0.587 * g + 0.114 * b) / 255 > 0.6;
    root.style.setProperty('--accent', s.accent);
    root.style.setProperty('--accent-2', `rgb(${[r, g, b].map((c) => Math.round(c * 0.85)).join(',')})`);
    root.style.setProperty('--accent-soft', `rgba(${r},${g},${b},.12)`);
    root.style.setProperty('--on-accent', light ? '#111214' : '#FFFFFF');
  },
};
