// The Code tab of the pane beside the conversation: the files of the session's folder as a tree
// (`list_dir`), and the one that is open as text to edit (`read_file`, `write_file`). What is typed
// and not saved is asked about before it is left behind (js/unsaved.js).

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { settleUnsaved, track, watchUnsaved } from '../unsaved.js';

/** `session` is `{ id }`. Gives `{ el, open(path), refresh(), unmount() }`. */
export function codeView(session) {
  // What each folder that has been opened holds, by its path; the root is ''.
  const listed = new Map();
  const unfolded = new Set(['']);
  let file = null;
  let saved = '';

  const tree = h('div.code-tree', { role: 'tree', 'aria-label': 'Files' });
  const name = h('span.code-path');
  const save = h('button.btn', { id: 'code-save', type: 'button', disabled: true, onclick: () => write().catch(() => {}) }, 'Save');
  const lines = h('pre.code-lines', { 'aria-hidden': 'true' });
  const text = h('textarea', {
    id: 'code-text',
    'aria-label': 'File',
    spellcheck: 'false',
    wrap: 'off',
    autocapitalize: 'off',
    oninput: mark,
    onscroll: () => (lines.scrollTop = text.scrollTop),
    onkeydown: onKey,
  });
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const editor = h('div.code-edit', { hidden: true }, lines, text);
  const empty = h('p.side-note.code-empty', 'No file open');
  const bar = h('div.code-bar', { hidden: true }, name, save);
  const el = h('div.code', tree, h('div.code-pane', bar, error, editor, empty));

  const dirty = () => file !== null && text.value !== saved;

  function fail(problem) {
    error.textContent = String(problem);
    error.hidden = false;
  }

  /** The gutter and the Save button follow the text. */
  function mark() {
    const count = text.value.split('\n').length;
    if (lines.childElementCount !== count) lines.replaceChildren(...Array.from({ length: count }, (_, at) => h('span', String(at + 1))));
    lines.scrollTop = text.scrollTop;
    save.disabled = !dirty();
    name.classList.toggle('is-unsaved', dirty());
  }

  function onKey(event) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      if (dirty()) write().catch(() => {});
    } else if (event.key === 'Tab' && !event.shiftKey && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault();
      // As the file itself indents.
      const indent = /^\t/m.test(text.value) ? '\t' : '  ';
      if (!document.execCommand('insertText', false, indent)) text.setRangeText(indent, text.selectionStart, text.selectionEnd, 'end');
      mark();
    }
  }

  function show(path, content) {
    file = path;
    saved = content;
    text.value = content;
    text.scrollTop = text.scrollLeft = 0;
    name.textContent = path;
    name.title = path;
    bar.hidden = editor.hidden = false;
    empty.hidden = true;
    error.hidden = true;
    mark();
    draw();
  }

  /** Opens the file at `path`, once what was typed in the one open now is saved or let go. */
  async function open(path) {
    if (path === file || !(await settleUnsaved())) return;
    try {
      show(path, await api.readFile(session.id, path));
      // Its folders unfold, so the tree shows where it is.
      const parts = path.split('/').slice(0, -1);
      await Promise.all(parts.map((_, at) => unfold(parts.slice(0, at + 1).join('/'))));
    } catch (problem) {
      fail(problem);
    }
  }

  function write() {
    const [path, content] = [file, text.value];
    error.hidden = true;
    return track(
      api.writeFile(session.id, path, content).then(
        () => {
          if (file === path) saved = content;
          mark();
        },
        (problem) => {
          fail(problem);
          throw problem;
        },
      ),
    );
  }

  async function list(path) {
    listed.set(path, await api.listDir(session.id, path));
  }

  async function unfold(path) {
    if (!listed.has(path)) await list(path);
    unfolded.add(path);
    draw();
  }

  function toggle(path) {
    if (!unfolded.delete(path)) return unfold(path).catch(fail);
    draw();
  }

  function rows(path, depth) {
    return (listed.get(path) ?? []).flatMap((entry) => {
      const shown = entry.dir && unfolded.has(entry.path);
      const row = h(
        'button.tree-row',
        {
          type: 'button',
          role: 'treeitem',
          title: entry.path,
          style: `padding-left:${6 + depth * 12}px`,
          dataset: { path: entry.path },
          'aria-expanded': entry.dir ? String(shown) : null,
          'aria-current': entry.path === file ? 'true' : null,
          onclick: () => (entry.dir ? toggle(entry.path) : open(entry.path)),
        },
        entry.dir ? icon('chevron', 12) : h('span.tree-gap'),
        icon(entry.dir ? 'folder' : 'file', 14),
        h('span.tree-name', entry.name),
      );
      return shown ? [row, ...rows(entry.path, depth + 1)] : [row];
    });
  }

  function draw() {
    const at = tree.querySelector(':focus')?.dataset.path;
    tree.replaceChildren(...rows('', 0));
    if (at !== undefined) tree.querySelector(`[data-path="${CSS.escape(at)}"]`)?.focus();
  }

  /** Reads again what is shown: the folders that are unfolded, and the open file unless it has unsaved text. */
  async function refresh() {
    try {
      const folders = [...unfolded].filter((path) => path === '' || listed.has(path));
      const found = await Promise.allSettled(folders.map((path) => api.listDir(session.id, path)));
      folders.forEach((path, at) => {
        // A folder that is gone folds away.
        if (found[at].status === 'fulfilled') {
          listed.set(path, found[at].value);
        } else if (path !== '') {
          listed.delete(path);
          unfolded.delete(path);
        }
      });
      if (found[0].status === 'rejected') throw found[0].reason;
      draw();
      if (file !== null && !dirty()) {
        const content = await api.readFile(session.id, file);
        if (!dirty() && content !== saved) show(file, content);
      }
    } catch (problem) {
      fail(problem);
    }
  }

  const unwatch = watchUnsaved(() =>
    dirty()
      ? [
          {
            label: file,
            save: write,
            discard: () => {
              text.value = saved;
              mark();
            },
          },
        ]
      : [],
  );

  refresh();
  return { el, open, refresh, unmount: unwatch };
}
