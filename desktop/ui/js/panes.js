// The two panels beside the sheet, each put away and brought back by a button of its own: the
// sidebar's in the title bar, the inspector's in the sheet's corner. A panel is there or it is
// not: nothing slides. The sidebar is there until it is put away; the inspector is away until it
// is asked for. Which are away is kept by the window (js/boot.js reads it before the first paint),
// and is all the stylesheets see: data-sidebar and data-inspector on <html>, each `on` or `off`.

import { icon } from './dom.js';
import * as prefs from './prefs.js';

const PANES = { sidebar: 'paneLeft', inspector: 'paneRight' };
const root = document.documentElement;

export const shown = (key) => root.dataset[key] !== 'off';

/** Brings the panel `key` back, or puts it away. */
export function set(key, on) {
  root.dataset[key] = on ? 'on' : 'off';
  prefs.set(key, root.dataset[key]);
  sync(key);
}

export const toggle = (key) => set(key, !shown(key));

/** The button says what it will do. */
function sync(key) {
  const button = document.getElementById(`toggle-${key}`);
  const label = `${shown(key) ? 'Hide' : 'Show'} ${key}`;
  button.dataset.tooltip = label;
  button.setAttribute('aria-label', label);
  button.setAttribute('aria-expanded', String(shown(key)));
}

export function mountPaneToggles() {
  for (const [key, glyph] of Object.entries(PANES)) {
    const button = document.getElementById(`toggle-${key}`);
    button.append(icon(glyph));
    button.addEventListener('click', () => toggle(key));
    sync(key);
  }
}
