// Runs before the first paint, as a classic script: puts the chosen appearance and background on
// <html>, and which of the two panes are put away, so the window never opens in the wrong one.
// js/themes.js owns the first two from then on, js/panes.js the panes.
(() => {
  const read = (key) => {
    try {
      return localStorage.getItem(`presage.${key}`);
    } catch {
      return null;
    }
  };
  const root = document.documentElement;
  const appearance = read('appearance');
  const dark = appearance === 'dark' || (appearance !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);
  root.dataset.scheme = dark ? 'dark' : 'light';
  root.dataset.theme = matchMedia('(prefers-reduced-transparency: reduce)').matches ? 'none' : read('background') || 'ember';
  root.dataset.dither = read('dither') === 'off' ? 'off' : 'on';
  for (const pane of ['sidebar', 'inspector']) root.dataset[pane] = read(pane) === 'off' ? 'off' : 'on';
})();
