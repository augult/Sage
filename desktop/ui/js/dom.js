// DOM construction. Text always becomes a text node; nothing here parses HTML.

const PROPERTIES = new Set(['value', 'checked']);

/**
 * h('button.btn.primary', { type: 'button', onclick: fn, dataset: { id } }, 'Label', child)
 * The options object is optional. Null and false children are skipped.
 */
export function h(spec, ...rest) {
  const [tag, ...classes] = spec.split('.');
  const el = document.createElement(tag);
  if (classes.length) el.className = classes.join(' ');
  const first = rest[0];
  const hasOptions = first != null && typeof first === 'object' && !(first instanceof Node) && !Array.isArray(first);
  if (hasOptions) {
    for (const [name, value] of Object.entries(first)) {
      if (value == null || value === false) continue;
      if (typeof value === 'function') el.addEventListener(name.slice(2), value);
      else if (name === 'dataset') Object.assign(el.dataset, value);
      // Through the CSSOM, which a strict style-src allows; a `style` attribute it would not.
      else if (name === 'style') el.style.cssText = String(value);
      else if (PROPERTIES.has(name)) el[name] = value;
      else el.setAttribute(name === 'title' ? 'data-tooltip' : name, value === true ? '' : String(value));
    }
  }
  append(el, hasOptions ? rest.slice(1) : rest);
  return el;
}

function append(parent, children) {
  for (const child of children.flat(Infinity)) {
    if (child != null && child !== false) parent.append(child);
  }
  return parent;
}

const SVG = 'http://www.w3.org/2000/svg';

