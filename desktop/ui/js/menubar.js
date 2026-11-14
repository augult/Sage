// The menus of the title bar: File, Edit, View, Window and Help, with what this app has to put in
// them. The window has no system frame, so they are drawn here, and the keys that go with them are
// listened for here too.
//
// A menu is built when it is opened, from the state as it stands. A press on the bar leaves the
// focus, and what is selected, where they are: that is what Edit acts on. So the open menu has no
// focus of its own, and takes its keys from the document.
//
// Scripts reach a menu as `#menu-file` and an entry in it as `[data-command="new-session"]`.

import { api } from './api.js';
import { appWindow, maximized } from './chrome.js';
import { brandMark, h, icon } from './dom.js';
import { tilde } from './format.js';
import * as panes from './panes.js';
import * as themes from './themes.js';
import { settleUnsaved } from './unsaved.js';
import { MOD_KEY, closeButton, openDialog, sendKey, sendKeyLabel } from './views/common.js';
import { useFolder } from './views/new-session.js';

const MAC = MOD_KEY === 'Cmd';

// The keys this app answers to itself, as `mod+shift+b`: `mod` is Ctrl, or Cmd on a Mac.
const KEYS = {
  newSession: 'mod+n',
  openFolder: 'mod+o',
  settings: 'mod+,',
  sidebar: 'mod+b',
  inspector: 'mod+shift+b',
  // Alt with an arrow moves the caret by a word on a Mac.
  back: MAC ? 'mod+[' : 'alt+arrowleft',
  forward: MAC ? 'mod+]' : 'alt+arrowright',
};
// What the webview does with a text field by itself: named in the Edit menu, not listened for.
const EDIT_KEYS = { undo: 'mod+z', redo: MAC ? 'mod+shift+z' : 'mod+y', cut: 'mod+x', copy: 'mod+c', paste: 'mod+v', selectAll: 'mod+a' };

const KEY_NAMES = { mod: MOD_KEY, shift: 'Shift', alt: 'Alt', arrowleft: 'Left', arrowright: 'Right' };
const keyLabel = (keys) => keys.split('+').map((part) => KEY_NAMES[part] ?? part.toUpperCase()).join('+');

function pressed(event, keys) {
  const parts = keys.split('+');
  return event.key.toLowerCase() === parts.at(-1) && (event.ctrlKey || event.metaKey) === parts.includes('mod') && event.shiftKey === parts.includes('shift') && event.altKey === parts.includes('alt');
}

/** An entry of a menu. `more` is `{ keys, disabled, checked }`: one with `checked` is on or off. */
const item = (id, label, run, more = {}) => ({ id, label, run, ...more });
// Between two entries, a rule.
const RULE = null;

/** The text field the focus is in, if it can be typed into. */
function field() {
  const el = document.activeElement;
  return el?.matches('textarea, input:not([type="checkbox"], [type="radio"])') && !el.disabled && !el.readOnly ? el : null;
}

/** Puts the clipboard's text into `box`, where its caret is. */
async function paste(box) {
  if (document.execCommand('paste')) return;
  // A webview that keeps the command to itself may still let the clipboard be read.
  let text;
  try {
    text = await navigator.clipboard.readText();
  } catch {
    throw new Error(`This window does not let a menu paste: press ${keyLabel(EDIT_KEYS.paste)} instead.`);
  }
  box.focus();
  if (document.execCommand('insertText', false, text)) return;
  box.setRangeText(text, box.selectionStart, box.selectionEnd, 'end');
  box.dispatchEvent(new Event('input', { bubbles: true }));
}

const rows = (pairs) => h('dl.help-list', pairs.map(([key, what]) => [h('dt', key), h('dd', what)]));

function showCommands() {
  const other = sendKeyLabel(sendKey() === 'mod' ? 'enter' : 'mod');
  openDialog('help', (close) => [
    h('div.dialog-head', h('h2', 'Commands and keys'), closeButton(close)),
    h(
      'div.help-body',
      h('h3.help-title', 'At the start of a message'),
      rows([
        ['/', 'List the skills on offer'],
        ['/<skill>', 'Switch that skill on for the message'],
        ['/skills', 'Browse the skills, and read them'],
        ['/goal <condition>', 'Set what the work is held to until it holds'],
        ['/goal', 'Say what the goal is'],
        ['/goal clear', 'Drop the goal'],
      ]),
      h('h3.help-title', 'Keys'),
      rows([
        [sendKeyLabel(), 'Send the message'],
        [`${other} · Shift+Enter`, 'Break the line'],
        [keyLabel(KEYS.newSession), 'New session'],
        [keyLabel(KEYS.openFolder), 'Open a folder for a new session'],
        [keyLabel(KEYS.settings), 'Settings'],
        [keyLabel(KEYS.sidebar), 'Show or hide the sidebar'],
        [keyLabel(KEYS.inspector), 'Show or hide the inspector'],
        [`${keyLabel(KEYS.back)} · ${keyLabel(KEYS.forward)}`, 'Back and forward'],
      ]),
    ),
  ]);
}

