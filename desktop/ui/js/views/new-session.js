// The New session view: workspace, provider and model, effort, mode, autonomy, task.
// With no key configured it leads with the walkthrough and the way to add a key.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { tilde } from '../format.js';
import { MOD_KEY } from './common.js';
import { effortOptions, field, hint, modelOptions, providerOptions, radioChips } from './controls.js';
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
  const workspace = h('input.input.mono', {
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
    'button.btn',
    {
      id: 'browse-workspace',
      type: 'button',
      onclick: async () => {
        const chosen = await pickFolder(draft.cwd.trim());
        if (chosen) setWorkspace(chosen);
      },
    },
    icon('folder', 14),
    'Browse…',
  );
  const recentRow =
    recent.length > 0 &&
    h(
      'div.recent',
      'Recent',
      recent.map((path) => h('button', { type: 'button', title: path, onclick: () => setWorkspace(path) }, tilde(path, home))),
    );

  // Provider and model. The model choices follow the provider.
  const modelSlot = h('div');
  const providerNote = hint();
  function showModels() {
    const provider = state.providers.find((candidate) => candidate.id === draft.provider);
    providerNote.textContent = provider?.note ?? '';
    providerNote.hidden = !provider?.note;
    if (!provider) {
      modelSlot.replaceChildren(h('div.field-hint', 'Models are listed once a provider has a key.'));
      return;
    }
    const options = modelOptions(provider, state);
    if (!options.some((option) => option.value === draft.model)) {
      draft.model = provider.id === state.provider && state.model ? state.model : options[0]?.value ?? null;
    }
    modelSlot.replaceChildren(radioChips({ name: 'model', label: 'Model', options, value: draft.model, onChange: (value) => (draft.model = value) }));
  }
  const providers = radioChips({
    name: 'provider',
    label: 'Provider',
    options: providerOptions(state.providers),
    value: draft.provider,
    onChange: (value) => {
      draft.provider = value;
      draft.model = null;
      showModels();
    },
  });
  showModels();

  /** A segmented choice whose hint line describes the selected option. */
  function described(name, label, options, key, hintFor) {
    const note = hint(hintFor(draft[key]));
    const control = radioChips({
      name,
      label,
      options,
      value: draft[key],
      segmented: true,
      onChange: (value) => {
        draft[key] = value;
        note.textContent = hintFor(value);
      },
    });
    return field(label, control, note);
  }
  const hintOf = (options) => (value) => options.find((option) => option.value === value)?.hint ?? '';

  const task = h('textarea.input', {
    id: 'task',
    rows: 6,
    value: draft.task,
    placeholder: 'What should be done? If you know how to check it, say so: “… so that `make test` passes”.',
    'aria-label': 'Task',
    oninput: () => (draft.task = task.value),
  });

  const startButton = h('button.btn.primary', { id: 'start', type: 'button', onclick: start }, 'Start session');

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
    'section.panel.form',
    {
      onkeydown: (event) => {
        if (event.key !== 'Enter' || !(event.ctrlKey || event.metaKey)) return;
        event.preventDefault();
        start();
      },
    },
    field('Workspace', h('div.field-row', workspace, browse), recentRow),
    field('Provider', providers, providerNote),
    field('Model', modelSlot),
    described('effort', 'Effort', effortOptions(state.effort), 'effort', (value) => EFFORT_HINTS[value] ?? ''),
    described('mode', 'Mode', MODES, 'mode', hintOf(MODES)),
    described('autonomy', 'Autonomy', AUTONOMIES, 'autonomy', hintOf(AUTONOMIES)),
    field('Task', task),
    h('div.form-actions', error, h('span.kbd', `${MOD_KEY}+Enter`), startButton),
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
          h('button.btn', { id: 'add-key', type: 'button', onclick: openSettings }, icon('key', 14), 'Add an API key'),
        ),
      ),
    );

  main.append(
    h('header.view-head', h('h1', 'New session'), keyed.length > 0 && h('button.btn', { id: 'play-demo', type: 'button', onclick: playDemo }, icon('play', 12), 'Play the walkthrough')),
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
  svg.setAttribute('width', '48');
  svg.setAttribute('height', '30');
  svg.setAttribute('aria-hidden', 'true');
  svg.classList.add('hero-mark');
  for (const [cx, fill, opacity] of [
    [11, '#7c5cff', 1],
    [21, '#2dd4bf', 0.78],
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
