// Settings: an API key per provider, the defaults for new sessions, and the text the model is
// given: skills, the system prompt and the user's instructions.
// Keys only ever travel towards the engine; a stored key is never read back or shown.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { tilde } from '../format.js';
import { tag } from './common.js';
import { effortOptions, field, hint, modelOptions, providerOptions, radioChips } from './controls.js';

// What the last save or removal did, per provider, to show once the view is redrawn.
const notices = new Map();
const REMOVED = 'Removed';

// Text typed into an editor and not saved yet, by what it edits. It survives leaving the view.
const drafts = new Map();
// The skill whose file is open: its name, NEW, or null.
let openSkill = null;
const NEW = '+new';
const NEW_SKILL = '---\nname: \ntitle: \ndescription: \n---\n\n';

export function mountSettings(main, app) {
  const live = h('div.contents');
  const dataFolder = h('section.panel', h('h2.section-title', 'Data folder'), h('div.data-path.mono', tilde(app.state.data_dir, app.state.home)));
  main.append(
    h('header.view-head', { 'data-tauri-drag-region': 'deep' }, h('h1', 'Settings')),
    h('div.view-body', h('div.view-column', live, skillsSection(), systemSection(), instructionsSection(), dataFolder)),
  );

  /** Reloads the engine state and redraws, since a key changes what both sections offer. */
  async function refresh() {
    await app.reloadState();
    draw();
  }

  // Only these two follow the engine state. The editors below them keep what is being typed.
  function draw() {
    const { state } = app;
    live.replaceChildren(
      h(
        'section.panel',
        h('h2.section-title', 'API keys'),
        state.providers.map((provider) => providerCard(provider, refresh)),
      ),
      defaults(state, refresh),
    );
  }

  draw();
  return {};
}

function providerCard(provider, refresh) {
  const custom = provider.id === 'custom';
  const input = h('input.input', {
    type: 'password',
    autocomplete: 'off',
    spellcheck: 'false',
    placeholder: provider.has_key && !custom ? 'New key' : 'API key',
    'aria-label': `API key for ${provider.name}`,
    oninput: () => (save.disabled = !input.value.trim()),
    onkeydown: (event) => {
      if (event.key === 'Enter' && !save.disabled) store(input.value);
    },
  });
  const save = h('button.btn.save-key', { type: 'button', disabled: true, onclick: () => store(input.value) }, 'Save');
  const remove = h('button.btn.danger.remove-key', { type: 'button', disabled: !provider.has_key, onclick: () => store('') }, 'Remove');
  const status = hint();
  status.setAttribute('role', 'status');
  status.textContent = notices.get(provider.id) ?? '';
  // Still keyed after a removal: the environment variable is supplying one.
  if (status.textContent === REMOVED && provider.has_key && !custom) status.textContent += ` · ${provider.key_env} still set`;
  status.hidden = !status.textContent;
  notices.delete(provider.id);

  async function store(key) {
    save.disabled = remove.disabled = true;
    try {
      await api.setKey(provider.id, key);
      notices.set(provider.id, key.trim() ? 'Saved' : REMOVED);
    } catch (problem) {
      notices.set(provider.id, String(problem));
    }
    await refresh();
  }

  const state = custom ? tag('custom endpoint') : provider.has_key ? tag('key configured', 'teal') : tag('no key');
  return h(
    'div.provider',
    { dataset: { provider: provider.id } },
    h('div.provider-head', h('span.provider-name', provider.name), h('span.provider-url', provider.base_url), h('span.provider-url', provider.key_env), state),
    // The engine's own caveat about a provider, such as one that logs prompts.
    provider.note && hint(provider.note),
    h('div.field-row', input, save, remove),
    status,
  );
}

function defaults(state, refresh) {
  const provider = state.providers.find((candidate) => candidate.id === state.provider && candidate.has_key);
  const saved = hint();
  saved.setAttribute('role', 'status');
  const apply = async (change) => {
    try {
      await api.setDefaults(change);
      notices.set('defaults', 'Saved');
    } catch (problem) {
      notices.set('defaults', String(problem));
    }
    await refresh();
  };
  saved.textContent = notices.get('defaults') ?? '';
  notices.delete('defaults');

  return h(
    'section.panel',
    h('h2.section-title', 'Defaults'),
    field(
      'Provider',
      radioChips({
        name: 'default-provider',
        label: 'Default provider',
        options: providerOptions(state.providers),
        value: provider?.id,
        // A saved model belongs to its provider, so the two are always saved together.
        onChange: (id) => apply({ provider: id, model: state.providers.find((candidate) => candidate.id === id).models[0] }),
      }),
    ),
    provider &&
      field('Model', radioChips({ name: 'default-model', label: 'Default model', options: modelOptions(provider, state), value: state.model, onChange: (model) => apply({ provider: provider.id, model }) })),
    field('Effort', radioChips({ name: 'default-effort', label: 'Default effort', options: effortOptions(state.effort), value: state.effort, segmented: true, onChange: (effort) => apply({ effort }) }), saved),
  );
}

/**
 * A text file being edited: the text area, Save (lit only while the text differs from what is
 * stored) and at most one other action. `set` hands it a file and what the buttons do with it.
 */
