// Viewer for a saved output (`out:N`): everything the command printed, not only what the model saw.

import { api } from '../api.js';
import { h } from '../dom.js';
import { bytes, plural } from '../format.js';
import { closeButton, openDialog } from './common.js';

export function openOutput(sessionId, handle) {
  const size = h('span.muted');
  const body = h('pre.output-body', 'Loading…');
  openDialog('output', (close) => [
    h('div.dialog-head', h('h2', h('span.mono', handle), ' · full output'), size, closeButton(close)),
    body,
  ]);
  api.readOutput(sessionId, handle).then(
    (text) => {
      body.textContent = text || '(no output)';
      const lines = text ? text.replace(/\n$/, '').split('\n').length : 0;
      size.textContent = `${plural(lines, 'line')} · ${bytes(new TextEncoder().encode(text).length)}`;
    },
    (problem) => {
      body.textContent = String(problem);
      body.classList.add('is-error');
    },
  );
}
