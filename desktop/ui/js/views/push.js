// The Push dialog: where the session's workspace stands with git, and one button that commits
// what is uncommitted and pushes the branch (`git_state`, `git_push`). It says what it will do
// before it does it: the remote, the branch, every file that would go into the commit.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { firstLine, plural } from '../format.js';
import { openDialog } from './common.js';

// Longer than this and a commit's first line is cut off in most places it is shown.
const SUBJECT = 72;

/** Opens the dialog for `session` (`{ id, title }`). `onChange()` runs after anything was pushed, and on closing. */
export function openPush(session, onChange) {
  const body = h('div.push-body', h('p.muted', 'Reading…'));
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const note = h('span.push-note', { role: 'status' });
  const message = h('textarea.input', { id: 'push-message', rows: 2, 'aria-label': 'Commit message', placeholder: 'Commit message', spellcheck: 'false', oninput: () => enable() });
  message.value = firstLine(session.title).slice(0, SUBJECT);
  const go = h('button.btn.primary', { id: 'push-go', type: 'button', disabled: true, onclick: push });
  const title = h('h2', 'Push');
  let state = null;
  let busy = false;

  const dialog = openDialog('push', (close) => [
    h('div.dialog-head', title, h('button.btn.quiet', { type: 'button', onclick: close }, 'Close')),
    body,
    error,
    h('div.dialog-foot', note, go),
  ]);
  dialog.addEventListener('close', () => onChange());

  /** What the button would do now: commit and push, push, or nothing. */
  const pending = () => (!state?.repo || !state.branch || !state.remote ? 'none' : state.files.length ? 'commit' : state.ahead > 0 || !state.upstream ? 'push' : 'none');

  function enable() {
    go.disabled = busy || pending() === 'none' || (pending() === 'commit' && !message.value.trim());
  }

  function draw() {
    title.textContent = state.github ? 'Push to GitHub' : 'Push';
    go.textContent = pending() === 'commit' ? 'Commit and push' : 'Push';
    go.hidden = !state.repo;
    enable();
    if (!state.repo) {
      body.replaceChildren(h('p.muted', 'Not a git repository'));
      return;
    }
    const files = state.files;
    const standing = files.length ? plural(files.length, 'change') : state.ahead > 0 ? `${plural(state.ahead, 'commit')} to push` : !state.remote || !state.branch || state.upstream ? 'Up to date' : 'Branch not on the remote';
    body.replaceChildren(
      h(
        'div.push-target',
        icon(state.github ? 'github' : 'branch', 16),
        h('span.push-repo', state.github ?? state.url ?? 'No remote'),
        h('span.push-branch', state.branch ?? 'No branch checked out'),
        state.remote && state.branch && h('span.muted', `→ ${state.remote}`),
      ),
      h('div.push-standing', standing),
      files.length > 0 && h('div.push-files', files.map((file) => h('div.push-file', h('span.push-kind', file.status), h('span.push-path', { title: file.path }, file.path)))),
      files.length > 0 && message,
    );
  }

  async function load() {
    try {
      state = await api.gitState(session.id);
      draw();
    } catch (problem) {
      body.replaceChildren();
      error.textContent = String(problem);
      error.hidden = false;
    }
  }

  async function push() {
    busy = true;
    enable();
    error.hidden = true;
    note.textContent = 'Pushing…';
    try {
      const done = await api.gitPush(session.id, message.value);
      note.textContent = done.pushed ? `Pushed ${done.commit ? `${done.commit} ` : ''}to ${done.remote}/${done.branch}` : 'Already up to date';
      onChange();
    } catch (problem) {
      note.textContent = '';
      error.textContent = String(problem);
      error.hidden = false;
    }
    busy = false;
    // Whatever happened, show where things stand now: a commit may have been made before a push failed.
    await load();
  }

  load();
}
