// Runs before the first paint, as a classic script: puts the chosen appearance and background on
// <html>, and which of the two panes are put away, so the window never opens in the wrong one.
// js/themes.js owns the first two from then on, js/panes.js the panes.
(() => {
  const read = (key) => {
    try {
      return localStorage.getItem(`sage.${key}`);
    } catch {
      return null;
    }
  };
  const root = document.documentElement;
  const appearance = read('appearance');
  const dark = appearance === 'dark' || (appearance !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
  root.dataset.scheme = dark ? 'dark' : 'light';
  // What was chosen holds; what nobody has chosen starts as the system asks (js/themes.js).
  const less = (what) => matchMedia(`(prefers-reduced-${what}: reduce)`).matches;
  root.dataset.theme = read('background') || (less('transparency') ? 'none' : 'ember');
  root.dataset.dither = read('dither') === 'off' ? 'off' : 'on';
  root.dataset.motion = (read('motion') || (less('motion') ? 'off' : 'on')) === 'off' ? 'off' : 'on';
  for (const pane of ['sidebar', 'inspector']) root.dataset[pane] = read(pane) === 'off' ? 'off' : 'on';
})();
