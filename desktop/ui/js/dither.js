// An ordered dither that moves. Three slow waves, bent a little, add up to a field; where it
// crests the cells turn into light dots and where it dips into dark ones, as densely as the field
// is strong there, by an 8×8 Bayer matrix. The dots are whole cells of a small canvas that the page
// stretches without smoothing, so they stay square. A frame is one pass of table lookups.

const BAYER = Uint8Array.of(
  0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38, 60, 28, 52, 20, 62, 30, 54, 22,
  3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57, 25, 15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
);
const STEPS = 1024;
const WRAP = STEPS - 1;
const SINE = Float32Array.from({ length: STEPS }, (_, i) => Math.sin((i / STEPS) * 2 * Math.PI));
const FPS = 20;
// The moment a dither that does not move is drawn at.
const STILL = 12.5;

/** A colour as one cell of an ImageData seen through a Uint32Array. */
const pack = ([r, g, b, a]) => ((Math.round(a * 255) << 24) | (b << 16) | (g << 8) | r) >>> 0;

/**
 * Draws the dither on `canvas`, at the size the page gives it.
 * `look`: `{ cell, waves: [[x, y, speed] × 3], warp, reach: [from, to], fill, light: [r, g, b, a], dark: [r, g, b, a] }`.
 */
export function createDither(canvas, look) {
  const ctx = canvas.getContext('2d');
  let image = null;
  let cells = null;
  let width = 0;
  let height = 0;
  let bendRows = null;
  let bendCols = null;
  let frame = 0;
  let last = 0;
  let moving = false;

  function fit() {
    const box = canvas.getBoundingClientRect();
    const w = Math.max(1, Math.ceil(box.width / look.cell));
    const h = Math.max(1, Math.ceil(box.height / look.cell));
    if (w === width && h === height) return;
    [width, height] = [w, h];
    canvas.width = w;
    canvas.height = h;
    image = ctx.createImageData(w, h);
    cells = new Uint32Array(image.data.buffer);
    bendRows = new Float32Array(h);
    bendCols = new Float32Array(w);
  }

  function draw(seconds) {
    if (!image) return;
    const [[ax, ay, ahz], [bx, by, bhz], [cx, cy, chz]] = look.waves;
    const [pa, pb, pc] = [seconds * ahz, seconds * bhz, seconds * chz];
    for (let y = 0; y < height; y++) bendRows[y] = look.warp * SINE[((y * 0.0061 + seconds * 0.017) * STEPS) & WRAP];
    for (let x = 0; x < width; x++) bendCols[x] = look.warp * SINE[((x * 0.0047 - seconds * 0.013) * STEPS) & WRAP];
    // The field is a sum of three sines, from -3 to 3; a cell's threshold runs from 0 to 63.
    const [from, to] = look.reach;
    const gain = (64 * look.fill) / (3 * (to - from));
    const floor = 3 * from;
    const most = 64 * look.fill;
    const light = pack(look.light);
    const dark = pack(look.dark);
    for (let y = 0; y < height; y++) {
      const bend = bendRows[y];
      const row = y * width;
      const thresholds = (y & 7) << 3;
      for (let x = 0; x < width; x++) {
        const u = x + bend;
        const v = y + bendCols[x];
        const field = SINE[((u * ax + v * ay + pa) * STEPS) & WRAP] + SINE[((u * bx + v * by + pb) * STEPS) & WRAP] + SINE[((u * cx + v * cy + pc) * STEPS) & WRAP];
        const strength = ((field < 0 ? -field : field) - floor) * gain;
        cells[row + x] = (strength > most ? most : strength) > BAYER[thresholds + (x & 7)] ? (field > 0 ? light : dark) : 0;
      }
    }
    ctx.putImageData(image, 0, 0);
  }

  function tick(now) {
    frame = requestAnimationFrame(tick);
    if (now - last < 1000 / FPS) return;
    last = now;
    draw(now / 1000);
  }

  const watcher = new ResizeObserver(() => {
    fit();
    if (!moving) draw(STILL);
  });
  watcher.observe(canvas);

  return {
    /** Moves while `on` is true; holds one still frame while it is not. */
    run(on) {
      cancelAnimationFrame(frame);
      moving = on;
      fit();
      if (on) frame = requestAnimationFrame(tick);
      else draw(STILL);
    },
    /** Takes another look, at once. */
    restyle(next) {
      look = next;
      width = 0;
      fit();
      if (!moving) draw(STILL);
    },
    stop() {
      cancelAnimationFrame(frame);
      watcher.disconnect();
    },
  };
}
