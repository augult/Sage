// The New session view. With the middle panel's background on it opens on the bare glass: in its
// middle the app's mark, one line that asks, and the chat bar a session has at its foot. In the
// bar are the task and, along its foot, what attaches a file, the effort, the model, the
// microphone and what starts. Under the bar is the folder the session will work in, the recent
// ones to pick from and the way to another, with the autonomy beside it. Skills are
// commands typed into the task (js/views/skills.js). With no key configured the way to add one is
// offered over the bar.

import { api } from '../api.js';
import { brandMark, h, icon } from '../dom.js';
import { folderNames, plainPath, tilde } from '../format.js';
import { attachments } from './attach.js';
import { barButton, chatBar, fitBox, markIdle, onSendKey } from './common.js';
import { AUTONOMIES, effortOptions, modelOptions, modelPicker, picker, settingPicker } from './controls.js';
import { dictation } from './dictate.js';
import { skillCommands } from './skills.js';

// What has been entered so far. It survives a trip to Settings and back; the skills start empty
// for every new session.
const draft = { cwd: null, provider: null, model: null, effort: null, autonomy: 'auto', task: '', skills: [], files: [] };

// What the folder menu offers besides folders: the way to another. No path is spelled so.
const CHOOSE = ':choose';

/** Workspaces of earlier real sessions, most recent first. */
function recentWorkspaces(sessions) {
  return [...new Set(sessions.filter((meta) => !meta.simulated).map((meta) => plainPath(meta.cwd)))].slice(0, 5);
}

/** Makes `path` the folder of the session being drafted: what File > Open folder does. */
export function useFolder(path) {
  draft.cwd = plainPath(path);
}

