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
      else el.setAttribute(name, value === true ? '' : String(value));
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
  check: 'M3.5 8.4l3 3 6-6.8',
  bang: 'M8 3.2v5.6M8 11.9v.2',
  cross: 'M4.2 4.2l7.6 7.6M11.8 4.2l-7.6 7.6',
  dash: 'M4.5 8h7',
  denied: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM4.2 4.2l7.6 7.6',
  clock: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM8 5.2V8l2 1.4',
  dot: 'M8 7.2a.8.8 0 100 1.6.8.8 0 000-1.6z',
  reply: 'M6.5 4.5L3 8l3.5 3.5M3.4 8H9a4 4 0 014 4',
  chevron: 'M6 3.8L10.2 8 6 12.2',
  caret: 'M4 6.2l4 4 4-4',
  folder: 'M2 4.6c0-.6.4-1 1-1h3l1.4 1.5H13c.6 0 1 .4 1 1v5.8c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1z',
  up: 'M8 12.5v-9M4.2 7.2L8 3.4l3.8 3.8',
  down: 'M8 3.5v9M4.2 8.8L8 12.6l3.8-3.8',
  key: 'M5.2 5.6a2.6 2.6 0 100 5.2 2.6 2.6 0 000-5.2zM7.8 8.2h5.9M11.4 8.2v2.1M13.7 8.2v1.5',
  plus: 'M8 3.5v9M3.5 8h9',
  sliders: 'M2.5 5h5.9M11.6 5h1.9M10 3.4a1.6 1.6 0 100 3.2 1.6 1.6 0 000-3.2zM2.5 11h1.9M7.6 11h5.9M6 9.4a1.6 1.6 0 100 3.2 1.6 1.6 0 000-3.2z',
  // What a row of work was: a command, a file, a search, an edit, …
  terminal: 'M3 4.8L6.2 8 3 11.2M8.2 11.4h4.8',
  file: 'M4.5 2.5h4.4l2.6 2.7v8.3h-7zM8.8 2.7v2.7h2.5',
  filePlus: 'M4.5 2.5h4.4l2.6 2.7v8.3h-7zM8 7.2v3.8M6.1 9.1h3.8',
  search: 'M7.1 3a4.1 4.1 0 100 8.2 4.1 4.1 0 000-8.2zM10.1 10.1l3.1 3.1',
  pencil: 'M3 13l.6-2.9 6.9-6.9a1.4 1.4 0 012 0l.3.3a1.4 1.4 0 010 2L5.9 12.4zM9.5 4.2l2.3 2.3',
  eye: 'M1.9 8s2.3-3.9 6.1-3.9S14.1 8 14.1 8s-2.3 3.9-6.1 3.9S1.9 8 1.9 8zM8 6.3a1.7 1.7 0 100 3.4 1.7 1.7 0 000-3.4z',
  flag: 'M4 13.5v-11M4 3h7.6l-1.7 2.6 1.7 2.6H4',
  spark: 'M8 2.4v2.4M8 11.2v2.4M2.4 8h2.4M11.2 8h2.4M4.1 4.1l1.6 1.6M10.3 10.3l1.6 1.6M11.9 4.1l-1.6 1.6M5.7 10.3l-1.6 1.6',
  note: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM8 7.4v3.3M8 5.2v.2',
  layers: 'M8 2.7l5.3 2.8L8 8.3 2.7 5.5zM2.9 8.2L8 10.9l5.1-2.7M2.9 10.7L8 13.4l5.1-2.7',
  globe: 'M8 2.5a5.5 5.5 0 100 11 5.5 5.5 0 000-11zM2.6 8h10.8M8 2.5C6.5 4 5.7 5.9 5.7 8s.8 4 2.3 5.5c1.5-1.5 2.3-3.4 2.3-5.5S9.5 4 8 2.5z',
  // Actions.
  copy: 'M5.5 5.5h7v7h-7zM3.5 10.4V3.5h6.9',
  branch: 'M5 3a1.4 1.4 0 100 2.8A1.4 1.4 0 005 3zM5 10.2A1.4 1.4 0 105 13a1.4 1.4 0 000-2.8zM11 4.4a1.4 1.4 0 100 2.8 1.4 1.4 0 000-2.8zM5 5.8v4.4M11 7.2c0 2.3-6 .9-6 3',
  push: 'M8 13.2V5.4M4.6 8.6L8 5.2l3.4 3.4M3.5 2.8h9',
};

// Filled marks. The GitHub one is from Octicons (MIT).
const MARKS = {
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
      title: label,
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
