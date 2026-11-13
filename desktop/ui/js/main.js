// App shell: loads the engine state, owns navigation between the three views, keeps the sidebar current.

import { api } from './api.js';
import { startBackdrop } from './backdrop.js';
import { mountWindowControls } from './chrome.js';
import { h, icon } from './dom.js';
import { applyHooks } from './hooks.js';
import { mountMenuBar } from './menubar.js';
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
const backLink = document.getElementById('nav-back');
const forwardLink = document.getElementById('nav-forward');

// The title bar and the glass belong to the window, not to the engine: neither waits for it.
mountWindowControls();
mountPaneToggles();
startBackdrop();
newLink.prepend(icon('plus'));
settingsLink.prepend(icon('sliders'));
backLink.append(icon('back'));
forwardLink.append(icon('chevron'));

let mounted = null;
let listShown = null;

// The views gone through, oldest first, and which of them is shown: what the two arrows of the
// title bar walk.
const trail = [];
let at = -1;
const sameView = (a, b) => a.name === b.name && a.id === b.id;

const markCurrent = (link, current) => (current ? link.setAttribute('aria-current', 'page') : link.removeAttribute('aria-current'));

const app = {
  /** The engine's `AppState`: providers, defaults, paths. */
  state: null,
  /** Session listing, newest first. */
  sessions: [],
  view: { name: 'new' },

  /**
   * Switches the main column to `{ name: 'new' | 'settings' }` or `{ name: 'session', id }`. A view
   * gone to joins the trail, ahead of which nothing is left; `visit` is false for a step along it.
   */
  show(view, visit = true) {
    mounted?.unmount?.();
    app.view = view;
    if (visit && !(at >= 0 && sameView(trail[at], view))) {
      trail.splice(at + 1);
      at = trail.push(view) - 1;
    }
    backLink.disabled = at <= 0;
    forwardLink.disabled = at >= trail.length - 1;
    // For the stylesheets: a new session opens on the bare glass (styles/glass.css).
    document.documentElement.dataset.view = view.name;
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

/** One view back along the trail (`by` -1) or forward again (1). */
async function step(by) {
  const to = trail[at + by];
  if (!to || !(await settleUnsaved())) return;
  at += by;
  app.show(to, false);
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
backLink.addEventListener('click', () => step(-1));
forwardLink.addEventListener('click', () => step(1));
mountMenuBar(document.getElementById('menubar'), { app, go, back: () => step(-1), forward: () => step(1), canGoBack: () => at > 0, canGoForward: () => at < trail.length - 1 });
// A session changing state changes its mark.
let listFrame = 0;
store.onChange(() => (listFrame ||= requestAnimationFrame(() => ((listFrame = 0), drawSessionList()))));
await applyHooks(app);