// 16×16 stroke icons. Glyphs are drawn rather than typed so they do not depend on installed fonts.
const ICONS = {
  // Every corner is turned: a vertex is a small arc, and a shape's corners are quarter circles.
  check: 'M3.6 8.5l2.6 2.6a.6.6 0 0 0 .9 0l5.3-6',
  bang: 'M8 3.2v5.6M8 11.9v.2',
  cross: 'M4.4 4.4l7.2 7.2M11.6 4.4l-7.2 7.2',
  // The backspace key: a cap that points left, with a cross in it. Deletes.
  backspace: 'M5.2 4.3a1 1 0 0 1 .8-.3h6a1.5 1.5 0 0 1 1.5 1.5v5a1.5 1.5 0 0 1-1.5 1.5H6a1 1 0 0 1-.8-.3L2.8 8.6a1 1 0 0 1 0-1.2zM8 6.4l3.2 3.2M11.2 6.4L8 9.6',
  dash: 'M4.5 8h7',
  denied: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM4.2 4.2l7.6 7.6',
  clock: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM8 5.2V8l2 1.4',
  dot: 'M8 7.2a.8.8 0 100 1.6.8.8 0 000-1.6z',
  reply: 'M6.3 4.6L3.4 7.5a.7.7 0 0 0 0 1l2.9 2.9M3.6 8h5.2a4.2 4.2 0 0 1 4.2 4.2',
  chevron: 'M6 3.8l3.7 3.7a.7.7 0 0 1 0 1L6 12.2',
  back: 'M10 3.8L6.3 7.5a.7.7 0 0 0 0 1l3.7 3.7',
  caret: 'M3.8 6.2l3.7 3.7a.7.7 0 0 0 1 0l3.7-3.7',
  folder: 'M2.5 5.2A1.7 1.7 0 0 1 4.2 3.5h1.9a1.2 1.2 0 0 1 .9.4l.7.8a1.2 1.2 0 0 0 .9.4h3.2a1.7 1.7 0 0 1 1.7 1.7v4a1.7 1.7 0 0 1-1.7 1.7H4.2a1.7 1.7 0 0 1-1.7-1.7z',
  down: 'M8 3.5V12M4.3 8.8l3.2 3.2a.7.7 0 0 0 1 0l3.2-3.2',
  key: 'M5.2 5.6a2.6 2.6 0 100 5.2 2.6 2.6 0 000-5.2zM7.8 8.2h5.9M11.4 8.2v2.1M13.7 8.2v1.5',
  plus: 'M8 3.5v9M3.5 8h9',
  sliders: 'M2.5 5h5.9M11.6 5h1.9M10 3.4a1.6 1.6 0 100 3.2 1.6 1.6 0 000-3.2zM2.5 11h1.9M7.6 11h5.9M6 9.4a1.6 1.6 0 100 3.2 1.6 1.6 0 000-3.2z',
  // What a row of work was: a command, a file, a search, an edit, …
  terminal: 'M3.2 5l2.6 2.5a.7.7 0 0 1 0 1L3.2 11M8.4 11.2h4.4',
  file: 'M5.2 2.5h3.4a1.2 1.2 0 0 1 .85.35l2.2 2.2a1.2 1.2 0 0 1 .35.85v5.9a1.7 1.7 0 0 1-1.7 1.7H5.7A1.7 1.7 0 0 1 4 11.8V3.7a1.2 1.2 0 0 1 1.2-1.2zM8.9 2.8v1.9a1 1 0 0 0 1 1h1.9',
  filePlus: 'M5.2 2.5h3.4a1.2 1.2 0 0 1 .85.35l2.2 2.2a1.2 1.2 0 0 1 .35.85v5.9a1.7 1.7 0 0 1-1.7 1.7H5.7A1.7 1.7 0 0 1 4 11.8V3.7a1.2 1.2 0 0 1 1.2-1.2zM8 7.4V11M6.2 9.2h3.6',
  search: 'M7.1 3a4.1 4.1 0 100 8.2 4.1 4.1 0 000-8.2zM10.1 10.1l3.1 3.1',
  pencil: 'M3.2 12.8l.5-2.4a1.2 1.2 0 0 1 .3-.6l6.6-6.6a1.4 1.4 0 0 1 2 0l.2.2a1.4 1.4 0 0 1 0 2l-6.6 6.6a1.2 1.2 0 0 1-.6.3zM9.6 4.2l2.2 2.2',
  eye: 'M1.9 8s2.3-3.9 6.1-3.9S14.1 8 14.1 8s-2.3 3.9-6.1 3.9S1.9 8 1.9 8zM8 6.3a1.7 1.7 0 100 3.4 1.7 1.7 0 000-3.4z',
  flag: 'M4 13.5V3M4 3.6c1.4-.9 2.6-.9 4 0s2.6.9 4 0v5c-1.4.9-2.6.9-4 0s-2.6-.9-4 0',
  // The model at thought, and what it thought once it has.
  spark: 'M8 2.4v2.4M8 11.2v2.4M2.4 8h2.4M11.2 8h2.4M4.1 4.1l1.6 1.6M10.3 10.3l1.6 1.6M11.9 4.1l-1.6 1.6M5.7 10.3l-1.6 1.6',
  thought: 'M8 2.5a4 4 0 0 0-2.5 7.1c.5.4.8 1 .8 1.6a.6.6 0 0 0 .6.6h2.2a.6.6 0 0 0 .6-.6c0-.6.3-1.2.8-1.6A4 4 0 0 0 8 2.5zM6.9 13.7h2.2',
  note: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM8 7.4v3.3M8 5.2v.2',
  layers: 'M8.7 2.9l4.3 2.2a.5.5 0 0 1 0 .9L8.7 8.2a1.5 1.5 0 0 1-1.4 0L3 6a.5.5 0 0 1 0-.9l4.3-2.2a1.5 1.5 0 0 1 1.4 0zM2.9 8.6l4.4 2.2a1.5 1.5 0 0 0 1.4 0l4.4-2.2M2.9 11l4.4 2.2a1.5 1.5 0 0 0 1.4 0l4.4-2.2',
  globe: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM2.6 8h10.8M8 2.5C6.5 4 5.7 5.9 5.7 8s.8 4 2.3 5.5c1.5-1.5 2.3-3.4 2.3-5.5S9.5 4 8 2.5z',
  // Actions.
  copy: 'M7.2 5.5h4.1A1.7 1.7 0 0 1 13 7.2v4.1a1.7 1.7 0 0 1-1.7 1.7H7.2a1.7 1.7 0 0 1-1.7-1.7V7.2a1.7 1.7 0 0 1 1.7-1.7zM3 9.9V4.7A1.7 1.7 0 0 1 4.7 3h5.2',
  branch: 'M5 3a1.4 1.4 0 100 2.8A1.4 1.4 0 005 3zM5 10.2A1.4 1.4 0 105 13a1.4 1.4 0 000-2.8zM11 4.4a1.4 1.4 0 100 2.8 1.4 1.4 0 000-2.8zM5 5.8v4.4M11 7.2c0 2.3-6 .9-6 3',
  // The Return key's arrow: down from the upper right, then off to the left.
  send: 'M5.9 7L3.5 9.4a.7.7 0 0 0 0 1l2.4 2.4M3.7 9.9h7.3a1.4 1.4 0 0 0 1.4-1.4V3.4',
  // A microphone: its head, the cradle that stands clear of it, and the stem.
  mic: 'M8 1.8a1.8 1.8 0 0 0-1.8 1.8v3.6a1.8 1.8 0 0 0 3.6 0V3.6A1.8 1.8 0 0 0 8 1.8zM3.4 7.2a4.6 4.6 0 0 0 9.2 0M8 11.8v2.4',
  // A key with a slash on it: what a command typed into a chat bar switched on.
  slash: 'M4.7 2.5h6.6a2.2 2.2 0 0 1 2.2 2.2v6.6a2.2 2.2 0 0 1-2.2 2.2H4.7a2.2 2.2 0 0 1-2.2-2.2V4.7a2.2 2.2 0 0 1 2.2-2.2zM9.3 5.4l-2.6 5.2',
  // The window with its left or its right pane marked off: what puts that pane away and brings it back.
  paneLeft: 'M3.8 3h8.4A1.8 1.8 0 0 1 14 4.8v6.4a1.8 1.8 0 0 1-1.8 1.8H3.8A1.8 1.8 0 0 1 2 11.2V4.8A1.8 1.8 0 0 1 3.8 3zM6.2 3v10',
  paneRight: 'M3.8 3h8.4A1.8 1.8 0 0 1 14 4.8v6.4a1.8 1.8 0 0 1-1.8 1.8H3.8A1.8 1.8 0 0 1 2 11.2V4.8A1.8 1.8 0 0 1 3.8 3zM9.8 3v10',
  // What the pane beside the conversation shows.
  chart: 'M3.5 12.5V9.5M8 12.5v-9M12.5 12.5V7',
  code: 'M5.8 4.8L3 7.5a.7.7 0 0 0 0 1l2.8 2.7M10.2 4.8L13 7.5a.7.7 0 0 1 0 1l-2.8 2.7',
};

