// The New session view. The chat bar sits where a session's does, at the foot of the sheet: the
// task, with what attaches a file before it and what starts after it, and under them the pickers
// for model, skills, effort, mode and autonomy. In the
// middle of the sheet is the folder the session will work in: the recent ones to pick from, and
// the way to another. With no key configured the way to add one is offered there too.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { folderNames, plainPath, tilde } from '../format.js';
import { attachments } from './attach.js';
import { barButton, chatBar, fitBox, onSendKey } from './common.js';
import { AUTONOMIES, effortOptions, modelOptions, modelPicker, settingPicker } from './controls.js';
import { skillsButton } from './skills.js';

const MODES = [
  { value: 'presage', label: 'Presage' },
  { value: 'classic', label: 'Classic' },
];

// What has been entered so far. It survives a trip to Settings and back; the skills start empty
// for every new session.
const draft = { cwd: null, provider: null, model: null, effort: null, mode: 'presage', autonomy: 'auto', task: '', skills: [], files: [] };

/** Workspaces of earlier real sessions, most recent first. */
function recentWorkspaces(sessions) {
  return [...new Set(sessions.filter((meta) => !meta.simulated).map((meta) => plainPath(meta.cwd)))].slice(0, 5);
}

export function mountNewSession(main, app) {
  const { state } = app;
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

  /** Runs `action` with `button` disabled, and shows its failure over the chat bar. */
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

  // The folder: the recent ones with the chosen one checked, and the way to any other.
  const projects = h('div.projects', { role: 'group', 'aria-label': 'Folder' });
  function drawProjects() {
    const within = projects.contains(document.activeElement);
    // One picked with the dialog is listed as well, ahead of the recent ones.
    const paths = draft.cwd && !recent.includes(draft.cwd) ? [draft.cwd, ...recent] : recent;
    const names = folderNames(paths);
    const check = () => {
      const mark = icon('check', 14);
      mark.classList.add('project-check');
      return mark;
    };
    projects.replaceChildren(
      ...paths.map((path) =>
        h(
          'button.project',
          { type: 'button', title: tilde(path, state.home), 'aria-pressed': String(path === draft.cwd), onclick: () => setWorkspace(path) },
          icon('folder', 14),
          h('span.project-name', names.get(path)),
          check(),
        ),
      ),
      paths.length > 0 && h('div.projects-rule'),
      h(
        'button.project',
        {
          id: 'browse-workspace',
          type: 'button',
          // The system's own dialog: Explorer, Finder, the desktop's file chooser.
          onclick: (event) =>
            guarded(event.currentTarget, async () => {
              const picked = await api.pickFolder(draft.cwd || null);
              if (picked) setWorkspace(plainPath(picked));
            }),
        },
        'Choose folder…',
      ),
    );
    if (within) projects.querySelector('[aria-pressed="true"]')?.focus();
  }
  const setWorkspace = (path) => {
    draft.cwd = path;
    drawProjects();
    loadSkills();
  };

  /** A run setting: its name, the current value, and a menu of the others. */
  const setting = (key, name, options, changed) =>
    settingPicker(key, name, options, draft[key], (value) => {
      draft[key] = value;
      changed?.();
    });

  // Skills sit next to the model. The list follows the folder: a project brings its own.
  const skillsSlot = h('span.pick-slot');
  let skillList = [];
  let skillsAsked = 0;
  function drawSkills() {
    // The classic baseline takes no skills.
    skillsSlot.replaceChildren(skillsButton({ skills: skillList, value: draft.skills, disabled: draft.mode === 'classic', onChange: (names) => (draft.skills = names) }));
  }
  async function loadSkills() {
    const asked = ++skillsAsked;
    const listed = await api.skills(draft.cwd || null);
    // A slower answer for an earlier folder must not replace a later one.
    if (asked !== skillsAsked || JSON.stringify(listed) === JSON.stringify(skillList)) return;
    skillList = listed;
    // Another folder may not have what was picked.
    draft.skills = draft.skills.filter((name) => listed.some((skill) => skill.name === name));
    drawSkills();
  }
  drawProjects();
  drawSkills();
  loadSkills();

  const task = h('textarea', {
    id: 'task',
    rows: 1,
    value: draft.task,
    placeholder: 'Task',
    'aria-label': 'Task',
    oninput: () => {
      draft.task = task.value;
      fitBox(task);
    },
    onkeydown: (event) => onSendKey(event, start),
  });

  // With no key there is nothing to start: adding one is the view's one primary action.
  const startButton = barButton('start', 'Start', icon('send', 16), start, keyed.length > 0 ? 'primary' : 'bare');
  const files = attachments({ start: () => draft.cwd, value: draft.files, onChange: (paths) => (draft.files = paths) });

  function start() {
    if (!draft.provider) return fail('No API key.');
    if (!draft.cwd) return fail('No folder.');
    if (!draft.task.trim()) return fail('No task.');
    return guarded(startButton, async () => {
      const { cwd, provider, model, effort, mode, autonomy } = draft;
      const meta = await api.startSession({ cwd, task: draft.task, provider, model, effort, mode, autonomy, skills: mode === 'classic' ? [] : draft.skills, files: draft.files });
      draft.task = '';
      draft.skills = [];
      draft.files = [];
      await app.opened(meta);
    });
  }

  const pickers = [modelPicker(state, draft, (choice) => Object.assign(draft, choice)), skillsSlot, setting('effort', 'Effort', effortOptions(state.effort)), setting('mode', 'Mode', MODES, drawSkills), setting('autonomy', 'Autonomy', AUTONOMIES)];
  const foot = h(
    'footer.session-foot',
    {
      // From a picker. In the task box the key is the user's choice.
      onkeydown: (event) => {
        if (event.target === task || event.key !== 'Enter' || !(event.ctrlKey || event.metaKey)) return;
        event.preventDefault();
        start();
      },
    },
    chatBar({ over: error, files, box: task, buttons: startButton, pickers }),
  );

  // The way to a key leads while there is none.
  const lead = keyed.length === 0 && h('button.btn.primary', { id: 'add-key', type: 'button', onclick: () => app.show({ name: 'settings' }) }, icon('key', 14), 'Add an API key');

  main.append(h('header.view-head', { 'data-tauri-drag-region': 'deep' }), h('div.new-body', projects, lead), foot);
  fitBox(task);
  // Start where the first thing is still missing.
  (lead || (draft.cwd ? task : projects.querySelector('#browse-workspace'))).focus();
  return {
    unmount() {
      skillsAsked++;
    },
  };
}
