// What has been typed and not saved, wherever it lives, and the reminder that asks about it before
// it is left behind: on leaving Settings, and on closing the window.

import { h } from './dom.js';
import { closeButton, openDialog } from './views/common.js';

const sources = new Set();

/**
 * `source()` says what is unsaved now: `[{ label, save(): Promise, discard() }]`.
 * Returns the function that stops asking it.
 */
export function watchUnsaved(source) {
  sources.add(source);
  return () => sources.delete(source);
}

const busy = new Set();

/** Counts `saving` as work in progress: nothing is settled until it has ended. Returns it. */
export function track(saving) {
  busy.add(saving);
  const done = () => busy.delete(saving);
  saving.then(done, done);
  return saving;
}

let asking = null;

/**
 * Resolves `true` when nothing is unsaved, or once the user has saved or discarded what was;
 * `false` when they chose to stay.
 */
export async function settleUnsaved() {
  await Promise.allSettled([...busy]);
  const items = [...sources].flatMap((source) => source());
  if (!items.length) return true;
  asking ??= ask(items).finally(() => (asking = null));
  return asking;
}

function ask(items) {
  return new Promise((resolve) => {
    let answer = false;
    const error = h('div.form-error', { role: 'alert', hidden: true });
    const save = h('button.btn.primary', { id: 'unsaved-save', type: 'button', onclick: saveAll }, 'Save');
    const discard = h('button.btn.danger', { id: 'unsaved-discard', type: 'button', onclick: discardAll }, 'Discard');

    const dialog = openDialog('unsaved', (close) => [
      h('div.dialog-head', h('h2', 'Unsaved changes'), closeButton(close)),
      h('div.unsaved-body', h('ul.unsaved-list', items.map((item) => h('li', item.label))), error),
      h('div.dialog-foot', discard, h('button.btn', { type: 'button', onclick: close }, 'Cancel'), save),
    ]);
    dialog.addEventListener('close', () => resolve(answer));
    save.focus();

    async function saveAll() {
      save.disabled = discard.disabled = true;
      error.hidden = true;
      try {
        for (const item of items) await item.save();
        answer = true;
        dialog.close();
      } catch (problem) {
        error.textContent = String(problem);
        error.hidden = false;
        save.disabled = discard.disabled = false;
      }
    }

    function discardAll() {
      for (const item of items) item.discard();
      answer = true;
      dialog.close();
    }
  });
}
