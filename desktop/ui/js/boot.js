// Runs before the first paint, as a classic script: puts the chosen appearance and background on
// <html>, which panels show the background, which of the two side panels are put away, and the
// view the window opens with, so the window never opens in the wrong one. js/themes.js owns the
// appearance and the backgrounds from then on, js/panes.js the panels, js/main.js the view.
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
  root.dataset.theme = read('background') || (less('transparency') ? 'none' : 'orchid');
  root.dataset.dither = read('dither') === 'on' ? 'on' : 'off';
  root.dataset.motion = (read('motion') || (less('motion') ? 'off' : 'on')) === 'off' ? 'off' : 'on';
  // The background shows in all three panels until Settings takes it out of one.
  for (const panel of ['Left', 'Middle', 'Right']) root.dataset[`bg${panel}`] = read(`bg-${panel.toLowerCase()}`) === 'off' ? 'off' : 'on';
  // The sidebar is there until it is put away; the inspector is away until it is asked for.
  root.dataset.sidebar = read('sidebar') === 'off' ? 'off' : 'on';
  root.dataset.inspector = read('inspector') === 'on' ? 'on' : 'off';
  root.dataset.view = 'new';
})();
