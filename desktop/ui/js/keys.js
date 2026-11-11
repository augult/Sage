// Which key sends a message, and which starts a new line in it. The choice is the window's own
// (js/prefs.js): `enter`, the default, where Enter sends and Ctrl/Cmd+Enter or Shift+Enter starts a
// new line; or `mod`, where Ctrl/Cmd+Enter sends and Enter starts a new line.

import * as prefs from './prefs.js';

export const MOD_KEY = navigator.platform.startsWith('Mac') ? 'Cmd' : 'Ctrl';

export const SEND_KEYS = [
  { value: 'enter', label: 'Enter' },
  { value: 'mod', label: `${MOD_KEY}+Enter` },
];

/** The choice: `enter` or `mod`. */
export const sendKey = () => (prefs.get('send') === 'mod' ? 'mod' : 'enter');

export const chooseSendKey = (value) => prefs.set('send', value === 'mod' ? 'mod' : 'enter');

/** What the other key does, to show beside a message box: the one a user would not guess. */
export const sendHint = () => (sendKey() === 'mod' ? `${MOD_KEY}+Enter to send` : `${MOD_KEY}+Enter for a new line`);

/** Puts a line break where the caret is, as typing one would: undoable, and announced as input. */
function breakLine(box) {
  if (document.activeElement === box && document.execCommand('insertText', false, '\n')) return;
  box.setRangeText('\n', box.selectionStart, box.selectionEnd, 'end');
  box.dispatchEvent(new Event('input', { bubbles: true }));
}

/**
 * The `keydown` of a message box: sends with the chosen key and starts a new line with the other.
 * Returns true when the key was one of the two and has been dealt with.
 */
export function onSendKey(event, send) {
  // Enter that confirms an input method's composition is not a message's end.
  if (event.key !== 'Enter' || event.isComposing) return false;
  const mod = event.ctrlKey || event.metaKey;
  const sends = sendKey() === 'mod' ? mod : !mod && !event.shiftKey;
  // Enter alone, or with Shift, is a line break the box makes itself.
  if (!sends && !mod) return false;
  event.preventDefault();
  if (sends) send();
  else breakLine(event.target);
  return true;
}
