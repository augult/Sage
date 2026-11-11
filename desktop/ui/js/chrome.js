// The window's caption buttons. The app draws its own title bar (the top strip of each pane,
// marked `data-tauri-drag-region`), so minimise, maximise and close are ordinary buttons here.

export function mountWindowControls() {
  const current = window.__TAURI__.window.getCurrentWindow();
  const maximize = document.getElementById('win-max');

  document.getElementById('win-min').addEventListener('click', () => current.minimize());
  maximize.addEventListener('click', () => current.toggleMaximize());
  document.getElementById('win-close').addEventListener('click', () => current.close());

  // The middle button shows what it will do: maximise, or restore once the window is maximised.
  async function sync() {
    const maximized = await current.isMaximized();
    const label = maximized ? 'Restore' : 'Maximize';
    maximize.dataset.maximized = String(maximized);
    maximize.title = label;
    maximize.setAttribute('aria-label', label);
  }
  window.addEventListener('resize', sync);
  sync();
}
