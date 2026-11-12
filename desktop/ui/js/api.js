// The Tauri commands, one function each. Every call rejects with the engine's message as a string.

const { invoke, Channel } = window.__TAURI__.core;

export const api = {
  hooks: () => invoke('ui_hooks'),
  appState: () => invoke('app_state'),
  /** An empty key removes the stored one. */
  setKey: (provider, key) => invoke('set_key', { provider, key }),
  /** Fields left out keep their current value. */
  setDefaults: ({ provider, model, effort }) => invoke('set_defaults', { provider, model, effort }),
  /** Keep retrying a rate-limited or unreachable provider, in running sessions too. */
  setAutoRetry: (on) => invoke('set_auto_retry', { on }),
  listSessions: () => invoke('list_sessions'),
  startSession: (req) => invoke('start_session', { req }),
  startDemo: () => invoke('start_demo'),
  /** Test hook: the session `PRESAGE_UI_SCRIPTED` describes. */
  startScripted: () => invoke('start_scripted'),
  /**
   * `skills` are switched on with this message; the ones the session already has are ignored.
   * `changes` is what else is the session's from this message on: `{ provider, model, effort, autonomy }`, each optional.
   */
  sendMessage: (id, text, skills = [], changes = {}) => invoke('send_message', { id, text, skills, changes }),
  stopSession: (id) => invoke('stop_session', { id }),
  approve: (id, approval, allow) => invoke('approve', { id, approval, allow }),
  sessionEvents: (id) => invoke('session_events', { id }),
  readOutput: (id, handle) => invoke('read_output', { id, handle }),
  /** The folder chosen in the system's dialog, opened at `start`; null when it was dismissed. */
  pickFolder: (start) => invoke('pick_folder', { start }),
  /** Built-in and user skills, plus the project's own when `cwd` is given. */
  skills: (cwd) => invoke('list_skills', { cwd }),
  /** A skill's whole file, front matter included: what `saveSkill` takes. */
  skillFile: (name, cwd) => invoke('skill_file', { name, cwd }),
  saveSkill: (name, content) => invoke('save_skill', { name, content }),
  /** Deletes a user skill; one that replaced a built-in gives the built-in back. */
  deleteSkill: (name) => invoke('delete_skill', { name }),
  /** `{ system, system_default, system_custom, global }`. */
  instructions: () => invoke('instructions'),
  /** Null or empty goes back to the built-in template. */
  setSystemPrompt: (template) => invoke('set_system_prompt', { template }),
  /** Empty deletes the file. */
  setGlobalInstructions: (text) => invoke('set_global_instructions', { text }),
  /** Where a session's workspace stands with git: `{ repo, branch, remote, url, github, upstream, ahead, files }`. */
  gitState: (id) => invoke('git_state', { id }),
  /** Commits what is uncommitted under `message` and pushes the branch: `{ commit, remote, branch, pushed }`. */
  gitPush: (id, message) => invoke('git_push', { id, message }),

  /** Resolves with the stored events; `onEvent` receives everything emitted after them. */
  subscribe(id, onEvent) {
    const channel = new Channel();
    channel.onmessage = onEvent;
    return invoke('subscribe', { id, onEvent: channel });
  },
};
