// The New session view: one composer. The workspace sits above the task; the model, skills, effort,
// mode and autonomy are pickers in the bar below it, next to Start.
// With no key configured the view opens with the walkthrough and the way to add a key.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { tilde } from '../format.js';
import { MOD_KEY } from './common.js';
import { effortOptions, modelOptions, picker, skillsPicker } from './controls.js';
import { pickFolder } from './folder-picker.js';

const MODES = [
  { value: 'presage', label: 'Presage' },
  { value: 'classic', label: 'Classic' },
];
const AUTONOMIES = [
  { value: 'auto', label: 'Auto' },
  { value: 'ask', label: 'Ask' },
];

// What has been entered so far. It survives a trip to Settings and back; the skills start empty
// for every new session.
const draft = { cwd: null, provider: null, model: null, effort: null, mode: 'presage', autonomy: 'auto', task: '', skills: [] };

/** Workspaces of earlier real sessions, most recent first. */
function recentWorkspaces(sessions) {
  return [...new Set(sessions.filter((meta) => !meta.simulated).map((meta) => meta.cwd))].slice(0, 4);
}

export function mountNewSession(main, app) {
  const { state } = app;
  const home = state.home;
  const keyed = state.providers.filter((provider) => provider.has_key);
  const recent = recentWorkspaces(app.sessions);

  draft.cwd ??= recent[0] ?? '';
  draft.effort ??= state.effort;
  if (!keyed.some((provider) => provider.id === draft.provider)) {
    draft.provider = (keyed.find((provider) => provider.id === state.provider) ?? keyed[0])?.id ?? null;
    draft.model = null;
  }
  const chosen = state.providers.find((provider) => provider.id === draft.provider);
  if (chosen) {
    const models = modelOptions(chosen, state).map((option) => option.value);
    if (!models.includes(draft.model)) draft.model = chosen.id === state.provider && state.model ? state.model : models[0] ?? null;
  }

  const error = h('div.form-error', { role: 'alert', hidden: true });
  const fail = (problem) => {
    error.textContent = String(problem);
    error.hidden = false;
  };

  /** Runs `action` with `button` disabled, and shows its failure in the form. */
  async function guarded(button, action) {
    button.disabled = true;
    error.hidden = true;
    try {
      await action();
    } catch (problem) {
      fail(problem);
    }
    button.disabled = false;
  }

  const playDemo = (event) => guarded(event.currentTarget, () => app.playDemo());
  const openSettings = () => app.show({ name: 'settings' });

  // Workspace
  const workspace = h('input', {
    id: 'workspace',
    type: 'text',
    value: draft.cwd,
    placeholder: 'Folder',
    'aria-label': 'Workspace',
    spellcheck: 'false',
    autocomplete: 'off',
    oninput: () => {
      draft.cwd = workspace.value;
      clearTimeout(skillsTimer);
      skillsTimer = setTimeout(loadSkills, 300);
    },
  });
  const setWorkspace = (path) => {
    workspace.value = path;
    draft.cwd = path;
    loadSkills();
  };
  const browse = h(
    'button.btn.quiet',
    {
      id: 'browse-workspace',
      type: 'button',
      onclick: async () => {
        const picked = await pickFolder(draft.cwd.trim());
        if (picked) setWorkspace(picked);
      },
    },
    'Browse…',
  );
  const recentRow =
    recent.length > 0 &&
    h(
      'div.recent',
      'Recent',
      recent.map((path) => h('button', { type: 'button', title: path, onclick: () => setWorkspace(path) }, tilde(path, home))),
    );

  // Provider and model are one choice: a model, listed under the provider that serves it.
  const served = new Map();
  const modelGroups = state.providers.map((provider) => {
    const models = modelOptions(provider, state);
    // A provider that lists no models still has its own default.
    const options = (models.length ? models : [{ value: null, label: 'Provider default' }]).map((option) => {
      const value = `${provider.id}:${option.value ?? ''}`;
      served.set(value, { provider: provider.id, model: option.value });
      return { value, label: option.label, disabled: !provider.has_key };
    });
    return { label: provider.has_key ? provider.name : `${provider.name} · needs key`, options };
  });
  const modelPicker = picker({
    id: 'pick-model',
    name: 'Model',
    groups: modelGroups,
    value: draft.provider ? `${draft.provider}:${draft.model ?? ''}` : null,
    empty: 'No model yet',
    onChange: (value) => Object.assign(draft, served.get(value)),
  });

  /** A run setting: its name, the current value, and a menu of the others. */
  const setting = (key, name, options, changed) =>
    picker({
      id: `pick-${key}`,
      name,
      prefix: name,
      groups: [{ options }],
      value: draft[key],
      onChange: (value) => {
        draft[key] = value;
        changed?.();
      },
    });

  // Skills sit next to the model. The list follows the folder: a project brings its own.
  const skillsSlot = h('span.pick-slot');
  let skillList = [];
  let skillsTimer = 0;
  let skillsAsked = 0;
  const workspacePath = () => draft.cwd.trim().replace(/^~(?=\/|$)/, home);
  function drawSkills() {
    draft.skills = draft.skills.filter((name) => skillList.some((skill) => skill.name === name));
    // The classic baseline takes no skills.
    skillsSlot.replaceChildren(skillsPicker({ skills: skillList, value: draft.skills, disabled: draft.mode === 'classic', onChange: (names) => (draft.skills = names) }));
  }
  async function loadSkills() {
    clearTimeout(skillsTimer);
    const asked = ++skillsAsked;
    const listed = await api.skills(workspacePath() || null);
    // A slower answer for an earlier folder must not replace a later one.
    if (asked !== skillsAsked) return;
    skillList = listed;
    drawSkills();
  }
  drawSkills();
  loadSkills();

  const task = h('textarea', {
    id: 'task',
    rows: 5,
    value: draft.task,
    placeholder: 'Task',
    'aria-label': 'Task',
    oninput: () => (draft.task = task.value),
  });

  // With no key there is nothing to start: the walkthrough is the view's one primary action.
  const startButton = h(`button.btn${keyed.length ? '.primary' : ''}`, { id: 'start', type: 'button', onclick: start }, 'Start session');

  function start() {
    const cwd = workspacePath();
    if (!draft.provider) return fail('No API key.');
    if (!cwd) return fail('No folder.');
    if (!draft.task.trim()) return fail('No task.');
    return guarded(startButton, async () => {
      const { provider, model, effort, mode, autonomy } = draft;
      const meta = await api.startSession({ cwd, task: draft.task, provider, model, effort, mode, autonomy, skills: mode === 'classic' ? [] : draft.skills });
      draft.task = '';
      draft.skills = [];
      await app.opened(meta);
    });
  }

  const form = h(
    'div.new-session',
    {
      onkeydown: (event) => {
        if (event.key !== 'Enter' || !(event.ctrlKey || event.metaKey)) return;
        event.preventDefault();
        start();
      },
    },
    h(
      'section.composer-card',
      h('div.composer-top', h('label.workspace-pick', icon('folder', 14), workspace), browse),
      task,
      h(
        'div.composer-bar',
        modelPicker,
        skillsSlot,
        setting('effort', 'Effort', effortOptions(state.effort)),
        setting('mode', 'Mode', MODES, drawSkills),
        setting('autonomy', 'Autonomy', AUTONOMIES),
        startButton,
      ),
    ),
    h('div.composer-foot', recentRow, h('span.kbd', `${MOD_KEY}+Enter`)),
    error,
  );

  const hero =
    keyed.length === 0 &&
    h(
      'div.lead-actions',
      h('button.btn.primary', { id: 'play-demo', type: 'button', onclick: playDemo }, icon('play', 12), 'Play the walkthrough'),
      h('button.btn.quiet', { id: 'add-key', type: 'button', onclick: openSettings }, icon('key', 14), 'Add an API key'),
    );

  main.append(
    h(
      'header.view-head',
      { 'data-tauri-drag-region': 'deep' },
      h('h1', 'New session'),
      keyed.length > 0 && h('button.btn.quiet', { id: 'play-demo', type: 'button', onclick: playDemo }, icon('play', 12), 'Play the walkthrough'),
    ),
    h('div.view-body', h('div.view-column', hero, form)),
  );
  // Start where the first thing is still missing.
  (keyed.length === 0 ? main.querySelector('#play-demo') : draft.cwd ? task : workspace).focus();
  return {
    unmount() {
      clearTimeout(skillsTimer);
      skillsAsked++;
    },
  };
}
