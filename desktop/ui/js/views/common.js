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
 * The chat bar, of a new session and of a running one. What went wrong lies over it. The bar is a
 * pane of frosted glass: the attached files and the skills in use over the text, and along its
 * foot what attaches a file and `start` at one end, `end` at the other, which closes with what
 * sends. Under the bar is `under`: what is chosen for the session rather than for the message.
 * `skills` is the bar's `skillCommands` (js/views/skills.js).
 */
export function chatBar({ over, files, skills, box, start, end, under }) {
  const foot = h('div.composer-foot', h('div.composer-side', files.button, start), h('div.composer-side', end));
  const composer = h('div.composer', skills.menu, files.list, skills.chips, box, foot);
  // The pane is the box's own: a press on its surface, between the controls, goes to the text.
  composer.addEventListener('mousedown', (event) => {
    if (event.target.closest('button, textarea, .menu, .file-chip')) return;
    event.preventDefault();
    box.focus();
  });
  return h('div.chat', over, composer, h('div.composer-under', under));
}

/** What sends is grey until `box` holds something to send. */
export function markIdle(button, box) {
  button.classList.toggle('is-idle', !box.value.trim());
}

/**
 * What sends, stops or attaches: a square button that is its glyph. `look` is `primary` for the
 * one that sends, `soft` for the one that stops and `bare` for the one that adds to the message.
 * `label` is its name to those who cannot see it.
 */
export function barButton(id, label, glyph, onclick, look = 'primary') {
  return h(`button.bar-btn.${look}`, { id, type: 'button', 'data-tooltip': label, 'aria-label': label, onclick }, glyph);
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
  return h(`span.glyph.tone-${tone}`, { 'data-tooltip': label, role: 'img', 'aria-label': label }, mark);
}

export const tag = (text, tone = 'plain', title) => h(`span.tag.tone-${tone}`, { 'data-tooltip': title && title !== text ? title : null }, text);

/** Looks up a value the engine named. Anything this build does not know comes back undefined. */
export const known = (table, name) => (Object.hasOwn(table, name) ? table[name] : undefined);

/** The cross that closes a dialog. */
export const closeButton = (close) => h('button.close', { type: 'button', 'data-tooltip': 'Close', 'aria-label': 'Close', onclick: close }, icon('cross', 14));

// The right-click menu, while one is open: there is never more than one.
let contextShown = null;

function contextOutside(event) {
  if (!contextShown?.contains(event.target)) closeContextMenu();
}

export function closeContextMenu() {
  contextShown?.remove();
  contextShown = null;
  document.removeEventListener('pointerdown', contextOutside, true);
  window.removeEventListener('blur', closeContextMenu);
}

/**
 * The menu of what was right-clicked, where the pointer was, in place of the webview's own.
 * `rows` is `[{ label, act, danger, glyph, disabled, title }]`, with `'rule'` between two for a
 * line; what is false among them is left out. Arrows move, Escape closes and gives the focus to
 * what `back()` returns, and Backspace or Delete does `remove`, the Delete row's shortcut, where
 * there is one.
 */
export function contextMenu(event, rows, { remove, back } = {}) {
  event.preventDefault();
  closeContextMenu();
  const item = (row) =>
    h(
      `button.menu-item${row.danger ? '.is-danger' : ''}`,
      { type: 'button', role: 'menuitem', disabled: row.disabled, 'data-tooltip': row.title, onclick: () => (closeContextMenu(), row.act()) },
      h('span.menu-text', row.label),
      row.glyph && h('span.menu-glyph', icon(row.glyph, 14)),
    );
  const menu = h('div.menu.context-menu', { role: 'menu' }, rows.filter(Boolean).map((row) => (row === 'rule' ? h('div.menu-rule') : item(row))));
  menu.addEventListener('keydown', (keyed) => {
    const items = [...menu.querySelectorAll('.menu-item:not(:disabled)')];
    const at = items.indexOf(document.activeElement);
    if ((keyed.key === 'Backspace' || keyed.key === 'Delete') && remove) {
      keyed.preventDefault();
      closeContextMenu();
      remove();
    } else if (keyed.key === 'Escape') {
      closeContextMenu();
      back?.()?.focus();
    } else if ((keyed.key === 'ArrowDown' || keyed.key === 'ArrowUp') && items.length) {
      keyed.preventDefault();
      items[(at + (keyed.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length].focus();
    }
  });
  document.body.append(menu);
  // Inside the window, by the pointer.
  const { width, height } = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(4, Math.min(event.clientX, innerWidth - width - 4))}px`;
  menu.style.top = `${Math.max(4, Math.min(event.clientY, innerHeight - height - 4))}px`;
  menu.querySelector('.menu-item:not(:disabled)')?.focus();
  contextShown = menu;
  document.addEventListener('pointerdown', contextOutside, true);
  window.addEventListener('blur', closeContextMenu);
}

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
