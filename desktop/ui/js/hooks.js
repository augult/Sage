// Headless test hooks: the PRESAGE_UI_* environment variables, as read by the shell.
//
//   PRESAGE_UI_AUTODEMO=1                   start the walkthrough and open it
//   PRESAGE_UI_SCRIPTED=<file>              start the scripted session the file describes and open it
//   PRESAGE_UI_OPEN=<session id>            open that session
//   PRESAGE_UI_VIEW=settings|new|session    open that view (`session`: the most recent one)
//   PRESAGE_UI_SCRIPT=<actions>             then play UI actions, separated by `;;`:
//       click:<selector>          type:<selector>=><text>     key:<selector>=>Enter (or ctrl+Enter)
//       scroll:<selector>         wait:<milliseconds>         attach:#attach=><path>[,<path>]
//     An action waits for its element to exist, so scripts need no timing of their own.
//   PRESAGE_UI_SIZE=<width>x<height>        window size (applied by the shell)
//   PRESAGE_UI_THEME=<background>           the painting behind the glass for this launch, by id:
//                                           ember, orchid, tide, graphite or none
//   PRESAGE_UI_SCHEME=light|dark            the appearance for this launch

import { api } from './api.js';
import * as themes from './themes.js';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const ACTIONS = {
  click: (el) => el.click(),
  type(el, text) {
    el.value = text;
    el.dispatchEvent(new Event('input', { bubbles: true }));
  },
  key(el, combo) {
    const keys = combo.split('+');
    el.dispatchEvent(new KeyboardEvent('keydown', { key: keys.at(-1), ctrlKey: keys.includes('ctrl'), metaKey: keys.includes('meta'), bubbles: true, cancelable: true }));
  },
  scroll: (el) => el.scrollIntoView({ block: 'start' }),
  attach: (el, paths) => el.dispatchEvent(new CustomEvent('attach', { detail: paths.split(',') })),
};

async function element(selector) {
  for (let waited = 0; waited < 15_000; waited += 50) {
    const el = document.querySelector(selector);
    if (el) return el;
    await sleep(50);
  }
  throw new Error(`UI script: nothing matches ${selector}`);
}

async function play(script) {
  for (const step of script.split(';;')) {
    const [, action, rest = ''] = /^\s*(\w+):(.*)$/s.exec(step) ?? [];
    const arrow = rest.indexOf('=>');
    const [target, argument] = arrow < 0 ? [rest, ''] : [rest.slice(0, arrow), rest.slice(arrow + 2)];
    if (action === 'wait') await sleep(Number(target));
    else if (Object.hasOwn(ACTIONS, action)) ACTIONS[action](await element(target.trim()), argument);
    else throw new Error(`UI script: cannot read "${step.trim()}"`);
  }
}

/** Opens the first view, as the hooks ask or the New session view by default, then plays the script. */
export async function applyHooks(app) {
  const hooks = await api.hooks();
  themes.force({ background: hooks.theme, appearance: hooks.scheme });
  if (hooks.autodemo) await app.playDemo();
  else if (hooks.scripted) await app.opened(await api.startScripted());
  else if (hooks.open) app.show({ name: 'session', id: hooks.open });
  else if (hooks.view === 'session' && app.sessions.length) app.show({ name: 'session', id: app.sessions[0].id });
  else app.show({ name: hooks.view === 'settings' ? 'settings' : 'new' });
  if (hooks.script) await play(hooks.script);
}
