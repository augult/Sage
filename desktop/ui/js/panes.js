// The two panes beside the sheet, each put away and brought back by a button of its own. A pane is
// there or it is not: nothing slides. Which are away is kept by the window (js/boot.js reads it
// before the first paint), and is all the stylesheets see: data-sidebar and data-inspector on
// <html>, each `on` or `off`.

import { icon } from './dom.js';
import * as prefs from './prefs.js';

const PANES = [
  { key: 'sidebar', glyph: 'paneLeft' },
  { key: 'inspector', glyph: 'paneRight' },
];

export function mountPaneToggles() {
  const root = document.documentElement;
  for (const { key, glyph } of PANES) {
    const button = document.getElementById(`toggle-${key}`);
    button.append(icon(glyph));
    // The button says what it will do.
    const sync = () => {
      const shown = root.dataset[key] !== 'off';
      const label = `${shown ? 'Hide' : 'Show'} ${key}`;
      button.title = label;
      button.setAttribute('aria-label', label);
      button.setAttribute('aria-expanded', String(shown));
    };
    button.addEventListener('click', () => {
      const next = root.dataset[key] === 'off' ? 'on' : 'off';
      root.dataset[key] = next;
      prefs.set(key, next);
      sync();
    });
    sync();
  }
}
