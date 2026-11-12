// The Code tab of the pane beside the conversation: the files of the session's folder as a tree
// (`list_dir`), and the one that is open as text to edit (`read_file`, `write_file`). What is typed
// and not saved is asked about before it is left behind (js/unsaved.js). The tree's right-click
// menu makes, renames and deletes files and folders; the open file's name above the text renames it.

import { api } from '../api.js';
import { copyText, h, icon } from '../dom.js';
import { settleUnsaved, track, watchUnsaved } from '../unsaved.js';
import { closeButton, openDialog } from './common.js';

const parentOf = (path) => (path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '');
const join = (folder, name) => (folder ? `${folder}/${name}` : name);
/** `path` is `old` or inside it: then where it is once `old` is called `now`. */
const moved = (path, old, now) => (path === old ? now : path.startsWith(`${old}/`) ? now + path.slice(old.length) : null);

/** `session` is `{ id }`. Gives `{ el, open(path), refresh(), unmount() }`. */
export function codeView(session) {
  // What each folder that has been opened holds, by its path; the root is ''.
  const listed = new Map();
  const unfolded = new Set(['']);
  let file = null;
  let saved = '';
  // How the open file ends its lines. The text box only knows `\n`; what is written gets these back.
  let eol = '\n';
  // A name being typed in the tree: `{ kind: 'rename', path }` or `{ kind: 'new', folder, dir }`.
  let naming = null;

  const tree = h('div.code-tree', { role: 'tree', 'aria-label': 'Files', oncontextmenu: (event) => event.target === tree && menuAt(event, null), onkeydown: onTreeKey });
  const name = h('button.code-path', { type: 'button', title: 'Rename', onclick: renameOpen });
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

  function showName() {
    name.textContent = file;
    name.title = `${file} — click to rename`;
  }

  function show(path, content) {
    file = path;
    eol = content.includes('\r\n') ? '\r\n' : '\n';
    saved = content.replace(/\r\n/g, '\n');
    text.value = saved;
    text.scrollTop = text.scrollLeft = 0;
    showName();
    bar.hidden = editor.hidden = false;
    empty.hidden = true;
    error.hidden = true;
    mark();
    draw();
  }

  /** Nothing open any more: its file is gone. */
  function close() {
    file = null;
    saved = text.value = '';
    bar.hidden = editor.hidden = true;
    empty.hidden = false;
    mark();
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
      api.writeFile(session.id, path, content.replace(/\n/g, eol)).then(
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

  // Making, renaming and deleting.

  /** The box a name is typed in, in place of a row's name. `done(value)` runs once, on Enter or on leaving it. */
  function nameBox(value, done) {
    let settled = false;
    const finish = (keep) => {
      if (settled) return;
      settled = true;
      done(keep ? input.value.trim() : null);
    };
    const input = h('input.tree-input', {
      value,
      spellcheck: 'false',
      'aria-label': 'Name',
      onkeydown(event) {
        event.stopPropagation();
        if (event.key === 'Enter') finish(true);
        else if (event.key === 'Escape') finish(false);
      },
      // Taken out of the page by a redraw is not leaving it.
      onblur: () => input.isConnected && finish(true),
      onclick: (event) => event.stopPropagation(),
      oncontextmenu: (event) => event.stopPropagation(),
    });
    requestAnimationFrame(() => {
      input.focus();
      // The name without its extension, as most editors pick it.
      const dot = value.lastIndexOf('.');
      input.setSelectionRange(0, dot > 0 ? dot : value.length);
    });
    return input;
  }

  async function startNew(folder, dir) {
    try {
      if (folder) await unfold(folder);
      naming = { kind: 'new', folder, dir };
      draw();
    } catch (problem) {
      fail(problem);
    }
  }

  function startRename(path) {
    naming = { kind: 'rename', path };
    draw();
  }

  async function make(folder, dir, entered) {
    naming = null;
    if (!entered) return draw();
    const path = join(folder, entered);
    error.hidden = true;
    try {
      await api.createEntry(session.id, path, dir);
      await list(folder);
      draw();
      if (!dir) await open(path);
    } catch (problem) {
      draw();
      fail(problem);
    }
  }

  /** Moves `from` to `to`, keeping the open file, its unsaved text and the unfolded folders with it. */
  async function rename(from, to) {
    naming = null;
    if (!to || to === from) return draw();
    error.hidden = true;
    try {
      await api.renameEntry(session.id, from, to);
      for (const path of [...unfolded]) {
        const now = moved(path, from, to);
        if (now !== null) {
          unfolded.delete(path);
          unfolded.add(now);
        }
      }
      for (const path of [...listed.keys()]) if (moved(path, from, to) !== null) listed.delete(path);
      const now = file === null ? null : moved(file, from, to);
      if (now !== null) {
        file = now;
        showName();
      }
      await Promise.all([...new Set([parentOf(from), parentOf(to)])].map((folder) => (listed.has(folder) || folder === '' ? list(folder) : null)));
      await Promise.all([...unfolded].filter((path) => !listed.has(path)).map((path) => list(path).catch(() => unfolded.delete(path))));
      draw();
    } catch (problem) {
      draw();
      fail(problem);
    }
  }

  function renameOpen() {
    if (file === null) return;
    const input = nameBox(file, (entered) => {
      input.replaceWith(name);
      rename(file, entered?.replace(/^\/+/, ''));
    });
    // The whole path can be changed here, so a file can move to another folder too; the name is what is picked.
    requestAnimationFrame(() => {
      const dot = file.lastIndexOf('.');
      input.setSelectionRange(file.lastIndexOf('/') + 1, dot > file.lastIndexOf('/') + 1 ? dot : file.length);
    });
    input.classList.add('code-path-input');
    name.replaceWith(input);
  }

  function askDelete(entry) {
    const holdsOpen = file !== null && moved(file, entry.path, '') !== null;
    let go;
    openDialog('confirm-delete', (shut) => {
      go = h(
        'button.btn.danger',
        {
          id: 'delete-go',
          type: 'button',
          async onclick() {
            go.disabled = true;
            try {
              await api.deleteEntry(session.id, entry.path);
              shut();
              if (holdsOpen) close();
              for (const path of [...unfolded]) if (moved(path, entry.path, '') !== null) unfolded.delete(path);
              for (const path of [...listed.keys()]) if (moved(path, entry.path, '') !== null) listed.delete(path);
              await list(parentOf(entry.path));
              draw();
            } catch (problem) {
              shut();
              fail(problem);
            }
          },
        },
        'Delete',
      );
      return [
        h('div.dialog-head', h('h2', `Delete ${entry.dir ? 'folder' : 'file'}?`), closeButton(shut)),
        h(
          'div.unsaved-body',
          h('p', h('span.mono', entry.path), entry.dir ? ' and everything in it will be deleted.' : ' will be deleted.'),
          h('p.muted', holdsOpen && dirty() ? 'Its unsaved changes are lost too. This cannot be undone.' : 'This cannot be undone.'),
        ),
        h('div.dialog-foot', h('button.btn', { type: 'button', onclick: shut }, 'Cancel'), go),
      ];
    });
    go.focus();
  }

  // The right-click menu.

  let menu = null;

  function closeMenu() {
    menu?.remove();
    menu = null;
    document.removeEventListener('pointerdown', outside, true);
    window.removeEventListener('blur', closeMenu);
  }

  function outside(event) {
    if (!menu?.contains(event.target)) closeMenu();
  }

  /** `entry` is the row's `{ path, dir }`, or null for the folder's root. */
  function menuAt(event, entry) {
    event.preventDefault();
    closeMenu();
    const folder = entry === null ? '' : entry.dir ? entry.path : parentOf(entry.path);
    const item = (label, act) =>
      h('button.menu-item', { type: 'button', role: 'menuitem', onclick: () => (closeMenu(), act()) }, h('span.menu-text', label));
    const items = [
      entry && !entry.dir && item('Open', () => open(entry.path)),
      item('New file', () => startNew(folder, false)),
      item('New folder', () => startNew(folder, true)),
      entry && h('div.menu-rule'),
      entry && item('Rename', () => startRename(entry.path)),
      entry && item('Delete', () => askDelete(entry)),
      entry && h('div.menu-rule'),
      entry && item('Copy path', () => copyText(entry.path)),
    ];
    menu = h('div.menu.context-menu', { role: 'menu', onkeydown: onMenuKey }, items);
    document.body.append(menu);
    // Inside the window, by the pointer.
    const { width, height } = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(4, Math.min(event.clientX, innerWidth - width - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(event.clientY, innerHeight - height - 4))}px`;
    menu.querySelector('.menu-item')?.focus();
    document.addEventListener('pointerdown', outside, true);
    window.addEventListener('blur', closeMenu);
  }

  function onMenuKey(event) {
    const items = [...menu.querySelectorAll('.menu-item')];
    const at = items.indexOf(document.activeElement);
    if (event.key === 'Escape') {
      closeMenu();
      tree.querySelector('[aria-current="true"]')?.focus();
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      items[(at + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length].focus();
    }
  }

  /** F2 renames the row that has the focus, Delete deletes it, as in most editors. */
  function onTreeKey(event) {
    const row = event.target.closest?.('.tree-row');
    if (!row || naming) return;
    const entry = { path: row.dataset.path, dir: row.dataset.dir === 'true' };
    if (event.key === 'F2') {
      event.preventDefault();
      startRename(entry.path);
    } else if (event.key === 'Delete') {
      event.preventDefault();
      askDelete(entry);
    } else if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      const box = row.getBoundingClientRect();
      menuAt({ preventDefault: () => event.preventDefault(), clientX: box.left + 24, clientY: box.bottom }, entry);
    }
  }

  // The tree.

  function newRow(depth) {
    return h(
      'div.tree-row.is-naming',
      { style: `padding-left:${6 + depth * 12}px` },
      naming.dir ? icon('chevron', 12) : h('span.tree-gap'),
      icon(naming.dir ? 'folder' : 'file', 14),
      nameBox('', (entered) => make(naming.folder, naming.dir, entered)),
    );
  }

  function rows(path, depth) {
    const entries = (listed.get(path) ?? []).flatMap((entry) => {
      const shown = entry.dir && unfolded.has(entry.path);
      const renaming = naming?.kind === 'rename' && naming.path === entry.path;
      const row = h(
        renaming ? 'div.tree-row.is-naming' : 'button.tree-row',
        {
          type: renaming ? null : 'button',
          role: 'treeitem',
          title: entry.path,
          style: `padding-left:${6 + depth * 12}px`,
          dataset: { path: entry.path, dir: String(entry.dir) },
          'aria-expanded': entry.dir ? String(shown) : null,
          'aria-current': entry.path === file ? 'true' : null,
          onclick: renaming ? null : () => (entry.dir ? toggle(entry.path) : open(entry.path)),
          oncontextmenu: (event) => menuAt(event, entry),
        },
        entry.dir ? icon('chevron', 12) : h('span.tree-gap'),
        icon(entry.dir ? 'folder' : 'file', 14),
        renaming ? nameBox(entry.name, (entered) => rename(entry.path, entered && join(parentOf(entry.path), entered))) : h('span.tree-name', entry.name),
      );
      return shown ? [row, ...rows(entry.path, depth + 1)] : [row];
    });
    return naming?.kind === 'new' && naming.folder === path ? [newRow(depth), ...entries] : entries;
  }

  function draw() {
    const at = tree.querySelector(':focus')?.dataset.path;
    tree.replaceChildren(...rows('', 0));
    if (at !== undefined && !naming) tree.querySelector(`[data-path="${CSS.escape(at)}"]`)?.focus();
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
      // A name being typed keeps its box.
      if (!naming) draw();
      if (file !== null && !dirty()) {
        const content = await api.readFile(session.id, file);
        if (!dirty() && content.replace(/\r\n/g, '\n') !== saved) show(file, content);
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
  return {
    el,
    open,
    refresh,
    unmount() {
      closeMenu();
      unwatch();
    },
  };
}
