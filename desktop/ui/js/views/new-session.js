// The New session view: one composer. The workspace sits above the task; the model, effort, mode
// and autonomy are pickers in the bar below it, next to Start.
// With no key configured the view opens with the walkthrough and the way to add a key.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { tilde } from '../format.js';
import { MOD_KEY } from './common.js';
import { effortOptions, modelOptions, picker } from './controls.js';
import { pickFolder } from './folder-picker.js';

const EFFORT_HINTS = {
  ratchet: 'Starts at high and only rises, after repeated surprises.',
  low: 'The least deliberation per reply.',
  high: 'Stays at high.',
  max: 'The most deliberation per reply: slowest and most expensive.',
};
const MODES = [
  { value: 'presage', label: 'Presage', hint: 'Replies run as scripts, with predictions, watches and the evidence gate.' },
  { value: 'classic', label: 'Classic', hint: 'A conventional tool loop with the same model and tools, as a baseline.' },
];
const AUTONOMIES = [
  { value: 'auto', label: 'Auto', hint: 'Runs every step. Only catastrophic commands wait for you.' },
  { value: 'ask', label: 'Ask', hint: 'Every step that changes something waits for your approval.' },
];

// What has been entered so far. It survives a trip to Settings and back.
const draft = { cwd: null, provider: null, model: null, effort: null, mode: 'presage', autonomy: 'auto', task: '' };

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
    placeholder: 'The folder the agent works in',
    'aria-label': 'Workspace',
    spellcheck: 'false',
    autocomplete: 'off',
    oninput: () => (draft.cwd = workspace.value),
  });
  const setWorkspace = (path) => {
    workspace.value = path;
    draft.cwd = path;
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
    return {
      label: provider.has_key ? provider.name : `${provider.name} · needs key`,
      note: provider.has_key ? provider.note : `Add a key in Settings, or set ${provider.key_env}.`,
      options,
    };
  });
  const modelPicker = picker({
    id: 'pick-model',
    name: 'Model',
    groups: modelGroups,
    value: draft.provider ? `${draft.provider}:${draft.model ?? ''}` : null,
    empty: 'No model yet',
    onChange: (value) => Object.assign(draft, served.get(value)),
  });

  /** A run setting: its name, the current value, and a menu that explains each option. */
  const setting = (key, name, options) => picker({ id: `pick-${key}`, name, prefix: name, groups: [{ options }], value: draft[key], onChange: (value) => (draft[key] = value) });
  const efforts = effortOptions(state.effort).map((option) => ({ ...option, hint: EFFORT_HINTS[option.value] }));

  const task = h('textarea', {
    id: 'task',
    rows: 5,
    value: draft.task,
    placeholder: 'What should be done? If you know how to check it, say so: “… so that `make test` passes”.',
    'aria-label': 'Task',
    oninput: () => (draft.task = task.value),
  });

  // With no key there is nothing to start: the walkthrough is the view's one primary action.
  const startButton = h(`button.btn${keyed.length ? '.primary' : ''}`, { id: 'start', type: 'button', onclick: start }, 'Start session');

  function start() {
    const cwd = draft.cwd.trim().replace(/^~(?=\/|$)/, home);
    if (!draft.provider) return fail('No provider has an API key yet. Add one in Settings, or play the walkthrough.');
    if (!cwd) return fail('Choose the workspace folder first.');
    if (!draft.task.trim()) return fail('Describe the task first.');
    return guarded(startButton, async () => {
      const { provider, model, effort, mode, autonomy } = draft;
      const meta = await api.startSession({ cwd, task: draft.task, provider, model, effort, mode, autonomy });
      draft.task = '';
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
      h('div.composer-top', h('label.workspace-pick', { title: 'The folder the agent works in' }, icon('folder', 14), workspace), browse),
      task,
      h('div.composer-bar', modelPicker, setting('effort', 'Effort', efforts), setting('mode', 'Mode', MODES), setting('autonomy', 'Autonomy', AUTONOMIES), startButton),
    ),
    h('div.composer-foot', recentRow, h('span.kbd', `${MOD_KEY}+Enter starts the session`)),
    error,
  );

  const hero =
    keyed.length === 0 &&
    h(
      'section.hero',
      heroMark(),
      h(
        'div',
        h('h2', 'See it work before connecting a model'),
        h('p', 'The walkthrough plays a scripted three-turn session through the real harness: it reproduces a bug, makes a wrong fix that is caught before anything is declared done, then makes the right one. No key, no network.'),
        h(
          'div.hero-actions',
          h('button.btn.primary', { id: 'play-demo', type: 'button', onclick: playDemo }, icon('play', 12), 'Play the walkthrough'),
          h('button.btn.quiet', { id: 'add-key', type: 'button', onclick: openSettings }, icon('key', 14), 'Add an API key'),
        ),
      ),
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
  return {};
}

/** The app mark: prediction and reality, overlapping. */
function heroMark() {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 32 20');
  svg.setAttribute('width', '42');
  svg.setAttribute('height', '26');
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('hero-mark');
  for (const [cx, fill, opacity] of [
    [11, '#6ea8ff', 1],
    [21, '#2dd4bf', 0.8],
  ]) {
    const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
    circle.setAttribute('cx', cx);
    circle.setAttribute('cy', 10);
    circle.setAttribute('r', 9);
    circle.setAttribute('fill', fill);
    circle.setAttribute('fill-opacity', opacity);
    svg.append(circle);
  }
  return svg;
}
