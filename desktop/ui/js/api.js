// The Tauri commands, one function each. Every call rejects with the engine's message as a string.

const { invoke, Channel } = window.__TAURI__.core;

export const api = {
  hooks: () => invoke('ui_hooks'),
  appState: () => invoke('app_state'),
  /** An empty key removes the stored one. */
  setKey: (provider, key) => invoke('set_key', { provider, key }),
  /** An OpenAI-compatible endpoint of the user's own; `key` may be empty. Resolves with the id it goes by. */
  addProvider: ({ name, baseUrl, models, key }) => invoke('add_provider', { name, baseUrl, models, key }),
  /** Takes one of the user's own providers away, with its key. */
  removeProvider: (id) => invoke('remove_provider', { id }),
  /** Fields left out keep their current value. */
  setDefaults: ({ provider, model, effort }) => invoke('set_defaults', { provider, model, effort }),
  /** Keep retrying a rate-limited or unreachable provider, in running sessions too. */
  setAutoRetry: (on) => invoke('set_auto_retry', { on }),
  listSessions: () => invoke('list_sessions'),
  startSession: (req) => invoke('start_session', { req }),
  startDemo: () => invoke('start_demo'),
  /** Test hook: the session `SAGE_UI_SCRIPTED` describes. */
  startScripted: () => invoke('start_scripted'),
  /**
   * `skills` are switched on with this message; the ones the session already has are ignored. `files` are attached to it, by path.
   * `changes` is what else is the session's from this message on: `{ provider, model, effort, autonomy }`, each optional.
   */
  sendMessage: (id, text, skills = [], changes = {}, files = []) => invoke('send_message', { id, text, skills, changes, files }),
  stopSession: (id) => invoke('stop_session', { id }),
  /** Another title for a session. Resolves with the session as it then stands. */
  renameSession: (id, title) => invoke('rename_session', { id, title }),
  /** Deletes a session's record for good; its folder is not touched. Refused while the session runs. */
  deleteSession: (id) => invoke('delete_session', { id }),
  approve: (id, approval, allow) => invoke('approve', { id, approval, allow }),
  sessionEvents: (id, after) => invoke('session_events', { id, after }),
  readOutput: (id, handle) => invoke('read_output', { id, handle }),
  /** The folder chosen in the system's dialog, opened at `start`; null when it was dismissed. */
  pickFolder: (start) => invoke('pick_folder', { start }),
  /** The files chosen in the system's dialog, opened at `start`; none when it was dismissed. */
  pickFiles: (start) => invoke('pick_files', { start }),
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
  /** The session's current Git state and its preview revision. */
  gitState: (id) => invoke('git_state', { id }),
  /** Commits the reviewed workspace changes and pushes the branch. */
  gitPush: (id, message, revision) => invoke('git_push', { id, message, revision }),
  /** What a directory of the session's folder holds: `[{ name, path, dir }]`. Paths are from the folder's root. */
  listDir: (id, path = '') => invoke('list_dir', { id, path }),
  readFile: (id, path) => invoke('read_file', { id, path }),
  writeFile: (id, path, text, expected) => invoke('write_file', { id, path, text, expected }),
  /** An empty file, or a folder when `dir`, at `path`; nothing may be there yet. */
  createEntry: (id, path, dir) => invoke('create_entry', { id, path, dir }),
  renameEntry: (id, from, to) => invoke('rename_entry', { id, from, to }),
  /** Deletes a file, or a folder with everything in it. */
  deleteEntry: (id, path) => invoke('delete_entry', { id, path }),
  /** Puts a changed file back as the last commit has it. */
  gitDiscard: (id, path) => invoke('git_discard', { id, path }),

  /** Resolves with the stored events; `onEvent` receives everything emitted after them. */
  subscribe(id, onEvent) {
    const channel = new Channel();
    channel.onmessage = onEvent;
    return invoke('subscribe', { id, onEvent: channel });
  },
};
