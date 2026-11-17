// The glass behind the panels: a painting the app brings itself, never what is behind the window.
//
// The chosen background (themes/<id>/art.webp) is taken once, at thumbnail size, softened, and
// pressed into a band of brightness for the appearance, dark or light, so that text keeps its
// contrast on the thinnest pane whatever the picture is. The result goes to one canvas behind the
// panels. Those that show it are plain translucent fills over it, the rest are solid
// (styles/glass.css): the panels neither blur nor filter while the app runs (the chat bar alone
// does), and nothing looks at the desktop. A second, far smaller canvas carries the dither that
// drifts over the picture (js/dither.js).

import { createDither } from './dither.js';
import * as themes from './themes.js';

const WORK = 320; // long side, in pixels, of the image the softening runs on
const UPSCALE = 4; // the canvas on the page is this many times larger; the GPU stretches the rest
const RADIUS = 3; // box radius at work size; three passes come out close to a Gaussian
const PASSES = 3;
// The darkest and the brightest the backdrop may get, out of 255, and how far colour is pushed.
// Change these, a pane's tint or an ink (styles/base.css, glass.css) and the contrast figures in
// docs/DESKTOP_UI_BRIEF.md have to be computed again.
const BANDS = { dark: [14, 76], light: [198, 248] };
const SATURATION = { dark: 1.35, light: 1.8 };
const GRAIN = { dark: 5, light: 4 };
const SEED = 0x5eed;

const layer = document.getElementById('backdrop');
const canvas = document.getElementById('backdrop-art');
const dither = createDither(document.getElementById('backdrop-dither'), themes.ditherLook(themes.current()));

let shown = ''; // the background and appearance the canvas holds
let asked = 0;