export function mountNewSession(main, app) {
  const { state } = app;
  const keyed = state.providers.filter((provider) => provider.has_key);
  const recent = recentWorkspaces(app.sessions);

  draft.cwd ??= recent[0] ?? '';
  if (!keyed.some((provider) => provider.id === draft.provider)) {
    draft.provider = null;
    draft.model = null;
  }
  const chosen = keyed.find((provider) => provider.id === (draft.provider ?? state.provider)) ?? keyed[0];
  const models = chosen ? modelOptions(chosen, state).map((option) => option.value) : [];
  let selection = {
    provider: chosen?.id ?? null,
    model: models.includes(draft.model) ? draft.model : models[0] ?? null,
  };
  if (draft.model === null && chosen?.id === state.provider && models.includes(state.model)) selection.model = state.model;

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

  // The folder, under the bar: the recent ones with the chosen one checked, and the way to any other.
  const folderSlot = h('span.pick-slot');
  const folderButton = () => folderSlot.querySelector('#pick-folder');
  function drawFolder() {
    const within = folderSlot.contains(document.activeElement);
    // One picked with the dialog is listed as well, ahead of the recent ones.
    const paths = draft.cwd && !recent.includes(draft.cwd) ? [draft.cwd, ...recent] : recent;
    const names = folderNames(paths);
    folderSlot.replaceChildren(
      picker({
        id: 'pick-folder',
        name: 'Folder',
        glyph: 'folder',
        groups: [{ options: paths.map((path) => ({ value: path, label: names.get(path), title: tilde(path, state.home) })) }, { options: [{ value: CHOOSE, label: 'Choose folder…' }] }],
        value: draft.cwd || null,
        empty: 'Choose a folder',
        onChange: (value) => (value === CHOOSE ? browse() : setWorkspace(value)),
      }),
    );
    if (within) folderButton().focus();
  }
  const setWorkspace = (path) => {
    draft.cwd = path;
    drawFolder();
    loadSkills();
  };
  // The system's own dialog: Explorer, Finder, the desktop's file chooser.
  async function browse() {
    await guarded(folderButton(), async () => {
      const picked = await api.pickFolder(draft.cwd || null);
      if (picked) draft.cwd = plainPath(picked);
    });
    // Drawn again whatever came of it: the picker names the folder, not the way to another.
    setWorkspace(draft.cwd);
    folderButton().focus();
  }

  /** A run setting: its name, the current value, and a menu of the others. */
  const setting = (key, name, options, changed) =>
    settingPicker(key, name, options, draft[key] ?? state[key], (value) => {
      draft[key] = value;
      changed?.();
    });

  const task = h('textarea', {
    id: 'task',
    rows: 1,
    value: draft.task,
    placeholder: 'Describe a task or ask a question',
    'aria-label': 'Task',
    oninput: () => {
      draft.task = task.value;
      fitBox(task);
      markIdle(startButton, task);
    },
    onkeydown: (event) => skills.key(event) || onSendKey(event, start),
  });

  // The skills on offer follow the folder: a project brings its own.
  let skillList = [];
  let skillsAsked = 0;
  const skills = skillCommands({ box: task, list: () => skillList, value: draft.skills, onChange: (names) => (draft.skills = names) });
  const hint = h('span.chat-hint', 'Type / for skills');
  async function loadSkills() {
    const asked = ++skillsAsked;
    const listed = await api.skills(draft.cwd || null).catch(() => []);
    // A slower answer for an earlier folder must not replace a later one.
    if (asked !== skillsAsked || JSON.stringify(listed) === JSON.stringify(skillList)) return;
    skillList = listed;
    skills.refresh();
  }

  // With no key there is nothing to start: adding one is the view's one primary action.
  const startButton = barButton('start', 'Start', icon('send', 16), start, keyed.length > 0 ? 'primary' : 'bare');
  const files = attachments({ start: () => draft.cwd, value: draft.files, onChange: (paths) => (draft.files = paths) });
  const mic = dictation({ box: task, fail });

  function start() {
    // Enter pressed again while the first start is on its way would start a second session.
    if (startButton.disabled) return;
    // A command typed out and sent is taken now. Alone in the box, it was all there was to do.
    if (skills.take()) return;
    if (!selection.provider) return fail('No API key.');
    if (!draft.cwd) return fail('No folder.');
    if (!draft.task.trim()) return fail('No task.');
    mic.stop();
    return guarded(startButton, async () => {
      const { cwd, task, autonomy } = draft;
      const spentSkills = draft.skills;
      const spentFiles = draft.files;
      const meta = await api.startSession({ cwd, task, ...selection, effort: draft.effort ?? state.effort, autonomy, skills: spentSkills, files: spentFiles });
      if (draft.task === task) draft.task = '';
      skills.remove(spentSkills);
      files.remove(spentFiles);
      await app.opened(meta);
    });
  }

  const bar = h(
    'div.new-bar',
    {
      // From a picker. In the task box the key is the user's choice.
      onkeydown: (event) => {
        if (event.target === task || event.key !== 'Enter' || !(event.ctrlKey || event.metaKey)) return;
        event.preventDefault();
        start();
      },
    },
    chatBar({
      over: error,
      files,
      skills,
      box: task,
      start: setting('effort', 'Effort', effortOptions(state.effort)),
      end: [modelPicker(state, selection, (choice) => {
        selection = choice;
        Object.assign(draft, choice);
      }, 'end'), mic.button, startButton],
      under: [folderSlot, setting('autonomy', 'Autonomy', AUTONOMIES), hint],
    }),
  );

  // The way to a key leads while there is none.
  const lead = keyed.length === 0 && h('button.btn.primary.new-lead', { id: 'add-key', type: 'button', onclick: () => app.show({ name: 'settings', section: 'settings-keys' }) }, icon('key', 14), 'Add an API key');
  const hero = h('div.new-hero', brandMark(44), h('h1.view-title', 'What should we work on?'));

  main.append(h('div.new-body', hero, lead, bar));
  drawFolder();
  loadSkills();
  fitBox(task);
  markIdle(startButton, task);
  // Start where the first thing is still missing.
  (lead || (draft.cwd ? task : folderButton())).focus();
  return {
    unmount() {
      skillsAsked++;
      mic.stop();
    },
  };
}