function showAbout(state) {
  openDialog('about', (close) => [
    h('div.dialog-head', h('h2', 'About Sage'), closeButton(close)),
    h(
      'div.about-body',
      brandMark(40),
      h('div.about-name', 'Sage'),
      h('div.muted', `Version ${state.version}`),
      h('p.about-what', 'An expectation-driven agent harness.'),
      h('div.about-data', h('span.label', 'Data folder'), h('span.mono', tilde(state.data_dir, state.home))),
    ),
  ]);
}

/**
 * `bar` is the title bar's place for the menus. `nav` is the shell's way around:
 * `{ app, go(view), back(), forward(), canGoBack(), canGoForward() }`.
 */
export function mountMenuBar(bar, nav) {
  const { app } = nav;

  async function openFolder() {
    const picked = await api.pickFolder(null);
    if (!picked) return;
    useFolder(picked);
    await nav.go({ name: 'new' });
  }

  function setAppearance(name) {
    themes.choose({ appearance: name });
    // Settings, while it is the view, shows the same choice.
    const chip = document.querySelector(`#settings-appearance input[name="appearance"][value="${name}"]`);
    if (chip) chip.checked = true;
  }

  /** One of the inspector's views, with or without a session: the inspector comes back to show it. */
  function tab(id, label) {
    const button = document.getElementById(`tab-${id}`);
    const show = () => {
      panes.set('inspector', true);
      button.click();
    };
    return item(`tab-${id}`, label, show, { disabled: !button, checked: Boolean(button) && panes.shown('inspector') && document.documentElement.dataset.panel === id });
  }

  // What both a menu and a key can do.
  const acts = {
    newSession: () => nav.go({ name: 'new' }),
    openFolder,
    settings: () => nav.go({ name: 'settings' }),
    sidebar: () => panes.toggle('sidebar'),
    inspector: () => panes.toggle('inspector'),
    back: nav.back,
    forward: nav.forward,
  };

  const menus = [
    {
      id: 'file',
      label: 'File',
      items: () => [
        item('new-session', 'New session', acts.newSession, { keys: KEYS.newSession }),
        item('open-folder', 'Open folder…', acts.openFolder, { keys: KEYS.openFolder }),
        RULE,
        item('settings', 'Settings', acts.settings, { keys: KEYS.settings }),
        RULE,
        item('close-window', 'Close window', () => appWindow.close()),
      ],
    },
    {
      id: 'edit',
      label: 'Edit',
      items() {
        const box = field();
        const some = Boolean(box) && box.selectionStart !== box.selectionEnd;
        return [
          item('undo', 'Undo', () => document.execCommand('undo'), { keys: EDIT_KEYS.undo, disabled: !box }),
          item('redo', 'Redo', () => document.execCommand('redo'), { keys: EDIT_KEYS.redo, disabled: !box }),
          RULE,
          item('cut', 'Cut', () => document.execCommand('cut'), { keys: EDIT_KEYS.cut, disabled: !some }),
          // What is selected in the conversation can be copied as well as what is in a field.
          item('copy', 'Copy', () => document.execCommand('copy'), { keys: EDIT_KEYS.copy, disabled: !some && !String(getSelection()) }),
          item('paste', 'Paste', () => paste(box), { keys: EDIT_KEYS.paste, disabled: !box }),
          RULE,
          item('select-all', 'Select all', () => box.select(), { keys: EDIT_KEYS.selectAll, disabled: !box }),
        ];
      },
    },
    {
      id: 'view',
      label: 'View',
      items() {
        const look = themes.chosen().appearance;
        return [
          item('back', 'Back', acts.back, { keys: KEYS.back, disabled: !nav.canGoBack() }),
          item('forward', 'Forward', acts.forward, { keys: KEYS.forward, disabled: !nav.canGoForward() }),
          RULE,
          item('sidebar', 'Sidebar', acts.sidebar, { keys: KEYS.sidebar, checked: panes.shown('sidebar') }),
          item('inspector', 'Inspector', acts.inspector, { keys: KEYS.inspector, checked: panes.shown('inspector') }),
          RULE,
          tab('economy', 'Economy'),
          tab('code', 'Code'),
          tab('github', 'GitHub'),
          { group: 'Appearance' },
          ...themes.APPEARANCES.map((name) => item(`appearance-${name}`, name.charAt(0).toUpperCase() + name.slice(1), () => setAppearance(name), { checked: look === name })),
        ];
      },
    },
    {
      id: 'window',
      label: 'Window',
      items: () => [
        item('minimize', 'Minimize', () => appWindow.minimize()),
        item('maximize', maximized() ? 'Restore' : 'Maximize', () => appWindow.toggleMaximize()),
        // The sessions are what this window has open: the newest of them, to go to.
        ...(app.sessions.length > 0 ? [{ group: 'Sessions' }] : []),
        ...app.sessions.slice(0, 6).map((meta) =>
          item(`session-${meta.id}`, meta.title || 'Untitled session', () => nav.go({ name: 'session', id: meta.id }), { checked: app.view.name === 'session' && app.view.id === meta.id }),
        ),
      ],
    },
    {
      id: 'help',
      label: 'Help',
      items: () => [
        item('walkthrough', 'Play the walkthrough', async () => {
          if (await settleUnsaved()) await app.playDemo();
        }),
        item('commands', 'Commands and keys', showCommands),
        RULE,
        item('about', 'About Sage', () => showAbout(app.state)),
      ],
    },
  ];

  // The menu that is open: which one, its element, and the row the arrow keys are on.
  let open = null;

  const tops = menus.map((menu, at) =>
    h(
      'button.menu-top',
      {
        id: `menu-${menu.id}`,
        type: 'button',
        'aria-haspopup': 'menu',
        'aria-expanded': 'false',
        onclick: () => (open?.at === at ? close() : show(at)),
        // With one open, the pointer opens the others as it passes.
        onmouseenter: () => {
          if (open && open.at !== at) show(at);
        },
        onkeydown: (event) => {
          if (event.key === 'ArrowDown' && !open) show(at);
        },
      },
      menu.label,
    ),
  );
  bar.append(...tops.map((top) => h('div.menu-wrap', top)));
  bar.addEventListener('mousedown', (event) => event.preventDefault());

  const entries = () => [...open.menu.querySelectorAll('.menu-item')];
  const mark = () => entries().forEach((row, at) => row.classList.toggle('is-active', at === open.active));

  const row = (entry) =>
    h(
      'button.menu-item',
      {
        type: 'button',
        role: entry.checked == null ? 'menuitem' : 'menuitemcheckbox',
        'aria-checked': entry.checked == null ? null : String(entry.checked),
        tabindex: '-1',
        disabled: entry.disabled,
        dataset: { command: entry.id },
        onmousemove: (event) => {
          const at = entries().indexOf(event.currentTarget);
          if (at === open.active) return;
          open.active = at;
          mark();
        },
        onclick: () => {
          close();
          entry.run();
        },
      },
      entry.checked != null && icon('check', 12),
      h('span.menu-text', entry.label),
      entry.keys && h('span.menu-key', keyLabel(entry.keys)),
    );

  function show(at) {
    close();
    const menu = h('div.menu.bar-menu', { role: 'menu', 'aria-labelledby': tops[at].id });
    for (const entry of menus[at].items()) menu.append(entry === RULE ? h('div.menu-rule') : entry.group ? h('div.menu-group', entry.group) : row(entry));
    tops[at].after(menu);
    tops[at].setAttribute('aria-expanded', 'true');
    open = { at, menu, active: -1 };
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('keydown', onKey, true);
  }

  function close() {
    if (!open) return;
    open.menu.remove();
    tops[open.at].setAttribute('aria-expanded', 'false');
    document.removeEventListener('pointerdown', outside, true);
    document.removeEventListener('keydown', onKey, true);
    open = null;
  }

  function outside(event) {
    if (!bar.contains(event.target)) close();
  }

  /** The keys of the open menu, taken before whatever has the focus sees them. */
  function onKey(event) {
    const all = entries();
    const live = all.filter((each) => !each.disabled);
    const down = { ArrowDown: 1, ArrowUp: -1 }[event.key];
    const along = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
    if (down && live.length) {
      const from = live.indexOf(all[open.active]);
      const to = from < 0 ? (down > 0 ? 0 : live.length - 1) : (from + down + live.length) % live.length;
      open.active = all.indexOf(live[to]);
      mark();
    } else if (along) {
      show((open.at + along + menus.length) % menus.length);
    } else if (event.key === 'Enter' || event.key === ' ') {
      all[open.active]?.click();
    } else if (event.key === 'Escape') {
      close();
    } else if (!down) {
      // Anything else is for what has the focus: a key held with another leaves the menu open.
      if (!['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) close();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
  }

  const bound = Object.entries(KEYS).map(([name, keys]) => [keys, acts[name]]);
  document.addEventListener('keydown', (event) => {
    // A dialog keeps the keys to itself.
    if (event.defaultPrevented || document.querySelector('dialog[open]')) return;
    const hit = bound.find(([keys]) => pressed(event, keys));
    if (!hit) return;
    event.preventDefault();
    hit[1]();
  });
}
