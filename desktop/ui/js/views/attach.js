// Files attached to a message: the + of a chat bar, and the files it has picked, which sit in the
// box over the text until the message is sent.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { barButton } from './common.js';

export const baseName = (path) => path.split(/[\\/]/).pop();

/** A file by its name; the whole path is its `title`. With `onRemove`, a cross takes it off. */
export function fileChip(path, onRemove) {
  return chip('file', baseName(path), path, onRemove);
}

/** What goes with a message, in the bar over its text: a glyph and a name. With `onRemove`, a cross takes it off. */
export function chip(glyph, name, title, onRemove) {
  return h(
    'span.file-chip',
    { title },
    icon(glyph, 14),
    h('span.file-name', name),
    onRemove && h('button.file-remove', { type: 'button', 'aria-label': `Remove ${name}`, onclick: onRemove }, icon('cross', 12)),
  );
}

/**
 * `start()` is the folder the system's dialog opens at; `value` the paths attached already;
 * `onChange(paths)` runs when one is added or taken off. Gives `{ button, list, paths(), clear() }`.
 */
export function attachments({ start, value = [], onChange }) {
  let paths = [...value];
  const list = h('div.attached', { hidden: true });
  const button = barButton('attach', 'Attach files', icon('plus', 16), pick, 'bare');
  // Scripts attach by path (js/hooks.js): the system's dialog cannot be driven.
  button.addEventListener('attach', (event) => set([...paths, ...event.detail]));

  function set(next) {
    paths = [...new Set(next)];
    list.hidden = !paths.length;
    list.replaceChildren(...paths.map((path) => fileChip(path, () => set(paths.filter((each) => each !== path)))));
    onChange?.(paths);
  }

  async function pick() {
    set([...paths, ...(await api.pickFiles(start() || null))]);
  }

  set(paths);
  return { button, list, paths: () => paths, clear: () => set([]) };
}
