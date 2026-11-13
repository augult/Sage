// The window's caption buttons. The app draws its own title bar (js/menubar.js has its menus), so
// minimise, maximise and close are ordinary buttons here.

import { settleUnsaved } from './unsaved.js';

/** The window itself: what the caption buttons and the Window menu act on. */
export const appWindow = window.__TAURI__.window.getCurrentWindow();

/** Whether the window is maximised now, as the middle button last found it. */
export const maximized = () => document.getElementById('win-max').dataset.maximized === 'true';

export function mountWindowControls() {
  const maximize = document.getElementById('win-max');

  document.getElementById('win-min').addEventListener('click', () => appWindow.minimize());
  maximize.addEventListener('click', () => appWindow.toggleMaximize());
  document.getElementById('win-close').addEventListener('click', () => appWindow.close());
  // However the window is asked to close (this button, the taskbar, a shortcut), what was typed and
  // not saved is asked about first. The window goes once the handler returns without objecting.
  appWindow.onCloseRequested(async (event) => {
    if (!(await settleUnsaved())) event.preventDefault();
  });

  // The middle button shows what it will do: maximise, or restore once the window is maximised.
  async function sync() {
    const full = await appWindow.isMaximized();
    const label = full ? 'Restore' : 'Maximize';
    maximize.dataset.maximized = String(full);
    maximize.title = label;
    maximize.setAttribute('aria-label', label);
  }
  window.addEventListener('resize', sync);
  sync();
}
