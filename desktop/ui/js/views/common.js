// Small pieces shared by the views.

import { h, icon, spinner } from '../dom.js';
import { pendingApprovals } from '../model.js';

export const MOD_KEY = navigator.platform.startsWith('Mac') ? 'Cmd' : 'Ctrl';

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

/** What a session is doing now: from its view-model once it is loaded, else from the listing. */
export function sessionState(meta, entry) {
  const model = entry?.model;
  if (!model) return { phase: meta.status, outcome: meta.outcome ?? null, verified: meta.verified === true, approvals: 0 };
  const phase = !entry.live ? 'closed' : model.running ? 'running' : 'idle';
  return { phase, outcome: model.outcome?.status ?? null, verified: model.outcome?.verified === true, approvals: phase === 'running' ? pendingApprovals(model) : 0 };
}

/** The status glyph for a session, with its meaning as a tooltip and accessible name. */
export function stateGlyph(state) {
  if (state.approvals > 0) return glyph(icon('bang'), 'alert', 'Waiting for your approval');
  if (state.phase === 'running') return glyph(spinner(), 'quiet', 'Running');
  const look = outcomeLook(state.outcome, state.verified);
  return glyph(icon(look.icon), look.tone, look.label);
}

function glyph(mark, tone, label) {
  return h(`span.glyph.tone-${tone}`, { title: label, role: 'img', 'aria-label': label }, mark);
}

export const tag = (text, tone = 'plain', title) => h(`span.tag.tone-${tone}`, { title }, text);

/** Looks up a value the engine named. Anything this build does not know comes back undefined. */
export const known = (table, name) => (Object.hasOwn(table, name) ? table[name] : undefined);

/** A modal dialog that removes itself when closed. `build(close)` returns its children. */
export function openDialog(className, build) {
  const dialog = h(`dialog.${className}`);
  dialog.append(...build(() => dialog.close()));
  dialog.addEventListener('close', () => dialog.remove());
  document.body.append(dialog);
  dialog.showModal();
  return dialog;
}
