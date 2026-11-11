// The Tauri commands, one function each. Every call rejects with the engine's message as a string.

const { invoke, Channel } = window.__TAURI__.core;

export const api = {
  hooks: () => invoke('ui_hooks'),
  appState: () => invoke('app_state'),
  /** An empty key removes the stored one. */
  setKey: (provider, key) => invoke('set_key', { provider, key }),
  /** Fields left out keep their current value. */
  setDefaults: ({ provider, model, effort }) => invoke('set_defaults', { provider, model, effort }),
  listSessions: () => invoke('list_sessions'),
  startSession: (req) => invoke('start_session', { req }),
  startDemo: () => invoke('start_demo'),
  /** Test hook: the session `PRESAGE_UI_SCRIPTED` describes. */
  startScripted: () => invoke('start_scripted'),
  sendMessage: (id, text) => invoke('send_message', { id, text }),
  stopSession: (id) => invoke('stop_session', { id }),
  approve: (id, approval, allow) => invoke('approve', { id, approval, allow }),
  sessionEvents: (id) => invoke('session_events', { id }),
  readOutput: (id, handle) => invoke('read_output', { id, handle }),
  listDir: (path) => invoke('list_dir', { path }),

  /** Resolves with the stored events; `onEvent` receives everything emitted after them. */
  subscribe(id, onEvent) {
    const channel = new Channel();
    channel.onmessage = onEvent;
    return invoke('subscribe', { id, onEvent: channel });
  },
};
