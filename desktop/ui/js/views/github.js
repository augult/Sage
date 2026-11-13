// The GitHub tab of the pane beside the conversation: where the session's folder stands with git
// (`git_state`), and one button that commits what is uncommitted and pushes the branch
// (`git_push`). It says what it will do before it does it: the remote, the branch, every file
// that would go into the commit. Under that, the latest commits.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { firstLine, plural } from '../format.js';
import { closeButton, openDialog } from './common.js';

// Changes that can go back to what the last commit has.
const REVERTIBLE = new Set(['modified', 'deleted']);

// Longer than this and a commit's first line is cut off in most places it is shown.
const SUBJECT = 72;

/**
 * `session` is `{ id, title }`. `onState(state)` runs with each reading; `openFile(path)` shows a
 * changed file in the editor; `onDiscard(path)` runs once a file is put back. Gives `{ el, load() }`.
 */
export function githubView(session, { onState, openFile, onDiscard }) {
  const body = h('div.git-body', h('p.side-note', 'Reading…'));
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const note = h('span.push-note', { role: 'status' });
  const message = h('textarea.input', { id: 'push-message', rows: 2, 'aria-label': 'Commit message', placeholder: 'Commit message', spellcheck: 'false', oninput: () => enable() });
  message.value = firstLine(session.title).slice(0, SUBJECT);
  const go = h('button.btn.primary', { id: 'push-go', type: 'button', disabled: true, onclick: push });
  const foot = h('div.git-foot', note, go);
  const log = h('div.commits', { hidden: true });
  const el = h('div.git', body, error, foot, log);
  let state = null;
  let busy = false;
  let reading = 0;
  // A reading clears only its own error: a failed push or discard stays said after the reading that follows it.
  let readFailed = false;

  /** What the button would do now: commit and push, push, or nothing. */
  const pending = () => (!state?.repo || !state.branch || !state.remote ? 'none' : state.files.length ? 'commit' : state.ahead > 0 || !state.upstream ? 'push' : 'none');

  function enable() {
    go.disabled = busy || pending() === 'none' || (pending() === 'commit' && !message.value.trim());
  }

  function draw() {
    go.replaceChildren(...(state.github ? [icon('github', 14)] : []), pending() === 'commit' ? 'Commit and push' : 'Push');
    foot.hidden = !state.repo;
    enable();
    if (!state.repo) {
      body.replaceChildren(h('p.side-note', 'Not a git repository'));
      log.hidden = true;
      return;
    }
    const files = state.files;
    // With files to list, it introduces them: "2 changes:".
    const standing = files.length ? `${plural(files.length, 'change')}:` :state.ahead > 0 ? `${plural(state.ahead, 'commit')} to push` : !state.remote || !state.branch || state.upstream ? 'Up to date' : 'Branch not on the remote';
    // The message keeps what was typed in it and where the caret was, across readings.
    const typing = document.activeElement === message;
    body.replaceChildren(
      h(
        'div.push-target',
        h('div.push-line', icon(state.github ? 'github' : 'branch', 16), h('span.push-repo', { title: state.github ?? state.url }, state.github ?? state.url ?? 'No remote')),
        h('div.push-line', h('span.push-branch', { title: state.branch }, state.branch ?? 'No branch checked out'), state.remote && state.branch && h('span.muted', `→ ${state.remote}`)),
      ),
      h('div.push-standing', standing),
      // Only with something to commit: its files, and the message they go under.
      ...(files.length > 0
        ? [
            h(
              'div.push-files',
              files.map((file) =>
                h(
                  'div.push-row',
                  // What is gone cannot be opened.
                  h(file.status === 'deleted' ? 'div.push-file' : 'button.push-file', { type: file.status !== 'deleted' && 'button', title: file.path, onclick: file.status !== 'deleted' && (() => openFile(file.path)) }, h('span.push-kind', file.status), h('span.push-path', file.path)),
                  // What the last commit has can be put back; a new file has nothing to go back to.
                  REVERTIBLE.has(file.status) && h('button.push-revert', { type: 'button', title: 'Discard changes', 'aria-label': `Discard changes to ${file.path}`, onclick: () => askRevert(file) }, icon('back', 14)),
                ),
              ),
            ),
            message,
          ]
        : []),
    );
    log.replaceChildren(
      ...(state.commits.length > 0
        ? [
            h('div.side-title', h('span.label', 'Commits')),
            ...state.commits.map((commit, at) =>
              h(
                'div.commit',
                h('span.commit-subject', { title: commit.subject }, commit.subject),
                h('span.commit-facts', h('span.mono', commit.hash), ` · ${commit.when}`, at < state.ahead && h('span.commit-ahead', 'not pushed')),
              ),
            ),
          ]
        : []),
    );
    log.hidden = !state.commits.length;
    if (typing) message.focus();
  }

  function askRevert(file) {
    let go;
    openDialog('confirm-delete', (shut) => {
      go = h(
        'button.btn.danger',
        {
          id: 'revert-go',
          type: 'button',
          async onclick() {
            go.disabled = true;
            error.hidden = true;
            try {
              await api.gitDiscard(session.id, file.path);
            } catch (problem) {
              readFailed = false;
              error.textContent = String(problem);
              error.hidden = false;
            }
            shut();
            await load();
            onDiscard?.(file.path);
          },
        },
        'Discard',
      );
      return [
        h('div.dialog-head', h('h2', 'Discard changes?'), closeButton(shut)),
        h('div.unsaved-body', h('p', h('span.mono', file.path), file.status === 'deleted' ? ' comes back as the last commit has it.' : ' goes back to what the last commit has.'), h('p.muted', 'This cannot be undone.')),
        h('div.dialog-foot', h('button.btn', { type: 'button', onclick: shut }, 'Cancel'), go),
      ];
    });
    go.focus();
  }

  async function load() {
    const asked = ++reading;
    try {
      const read = await api.gitState(session.id);
      // A slower answer to an earlier question must not replace a later one.
      if (asked !== reading) return;
      state = read;
      if (readFailed) error.hidden = true;
      readFailed = false;
      draw();
      onState(state);
    } catch (problem) {
      if (asked !== reading) return;
      readFailed = true;
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
    } catch (problem) {
      note.textContent = '';
      readFailed = false;
      error.textContent = String(problem);
      error.hidden = false;
    }
    busy = false;
    // Whatever happened, show where things stand now: a commit may have been made before a push failed.
    await load();
  }

  return { el, load };
}

/** How many things wait to be pushed: changed files and commits the remote does not have. */
export const waiting = (state) => (state?.repo ? state.files.length + state.ahead : 0);