// Filled marks. The GitHub one is from Octicons (MIT).
const MARKS = {
  // A triangle with its corners turned. The stroke and the point run against it, so they are cut out.
  warn: 'M6.79 4.02a1.4 1.4 0 0 1 2.42 0l4.19 7.28a1.4 1.4 0 0 1-1.21 2.1H3.82a1.4 1.4 0 0 1-1.22-2.1zM8.8 6.8a.8.8 0 0 0-1.6 0v2.2a.8.8 0 0 0 1.6 0zM8.85 11.45a.85.85 0 1 0-1.7 0a.85.85 0 1 0 1.7 0z',
  stop: 'M5.5 3h5A2.5 2.5 0 0 1 13 5.5v5a2.5 2.5 0 0 1-2.5 2.5h-5A2.5 2.5 0 0 1 3 10.5v-5A2.5 2.5 0 0 1 5.5 3z',
  github:
    'M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z',
};

export function icon(name, size = 16) {
  const mark = Object.hasOwn(MARKS, name);
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('width', size);
  svg.setAttribute('height', size);
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add(mark ? 'mark' : 'icon');
  const path = document.createElementNS(SVG, 'path');
  path.setAttribute('d', mark ? MARKS[name] : Object.hasOwn(ICONS, name) ? ICONS[name] : ICONS.dot);
  svg.append(path);
  return svg;
}

export const spinner = () => h('span.spinner', { 'aria-hidden': 'true' });

/** The mark of a run in progress: a dot, and around it a ring that swells and fades (styles/base.css). */
export function pulse(size = 16) {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('width', size);
  svg.setAttribute('height', size);
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('pulse');
  for (const part of ['pulse-ring', 'pulse-dot']) {
    const circle = document.createElementNS(SVG, 'circle');
    circle.setAttribute('cx', 8);
    circle.setAttribute('cy', 8);
    circle.setAttribute('r', 3);
    circle.classList.add(part);
    svg.append(circle);
  }
  return svg;
}

/** Puts `text` on the clipboard. Resolves to whether it got there. */
export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // A webview that withholds the clipboard API still copies a selection.
    const area = h('textarea.offscreen', { value: text, readonly: true, 'aria-hidden': 'true', tabindex: '-1' });
    document.body.append(area);
    area.select();
    let copied = false;
    try {
      copied = document.execCommand('copy');
    } catch {
      copied = false;
    }
    area.remove();
    return copied;
  }
}

/** A small button that copies what `text()` returns, and shows a check for a moment once it has. */
export function copyButton(text, label = 'Copy') {
  const button = h(
    'button.act',
    {
      type: 'button',
      'data-tooltip': label,
      'aria-label': label,
      onclick: async (event) => {
        event.stopPropagation();
        if (!(await copyText(text()))) return;
        button.replaceChildren(icon('check', 14));
        setTimeout(() => button.replaceChildren(icon('copy', 14)), 1200);
      },
    },
    icon('copy', 14),
  );
  return button;
}
