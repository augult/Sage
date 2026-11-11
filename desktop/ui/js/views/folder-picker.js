// The in-app folder picker, on the engine's `list_dir`.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { openDialog, tag } from './common.js';

/** Opens the picker at `start` (the home directory when empty or unusable). Resolves with the chosen path, or null. */
export function pickFolder(start) {
  return new Promise((resolve) => {
    let listing = null;
    let chosen = null;

    const path = h('input.input.mono', {
      type: 'text',
      'aria-label': 'Folder path',
      spellcheck: 'false',
      onkeydown: (event) => {
        if (event.key !== 'Enter') return;
        event.preventDefault();
        go(path.value);
      },
    });
    const up = h('button.btn', { type: 'button', disabled: true, onclick: () => go(listing.parent) }, icon('up', 12), 'Up');
    const error = h('div.form-error', { role: 'alert', hidden: true });
    const list = h('div.folder-list');
    const kind = h('div');
    const use = h('button.btn.primary', { id: 'use-folder', type: 'button', disabled: true, onclick: useShown }, 'Use this folder');

    function choose(value) {
      chosen = value;
      dialog.close();
    }

    /** Chooses what the path field says. A path typed but not opened yet is opened first, and must exist. */
    async function useShown() {
      if (path.value.trim() !== listing.path && !(await go(path.value))) return;
      choose(listing.path);
    }

    /** Lists `target`. On failure the current listing stays and the reason is shown. */
    async function go(target) {
      try {
        listing = await api.listDir(target || null);
      } catch (problem) {
        error.textContent = String(problem);
        error.hidden = false;
        return false;
      }
      error.hidden = true;
      path.value = listing.path;
      up.disabled = !listing.parent;
      use.disabled = false;
      kind.replaceChildren(listing.is_git ? tag('git repository', 'teal') : h('span.muted', 'Not a git repository'));
      list.replaceChildren(
        ...(listing.dirs.length
          ? listing.dirs.map((dir) => h('button.folder', { type: 'button', title: dir.path, onclick: () => go(dir.path) }, icon('folder'), h('span', dir.name)))
          : [h('p.muted.folder-empty', 'No folders in here.')]),
      );
      list.scrollTop = 0;
      return true;
    }

    const dialog = openDialog('picker', (close) => [
      h('div.dialog-head', h('h2', 'Choose a workspace')),
      h('div.picker-path', path, up),
      error,
      list,
      h('div.dialog-foot', kind, h('button.btn', { type: 'button', onclick: close }, 'Cancel'), use),
    ]);
    dialog.addEventListener('close', () => resolve(chosen));
    go(start).then((listed) => listed || go(''));
  });
}
