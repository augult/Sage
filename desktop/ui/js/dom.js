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
      else if (PROPERTIES.has(name)) el[name] = value;
      else el.setAttribute(name, value === true ? '' : String(value));
    }
  }
  append(el, hasOptions ? rest.slice(1) : rest);
  return el;
}

export function append(parent, children) {
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
  stop: 'M5 5h6v6H5z',
  chevron: 'M6 3.8L10.2 8 6 12.2',
  folder: 'M2 4.6c0-.6.4-1 1-1h3l1.4 1.5H13c.6 0 1 .4 1 1v5.8c0 .6-.4 1-1 1H3c-.6 0-1-.4-1-1z',
  up: 'M8 12.5v-9M4.2 7.2L8 3.4l3.8 3.8',
  down: 'M8 3.5v9M4.2 8.8L8 12.6l3.8-3.8',
  play: 'M5.5 3.6l7 4.4-7 4.4z',
  key: 'M10 2.5a3.5 3.5 0 11-2.9 5.5L3 12.1v1.4h1.8v-1.3h1.4v-1.4h1.3l.7-.7M10.9 5.6v.1',
};

export function icon(name, size = 16) {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.setAttribute('width', size);
  svg.setAttribute('height', size);
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('icon');
  const path = document.createElementNS(SVG, 'path');
  path.setAttribute('d', ICONS[name] ?? ICONS.dot);
  svg.append(path);
  return svg;
}

export const spinner = () => h('span.spinner', { 'aria-hidden': 'true' });