/** A seeded generator (mulberry32): the grain comes out the same on every launch. */
function random(seed) {
  let state = seed;
  return () => {
    state = (state + 0x6d2b79f5) | 0;
    let t = Math.imul(state ^ (state >>> 15), 1 | state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** One box pass along rows or columns. Edges repeat, so the blur does not darken toward the borders. */
function box(from, to, width, height, alongRows) {
  const lines = alongRows ? height : width;
  const length = alongRows ? width : height;
  const step = alongRows ? 3 : width * 3;
  for (let line = 0; line < lines; line++) {
    const base = alongRows ? line * width * 3 : line * 3;
    for (let channel = 0; channel < 3; channel++) {
      const at = (i) => from[base + Math.min(length - 1, Math.max(0, i)) * step + channel];
      let sum = 0;
      for (let i = -RADIUS; i <= RADIUS; i++) sum += at(i);
      for (let i = 0; i < length; i++) {
        to[base + i * step + channel] = sum / (2 * RADIUS + 1);
        sum += at(i + RADIUS + 1) - at(i - RADIUS);
      }
    }
  }
}

function blur(rgb, width, height) {
  const scratch = new Float32Array(rgb.length);
  for (let pass = 0; pass < PASSES; pass++) {
    box(rgb, scratch, width, height, true);
    box(scratch, rgb, width, height, false);
  }
}

/**
 * Presses each pixel's brightness into the band and keeps its hue: the luminosity step of acrylic.
 * The colour is an offset from the grey of that brightness, scaled back until it fits in range, so
 * the pixel comes out exactly as bright as the band says, however saturated it went in.
 */
function press(rgb, [low, high], saturation) {
  for (let i = 0; i < rgb.length; i += 3) {
    const luma = 0.299 * rgb[i] + 0.587 * rgb[i + 1] + 0.114 * rgb[i + 2];
    const target = low + (high - low) * (luma / 255) ** 0.8;
    const gain = target / Math.max(luma, 1);
    let scale = luma < 1 ? 0 : saturation;
    for (let c = i; c < i + 3; c++) {
      const offset = rgb[c] * gain - target;
      if (offset > 0) scale = Math.min(scale, (255 - target) / offset);
      else if (offset < 0) scale = Math.min(scale, target / -offset);
    }
    for (let c = i; c < i + 3; c++) rgb[c] = target + (rgb[c] * gain - target) * scale;
  }
}

/** Draws the backdrop from `picture` for the appearance `scheme`. */
function draw(picture, scheme) {
  const aspect = picture.naturalWidth / Math.max(picture.naturalHeight, 1);
  const width = aspect >= 1 ? WORK : Math.max(8, Math.round(WORK * aspect));
  const height = aspect >= 1 ? Math.max(8, Math.round(WORK / aspect)) : WORK;
  const work = document.createElement('canvas');
  work.width = width;
  work.height = height;
  const ctx = work.getContext('2d', { willReadFrequently: true });
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(picture, 0, 0, width, height);

  const image = ctx.getImageData(0, 0, width, height);
  const rgb = new Float32Array(width * height * 3);
  for (let p = 0; p < width * height; p++) {
    for (let c = 0; c < 3; c++) rgb[p * 3 + c] = image.data[p * 4 + c];
  }
  blur(rgb, width, height);
  press(rgb, BANDS[scheme], SATURATION[scheme]);
  for (let p = 0; p < width * height; p++) {
    for (let c = 0; c < 3; c++) image.data[p * 4 + c] = rgb[p * 3 + c];
    image.data[p * 4 + 3] = 255;
  }
  ctx.putImageData(image, 0, 0);

  canvas.width = width * UPSCALE;
  canvas.height = height * UPSCALE;
  const out = canvas.getContext('2d');
  out.imageSmoothingEnabled = true;
  out.imageSmoothingQuality = 'high';
  out.drawImage(work, 0, 0, canvas.width, canvas.height);
  // Grain: what makes it frosted rather than merely soft, and keeps the gradients from banding.
  const rand = random(SEED);
  const grain = GRAIN[scheme];
  const shown = out.getImageData(0, 0, canvas.width, canvas.height);
  for (let i = 0; i < shown.data.length; i += 4) {
    const speck = (rand() - 0.5) * grain;
    for (let c = i; c < i + 3; c++) shown.data[c] += speck;
  }
  out.putImageData(shown, 0, 0);
}

function load(src) {
  return new Promise((resolve, reject) => {
    const picture = new Image();
    picture.onload = () => resolve(picture);
    picture.onerror = () => reject(new Error(`cannot load ${src}`));
    picture.src = src;
  });
}

/**
 * Whether any bare glass is in view for the dither to drift over: the title bar and the sidebar
 * with the left panel's background on, the inspector while it is there with its own, or the middle
 * panel while a new session has put the sheet away (styles/glass.css). Everywhere else the dither
 * is covered or cut away.
 */
function glassShows() {
  const at = document.documentElement.dataset;
  return at.bgLeft === 'on' || (at.bgRight === 'on' && at.inspector !== 'off') || (at.bgMiddle === 'on' && at.view === 'new');
}

/** The dither moves only where it can be seen. */
const move = (now) => dither.run(now.motion && now.dither && Boolean(now.background.art) && glassShows());

async function paint(now) {
  const { background, scheme } = now;
  dither.restyle(themes.ditherLook(now));
  move(now);
  const wanted = `${background.id}:${scheme}`;
  if (wanted === shown) return;
  shown = wanted;
  const turn = ++asked;
  // The flat panes of "None" need no picture, and are whole without one that fails to load.
  const picture = background.art ? await load(background.art).catch((problem) => console.warn('backdrop:', problem)) : null;
  // Another choice was made while this one loaded.
  if (turn !== asked) return;
  if (picture) draw(picture, scheme);
  layer.classList.toggle('is-ready', Boolean(picture));
}

/** Paints the chosen background, and again whenever the choice or the system's appearance changes. */
export function startBackdrop() {
  themes.onChange(paint);
  paint(themes.current());
  // The view and the inspector come and go without a change of theme.
  new MutationObserver(() => move(themes.current())).observe(document.documentElement, { attributes: true, attributeFilter: ['data-view', 'data-inspector'] });
}
