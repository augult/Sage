// App shell: loads the engine state, owns navigation between the three views, keeps the sidebar current.

import { api } from './api.js';
import { startBackdrop } from './backdrop.js';
import { mountWindowControls } from './chrome.js';
import { h, icon } from './dom.js';
import { applyHooks } from './hooks.js';
import { mountPaneToggles } from './panes.js';
import * as store from './store.js';
import { settleUnsaved } from './unsaved.js';
import { mountNewSession } from './views/new-session.js';
import { mountSession } from './views/session.js';
import { mountSettings } from './views/settings.js';
import { listSignature, renderSessionList } from './views/sidebar.js';

const main = document.getElementById('main');
const side = document.getElementById('inspector-body');
const list = document.getElementById('session-list');
const newLink = document.getElementById('new-session');
const settingsLink = document.getElementById('open-settings');

// The title bar and the glass belong to the window, not to the engine: neither waits for it.
mountWindowControls();
mountPaneToggles();
startBackdrop();
newLink.prepend(icon('plus'));
settingsLink.prepend(icon('sliders'));

let mounted = null;
let listShown = null;

const markCurrent = (link, current) => (current ? link.setAttribute('aria-current', 'page') : link.removeAttribute('aria-current'));

const app = {
  /** The engine's `AppState`: providers, defaults, paths. */
  state: null,
  /** Session listing, newest first. */
  sessions: [],
  view: { name: 'new' },

  /** Switches the main column to `{ name: 'new' | 'settings' }` or `{ name: 'session', id }`. */
  show(view) {
    mounted?.unmount?.();
    app.view = view;
    main.replaceChildren();
    side.replaceChildren();
    const meta = view.name === 'session' && app.sessions.find((session) => session.id === view.id);
    if (meta) {
      mounted = mountSession(main, side, app, meta);
    } else if (view.name === 'session') {
      main.append(h('div.view-message', h('p', `There is no session ${view.id}.`)));
      mounted = null;
    } else {
      mounted = view.name === 'settings' ? mountSettings(main, app) : mountNewSession(main, app);
    }
    markCurrent(newLink, view.name === 'new');
    markCurrent(settingsLink, view.name === 'settings');
    drawSessionList();
  },

  async reloadState() {
    app.state = await api.appState();
  },

  /** Shows a session that was just started. */
  async opened(meta) {
    // Subscribe before anything else: text streamed before the subscription cannot be recovered.
    store.open(meta);
    app.sessions = await api.listSessions();
    app.show({ name: 'session', id: meta.id });
  },

  async playDemo() {
    await app.opened(await api.startDemo());
  },
};

/** Goes where the user asked to, once what they typed in Settings is saved or let go. */
async function go(view) {
  if (await settleUnsaved()) app.show(view);
}

function drawSessionList() {
  const currentId = app.view.name === 'session' ? app.view.id : null;
  const signature = listSignature(app.sessions, currentId);
  if (signature === listShown) return;
  listShown = signature;
  renderSessionList(list, app.sessions, currentId, app.state.home, (id) => go({ name: 'session', id }));
}

[app.state, app.sessions] = await Promise.all([api.appState(), api.listSessions()]);
newLink.addEventListener('click', () => go({ name: 'new' }));
settingsLink.addEventListener('click', () => go({ name: 'settings' }));
// A session changing state changes its mark.
let listFrame = 0;
store.onChange(() => (listFrame ||= requestAnimationFrame(() => ((listFrame = 0), drawSessionList()))));
await applyHooks(app);
