// Appearance, background, dither and motion: what the user chose, and what that comes to right now.
// The outcome goes on <html> as data-scheme (light or dark), data-theme (the background's id),
// data-bg-left, data-bg-middle and data-bg-right (whether that panel shows the background),
// data-dither and data-motion (on or off), which is all the stylesheets read; js/backdrop.js paints
// the background.
//
// What is chosen in Settings holds, whatever the system says. A system that asks for less
// transparency or less motion decides only what a setting nobody has touched starts as: no
// background, no motion. (Windows asks for the first wherever "Transparency effects" is off.)
//
// A background is a painting made by a script in desktop/art (themes/<id>/art.webp, with a small
// thumb.webp beside it), and the way its dither drifts: three waves as [x, y, speed] in cycles per
// cell and per second, how far they are bent (in cells), the part of the field that carries dots,
// and how densely at most.

import * as prefs from './prefs.js';

const painting = (id, name, dither) => ({ id, name, art: `themes/${id}/art.webp`, thumb: `themes/${id}/thumb.webp`, dither });

export const BACKGROUNDS = [
  painting('ember', 'Ember', { waves: [[0.0105, -0.0062, 0.046], [0.0041, 0.0113, 0.031], [-0.0063, -0.0034, 0.021]], warp: 10, reach: [0.16, 0.78], fill: 0.5 }),
  painting('orchid', 'Orchid', { waves: [[0.0062, 0.0041, 0.03], [-0.0043, 0.0071, 0.022], [0.0031, -0.0023, 0.015]], warp: 16, reach: [0.2, 0.8], fill: 0.46 }),
  painting('tide', 'Tide', { waves: [[0.0031, 0.0163, 0.04], [0.0052, -0.0112, 0.03], [0.0113, 0.0021, 0.05]], warp: 8, reach: [0.18, 0.8], fill: 0.5 }),
  painting('graphite', 'Graphite', { waves: [[0.0121, 0.0032, 0.02], [-0.0083, 0.0061, 0.035], [0.0022, 0.0093, -0.045]], warp: 12, reach: [0.2, 0.82], fill: 0.44 }),
  { id: 'none', name: 'None', art: null, thumb: null, dither: null },
];
export const APPEARANCES = ['system', 'light', 'dark'];

// The three panels of the window, and the setting that says whether each shows the background.
export const PANELS = [
  { id: 'left', key: 'bg-left', name: 'Left' },
  { id: 'middle', key: 'bg-middle', name: 'Middle' },
  { id: 'right', key: 'bg-right', name: 'Right' },
];

// Out of the box the background shows in the middle panel alone.
const DEFAULTS = { appearance: 'system', background: 'ember', dither: 'on', motion: 'on', 'bg-left': 'off', 'bg-middle': 'on', 'bg-right': 'off' };
const SWITCH = ['on', 'off'];
const ALLOWED = { appearance: APPEARANCES, background: BACKGROUNDS.map((background) => background.id), dither: SWITCH, motion: SWITCH, 'bg-left': SWITCH, 'bg-middle': SWITCH, 'bg-right': SWITCH };
const QUIET_DITHER = { waves: [[0.008, 0.004, 0.03], [-0.005, 0.009, 0.022], [0.003, -0.006, 0.017]], warp: 10, reach: [0.2, 0.8], fill: 0.4 };
// The dots over the pressed backdrop: faint, and of both kinds, so they show on any picture.
const INKS = {
  dark: { light: [255, 255, 255, 0.17], dark: [0, 0, 0, 0.3] },
  light: { light: [255, 255, 255, 0.6], dark: [17, 18, 20, 0.085] },
};

const prefersDark = matchMedia('(prefers-color-scheme: dark)');
const lessMotion = matchMedia('(prefers-reduced-motion: reduce)');
const lessGlass = matchMedia('(prefers-reduced-transparency: reduce)');
const listeners = new Set();
// Set by the test hooks, for this launch only.
const forced = {};

/** What `key` has been set to, by the test hooks or in Settings; null while it has been left alone. */
function picked(key) {
  const value = forced[key] ?? prefs.get(key);
  return ALLOWED[key].includes(value) ? value : null;
}

/**
 * The settings as they stand, which is what Settings shows: `{ appearance, background, dither, motion }`
 * and, under each panel's key, whether that panel shows the background.
 * Each is what was chosen, or else what it starts as on this system.
 */
export function chosen() {
  const start = { ...DEFAULTS, ...(lessGlass.matches && { background: 'none' }), ...(lessMotion.matches && { motion: 'off' }) };
  const pick = {};
  for (const key of Object.keys(DEFAULTS)) pick[key] = picked(key) ?? start[key];
  return pick;
}

/** What the settings come to now: `{ scheme, background, dither, motion }`. */
export function current() {
  const pick = chosen();
  return {
    scheme: pick.appearance === 'system' ? (prefersDark.matches ? 'dark' : 'light') : pick.appearance,
    background: BACKGROUNDS.find((background) => background.id === pick.background),
    // Without it the painting is all there is behind the panes.
    dither: pick.dither === 'on',
    motion: pick.motion === 'on',
  };
}

/** The dither for a look, as js/dither.js takes it. */
export function ditherLook(now) {
  return { cell: 3, ...(now.background.dither ?? QUIET_DITHER), ...INKS[now.scheme] };
}

function apply() {
  const now = current();
  document.documentElement.dataset.scheme = now.scheme;
  document.documentElement.dataset.theme = now.background.id;
  document.documentElement.dataset.dither = now.dither ? 'on' : 'off';
  document.documentElement.dataset.motion = now.motion ? 'on' : 'off';
  const pick = chosen();
  for (const panel of PANELS) document.documentElement.setAttribute(`data-bg-${panel.id}`, pick[panel.key]);
  for (const listener of listeners) listener(now);
}

/** Keeps a choice: `{ appearance }`, `{ background }`, `{ dither }`, `{ motion }` or a panel's, as `{ 'bg-left': 'on' }`. */
export function choose(change) {
  for (const [key, value] of Object.entries(change)) {
    if (ALLOWED[key]?.includes(value)) prefs.set(key, value);
  }
  apply();
}

/** Overrides choices until the window closes, without keeping them. */
export function force(change) {
  for (const [key, value] of Object.entries(change)) {
    if (ALLOWED[key]?.includes(value)) forced[key] = value;
  }
  apply();
}

/** `listener(now)` runs whenever the outcome may have changed. Returns the unsubscribe function. */
export function onChange(listener) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

for (const query of [prefersDark, lessMotion, lessGlass]) query.addEventListener('change', apply);
apply();