function fileEditor(label) {
  let key = '';
  let stored = '';
  let actions = {};
  const area = h('textarea.input.editor', { 'aria-label': label, spellcheck: 'false', autocomplete: 'off', oninput: sync });
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const other = h('button.btn', { type: 'button', hidden: true, disabled: true, onclick: () => run(() => actions.other.run()) });
  const save = h('button.btn.editor-save', { type: 'button', disabled: true, onclick: () => run(() => actions.save(area.value)) }, 'Save');

  function sync() {
    const dirty = area.value !== stored;
    if (dirty) drafts.set(key, area.value);
    else drafts.delete(key);
    save.disabled = !dirty;
    other.disabled = !actions.other?.enabled;
  }

  /** Runs a button's action. On success the action hands the editor its file again. */
  async function run(action) {
    save.disabled = other.disabled = true;
    error.hidden = true;
    try {
      await action();
    } catch (problem) {
      error.textContent = String(problem);
      error.hidden = false;
      sync();
    }
  }

  return {
    el: h('div.editor-box', area, h('div.editor-actions', error, other, save)),
    area,
    /** `other` is `{ label, danger, enabled, run }` or null. An unsaved draft of the file comes back. */
    set(file) {
      key = file.key;
      stored = file.text;
      actions = file;
      const wanted = drafts.get(key) ?? stored;
      if (area.value !== wanted) area.value = wanted;
      error.hidden = true;
      other.hidden = !file.other;
      if (file.other) {
        other.textContent = file.other.label;
        other.classList.toggle('danger', file.other.danger === true);
      }
      sync();
    },
  };
}

/** The `name` a skill file's front matter gives, or nothing. */
function frontName(text) {
  const front = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text)?.[1] ?? '';
  return /^name:[ \t]*(.*?)[ \t]*$/m.exec(front)?.[1] ?? '';
}

const skillState = (skill) => (skill.source === 'builtin' ? 'Built-in' : skill.overrides_builtin ? 'Edited' : 'Custom');

function skillsSection() {
  const list = h('div.skill-list');
  // One editor for the section, moved under whichever row is open.
  const editor = fileEditor('Skill file');
  editor.area.id = 'skill-file';
  let skills = [];

  /** Drops the draft of the file just saved or removed, then shows the list as the engine has it. */
  const settle = async (key, open) => {
    drafts.delete(key);
    openSkill = open;
    await load();
  };

  async function load() {
    skills = await api.skills(null);
    const skill = skills.find((candidate) => candidate.name === openSkill);
    if (openSkill === NEW) {
      const key = 'skill:+new';
      editor.set({
        key,
        text: NEW_SKILL,
        save: async (text) => {
          const name = frontName(text);
          await api.saveSkill(name, text);
          await settle(key, name);
        },
        other: { label: 'Cancel', enabled: true, run: () => settle(key, null) },
      });
    } else if (skill) {
      const key = `skill:${skill.name}`;
      const mine = skill.source !== 'builtin';
      editor.set({
        key,
        text: await api.skillFile(skill.name, null),
        save: async (text) => {
          await api.saveSkill(skill.name, text);
          await settle(key, skill.name);
        },
        // Deleting the user's version of a built-in skill gives the built-in back.
        other:
          mine && !skill.overrides_builtin
            ? { label: 'Delete', danger: true, enabled: true, run: () => api.deleteSkill(skill.name).then(() => settle(key, null)) }
            : { label: 'Reset', enabled: mine, run: () => api.deleteSkill(skill.name).then(() => settle(key, skill.name)) },
      });
    } else {
      openSkill = null;
    }
    draw();
  }

  function draw() {
    const rows = openSkill === NEW ? [editor.el] : [];
    for (const skill of skills) {
      const open = skill.name === openSkill;
      rows.push(
        h(
          'button.skill-row',
          { type: 'button', title: skill.description || null, 'aria-expanded': String(open), dataset: { skill: skill.name }, onclick: () => show(open ? null : skill.name) },
          icon('chevron', 12),
          h('span.skill-title', skill.title),
          h('span.skill-state', skillState(skill)),
        ),
      );
      if (open) rows.push(editor.el);
    }
    list.replaceChildren(...rows);
  }

  async function show(name) {
    openSkill = name;
    await load();
    if (name !== NEW) return;
    editor.area.focus();
    // Start typing where the name goes.
    if (editor.area.value === NEW_SKILL) editor.area.setSelectionRange(10, 10);
  }

  load();
  return h(
    'section.panel',
    { id: 'settings-skills' },
    h('div.section-head', h('h2.section-title', 'Skills'), h('button.btn.quiet', { id: 'new-skill', type: 'button', onclick: () => show(openSkill === NEW ? null : NEW) }, icon('plus', 12), 'New skill')),
    list,
  );
}

function systemSection() {
  const key = 'system';
  const editor = fileEditor('System prompt');
  editor.area.id = 'system-prompt';
  const store = async (template) => {
    await api.setSystemPrompt(template);
    drafts.delete(key);
    await load();
  };
  async function load() {
    const { system, system_custom: custom } = await api.instructions();
    editor.set({ key, text: system, save: store, other: { label: 'Reset', enabled: custom, run: () => store(null) } });
  }
  load();
  return h('section.panel', { id: 'settings-system' }, h('h2.section-title', 'System prompt'), editor.el);
}

function instructionsSection() {
  const key = 'global';
  const editor = fileEditor('Instructions');
  editor.area.id = 'global-instructions';
  async function load() {
    const { global } = await api.instructions();
    editor.set({
      key,
      text: global,
      save: async (text) => {
        await api.setGlobalInstructions(text);
        drafts.delete(key);
        await load();
      },
      other: null,
    });
  }
  load();
  return h('section.panel', { id: 'settings-instructions' }, h('div.section-head', h('h2.section-title', 'Instructions'), h('span.section-file', 'AGENTS.md')), editor.el);
}
