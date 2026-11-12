// Small pieces shared by the views.

import { h, icon, pulse } from '../dom.js';
import { pendingApprovals } from '../model.js';
import * as prefs from '../prefs.js';

export const MOD_KEY = navigator.platform.startsWith('Mac') ? 'Cmd' : 'Ctrl';

// What sends a message: Enter alone, or Enter with Ctrl/Cmd. The other one breaks the line.
// Chosen in Settings and kept by the window.
export const SEND_KEYS = ['enter', 'mod'];
export const sendKey = () => (prefs.get('send') === 'mod' ? 'mod' : 'enter');
export const sendKeyLabel = (key = sendKey()) => (key === 'mod' ? `${MOD_KEY}+Enter` : 'Enter');

/** The keydown of a box that sends: `send()` on the chosen key, a new line on the other. */
export function onSendKey(event, send) {
  // Enter that ends an IME composition belongs to the composition. Shift+Enter is always a new line.
  if (event.key !== 'Enter' || event.isComposing || event.shiftKey) return;
  const mod = event.ctrlKey || event.metaKey;
  if (mod === (sendKey() === 'mod')) {
    event.preventDefault();
    send();
  } else if (mod) {
    // A textarea does nothing with Ctrl+Enter by itself.
    event.preventDefault();
    const box = event.target;
    // execCommand keeps the box's undo history; setRangeText is for a webview without it.
    if (!document.execCommand('insertText', false, '\n')) {
      box.setRangeText('\n', box.selectionStart, box.selectionEnd, 'end');
      box.dispatchEvent(new Event('input', { bubbles: true }));
    }
  }
}

/**
 * The chat bar, the same in a new session as in a running one, so that it is where it was once
 * the first message is sent: what went wrong over it; the box, with the attached files over the
 * text, what attaches one before it and what sends it after; and under the box the pickers.
 */
export function chatBar({ over, files, box, buttons, pickers }) {
  return h('div.chat', over, h('div.composer', files.list, h('div.composer-row', files.button, box, buttons)), h('div.composer-under', pickers));
}

/**
 * What sends, stops or attaches: a square button that is its glyph. `look` is `primary` for the
 * one that sends, `soft` for the one that stops and `bare` for the one that adds to the message.
 * `label` is its name to those who cannot see it.
 */
export function barButton(id, label, glyph, onclick, look = 'primary') {
  return h(`button.bar-btn.${look}`, { id, type: 'button', title: label, 'aria-label': label, onclick }, glyph);
}

/** Makes a message box as tall as its text. The stylesheet says how tall it may get. */
export function fitBox(box) {
  box.style.height = 'auto';
  box.style.height = `${box.scrollHeight}px`;
}

// Tones carry meaning without hue (styles/base.css): `good` held or done, `alert` wants a look,
// `quiet` is the model's side at work, `danger` failed, `plain` is none of these.

/** How a finished run reads: glyph, tone and words. */
export function outcomeLook(status, verified) {
  switch (status) {
    case 'done':
      return verified ? { icon: 'check', tone: 'good', label: 'Done' } : { icon: 'check', tone: 'alert', label: 'Done, unverified' };
    case 'yielded':
      return { icon: 'reply', tone: 'quiet', label: 'Handed back' };
    case 'stopped':
      return { icon: 'stop', tone: 'plain', label: 'Stopped' };
    case 'budget':
      return { icon: 'stop', tone: 'alert', label: 'Out of budget' };
    case 'error':
      return { icon: 'cross', tone: 'danger', label: 'Failed' };
    default:
      return { icon: 'dash', tone: 'plain', label: status || 'No result' };
  }
}

// Why a stored session cannot be continued, in a word (the engine's `why_closed`).
const WHY_CLOSED = { older: 'older', simulated: 'simulated', busy: 'in use', gone: 'folder gone' };

/**
 * What a session is doing now: from its view-model once it is loaded, else from the listing. A
 * stored session that can be continued is idle, like one that is running and waiting; one that
 * cannot is `closed`, and says why.
 */
export function sessionState(meta, entry) {
  const model = entry?.model;
  const closed = entry ? !entry.live && !entry.resumable : meta.status === 'closed';
  const why = closed ? known(WHY_CLOSED, (entry?.meta ?? meta).why_closed) ?? null : null;
  if (!model) return { phase: closed ? 'closed' : meta.status === 'running' ? 'running' : 'idle', outcome: meta.outcome ?? null, verified: meta.verified === true, approvals: 0, why };
  const phase = closed ? 'closed' : model.running ? 'running' : 'idle';
  return { phase, outcome: model.outcome?.status ?? null, verified: model.outcome?.verified === true, approvals: phase === 'running' ? pendingApprovals(model) : 0, why };
}

/**
 * The mark after a session's title, with its meaning as a tooltip and accessible name: a triangle
 * when it wants a look (an approval to give, a run that failed), a dot while it runs. A session
 * at rest has none.
 */
export function stateMark(state) {
  if (state.approvals > 0) return glyph(icon('warn', 14), 'alert', 'Waiting for your approval');
  if (state.phase === 'running') return glyph(pulse(14), 'quiet', 'Running');
  if (state.outcome === 'error') return glyph(icon('warn', 14), 'danger', 'Failed');
  return null;
}

function glyph(mark, tone, label) {
  return h(`span.glyph.tone-${tone}`, { title: label, role: 'img', 'aria-label': label }, mark);
}

export const tag = (text, tone = 'plain', title) => h(`span.tag.tone-${tone}`, { title }, text);

/** Looks up a value the engine named. Anything this build does not know comes back undefined. */
export const known = (table, name) => (Object.hasOwn(table, name) ? table[name] : undefined);

/** The cross that closes a dialog. */
export const closeButton = (close) => h('button.close', { type: 'button', title: 'Close', 'aria-label': 'Close', onclick: close }, icon('cross', 14));

/** A modal dialog that removes itself when closed. `build(close)` returns its children. */
export function openDialog(className, build) {
  const dialog = h(`dialog.${className}`);
  dialog.append(...build(() => dialog.close()));
  dialog.addEventListener('close', () => dialog.remove());
  document.body.append(dialog);
  dialog.showModal();
  // Opening goes to the first control, which must not be the cross: the dialog takes the focus instead.
  if (document.activeElement?.classList.contains('close')) {
    dialog.tabIndex = -1;
    dialog.focus();
  }
  return dialog;
}
