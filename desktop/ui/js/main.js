// App shell: loads the engine state, owns navigation between the three views, keeps the sidebar current.

import { api } from './api.js';
import { h } from './dom.js';
import { applyHooks } from './hooks.js';
import * as store from './store.js';
import { renderLegend } from './views/inspector.js';
import { mountNewSession } from './views/new-session.js';
import { mountSession } from './views/session.js';
import { mountSettings } from './views/settings.js';
import { listSignature, renderSessionList } from './views/sidebar.js';

const main = document.getElementById('main');
const side = document.getElementById('inspector');
const list = document.getElementById('session-list');
const settingsLink = document.getElementById('open-settings');

let mounted = null;
let listShown = null;

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
    } else {
      renderLegend(side);
      if (view.name === 'session') {
        main.append(h('div.view-message', h('p', `There is no session ${view.id}.`)));
        mounted = null;
      } else {
        mounted = view.name === 'settings' ? mountSettings(main, app) : mountNewSession(main, app);
      }
    }
    if (view.name === 'settings') settingsLink.setAttribute('aria-current', 'page');
    else settingsLink.removeAttribute('aria-current');
    drawSessionList();
  },

  async reloadState() {
    app.state = await api.appState();
  },

  /** Shows a session that was just started. */
  async opened(meta) {
    app.sessions = await api.listSessions();
    app.show({ name: 'session', id: meta.id });
  },

  async playDemo() {
    await app.opened(await api.startDemo());
  },
};

function drawSessionList() {
  const currentId = app.view.name === 'session' ? app.view.id : null;
  const signature = listSignature(app.sessions, currentId);
  if (signature === listShown) return;
  listShown = signature;
  renderSessionList(list, app.sessions, currentId, (id) => app.show({ name: 'session', id }));
}

[app.state, app.sessions] = await Promise.all([api.appState(), api.listSessions()]);
document.getElementById('app-version').textContent = `v${app.state.version}`;
document.getElementById('new-session').addEventListener('click', () => app.show({ name: 'new' }));
settingsLink.addEventListener('click', () => app.show({ name: 'settings' }));
// A session changing state changes its glyph; the timer keeps the relative times moving.
let listFrame = 0;
store.onChange(() => (listFrame ||= requestAnimationFrame(() => ((listFrame = 0), drawSessionList()))));
setInterval(drawSessionList, 30_000);
await applyHooks(app);
