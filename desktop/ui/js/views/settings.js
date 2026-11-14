// Settings: how the window looks, an API key per provider, the defaults for new sessions, and the
// text the model is given: skills, the system prompt and the user's instructions.
// Keys only ever travel towards the engine; a stored key is never read back or shown.
//
// Nothing here has a Save button. A choice is kept the moment it is made, and a key when its field
// is left. Text that was typed into a file is kept as a draft: leaving Settings or closing the
// window asks whether to save it (js/unsaved.js), and Ctrl/Cmd+S in the text saves it where it is.

import { api } from '../api.js';
import { h, icon } from '../dom.js';
import { tilde } from '../format.js';
import * as prefs from '../prefs.js';
import * as themes from '../themes.js';
import { track, watchUnsaved } from '../unsaved.js';
import { SEND_KEYS, closeButton, openDialog, sendKey, sendKeyLabel } from './common.js';
import { effortOptions, field, hint, modelOptions, picker, providerOptions, radioChips } from './controls.js';

// What removing a key or a failed save has to say, per provider, once the view is redrawn.
const notices = new Map();

// Text typed into an editor and not saved yet, by what it edits.
const drafts = new Map();
// Keys typed and not handed to the engine yet, by provider: `{ name, value }`.
const typedKeys = new Map();
// The skill whose file is open: its name, NEW, or null.
let openSkill = null;
const NEW = '+new';
const NEW_SKILL = '---\nname: \ntitle: \ndescription: \n---\n\n';

/** The `name` a skill file's front matter gives, or nothing. */
function frontName(text) {
  const front = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text)?.[1] ?? '';
  return /^name:[ \t]*(.*?)[ \t]*$/m.exec(front)?.[1] ?? '';
}

/** A draft as the reminder lists it, and how it is saved, by its key. */
function draftFile(key) {
  if (key === 'system') return { label: 'System prompt', save: (text) => api.setSystemPrompt(text) };
  if (key === 'global') return { label: 'Instructions', save: (text) => api.setGlobalInstructions(text) };
  const name = key.slice('skill:'.length);
  if (name === NEW) return { label: 'New skill', save: (text) => api.saveSkill(frontName(text), text) };
  return { label: `Skill ${name}`, save: (text) => api.saveSkill(name, text) };
}

watchUnsaved(() => [
  ...[...typedKeys].map(([provider, typed]) => ({
    label: `API key for ${typed.name}`,
    save: () => api.setKey(provider, typed.value).then(() => typedKeys.delete(provider)),
    discard: () => typedKeys.delete(provider),
  })),
  ...[...drafts].map(([key, text]) => ({
    label: draftFile(key).label,
    save: () => draftFile(key).save(text).then(() => drafts.delete(key)),
    discard: () => drafts.delete(key),
  })),
]);

export function mountSettings(main, app) {
  const live = h('div.contents');
  const dataFolder = h('section.panel', h('h2.section-title', 'Data folder'), h('div.data-path.mono', tilde(app.state.data_dir, app.state.home)));
  main.append(
    h('div.view-body', h('div.view-column', h('h1.view-title', 'Settings'), appearanceSection(), messagesSection(app), live, skillsSection(), systemSection(), instructionsSection(), dataFolder)),
  );

  /** Reloads the engine state and redraws, since a key changes what both sections offer. */
  async function refresh() {
    await app.reloadState();
    draw();
  }

  // Only these two follow the engine state. The editors below them keep what is being typed.
  function draw() {
    const { state } = app;
    // A key is kept as its field is left, which redraws: the key field moved on to keeps the focus.
    const at = document.activeElement;
    const typing = live.contains(at) && at.matches('.provider input') ? at.closest('[data-provider]').dataset.provider : null;
    live.replaceChildren(
      h(
        'section.panel',
        { id: 'settings-keys' },
        h('div.section-head', h('h2.section-title', 'API keys'), h('button.btn.quiet', { id: 'add-provider', type: 'button', onclick: () => providerDialog(refresh) }, icon('plus', 12), 'Add provider')),
        state.providers.map((provider) => providerRow(provider, refresh)),
      ),
      defaults(state, refresh),
    );
    if (typing) live.querySelector(`[data-provider="${CSS.escape(typing)}"] input`)?.focus();
  }

  draw();
  return {};
}

/**
 * Light or dark, the painting behind the panels, which of the panels show it, and its dither:
 * there or not, moving or still. Kept by the window, not the engine.
 */
function appearanceSection() {
  const pick = themes.chosen();
  const choice = (name, label, values, value) =>
    radioChips({ name, label, segmented: true, options: values.map((option) => ({ value: option, label: option.charAt(0).toUpperCase() + option.slice(1) })), value, onChange: (chosen) => themes.choose({ [name]: chosen }) });

  // The window in small, cut as the window is: a fifth of it the left panel, three fifths the
  // middle one, a fifth the right. Each part is a switch for the background in that panel, and
  // shows its own share of the picture while it is on.
  const map = h('div.panel-map', { id: 'panel-backgrounds', role: 'group', 'aria-label': 'Panel backgrounds' });
  const mapHint = hint();
  function drawMap() {
    const now = themes.chosen();
    const { thumb } = themes.BACKGROUNDS.find((background) => background.id === now.background);
    map.replaceChildren(
      ...themes.PANELS.map((panel) => {
        const label = `${panel.name} panel background`;
        const flip = (event) => themes.choose({ [panel.key]: event.target.checked ? 'on' : 'off' });
        return h(
          'label.panel-zone',
          { dataset: { panel: panel.id } },
          // With no background there is nothing to show in any of them.
          h('input', { type: 'checkbox', checked: Boolean(thumb) && now[panel.key] === 'on', disabled: !thumb, 'aria-label': label, onchange: flip }),
          h('span.panel-face', thumb && h('img.panel-art', { src: thumb, alt: '', draggable: 'false' })),
          h('span.panel-name', panel.name),
        );
      }),
    );
    mapHint.textContent = thumb ? 'Choose which panels have a background.' : 'Choose a background to show it in a panel.';
  }
  drawMap();

  const backgrounds = h(
    'div.theme-grid',
    { role: 'radiogroup', 'aria-label': 'Background' },
    themes.BACKGROUNDS.map((background) =>
      h(
        'label.theme-card',
        { dataset: { background: background.id } },
        h('input', {
          type: 'radio',
          name: 'background',
          value: background.id,
          checked: background.id === pick.background,
          onchange: () => {
            themes.choose({ background: background.id });
            drawMap();
          },
        }),
        h('span.theme-thumb', background.thumb && h('img', { src: background.thumb, alt: '', draggable: 'false' })),
        h('span.theme-name', background.name),
      ),
    ),
  );
  return h(
    'section.panel',
    { id: 'settings-appearance' },
    h('h2.section-title', 'Appearance'),
    field('Mode', choice('appearance', 'Appearance', themes.APPEARANCES, pick.appearance)),
    field('Background', backgrounds),
    field('Panels', map, mapHint),
    field('Dither', choice('dither', 'Dither', ['on', 'off'], pick.dither)),
    field('Motion', choice('motion', 'Motion', ['on', 'off'], pick.motion)),
  );
}

/**
 * Which key sends a task or a follow-up (the other one breaks the line; kept by the window, like
 * the appearance), and whether a request the provider turned away is made again without end
 * (kept by the engine, and reaching the sessions that are running as well as the ones to come).
 */
function messagesSection(app) {
  const keys = radioChips({
    name: 'send',
    label: 'Send with',
    segmented: true,
    options: SEND_KEYS.map((key) => ({ value: key, label: sendKeyLabel(key) })),
    value: sendKey(),
    onChange: (key) => prefs.set('send', key),
  });
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const shown = () => (app.state.auto_retry ? 'on' : 'off');
  const autoRetry = radioChips({
    name: 'auto-retry',
    label: 'Auto-retry',
    segmented: true,
    options: [
      { value: 'on', label: 'On', title: 'Retry until connected or stopped' },
      { value: 'off', label: 'Off', title: 'Stop after 8 retries (about 4 minutes)' },
    ],
    value: shown(),
    onChange: async (value) => {
      error.hidden = true;
      try {
        await api.setAutoRetry(value === 'on');
        await app.reloadState();
      } catch (problem) {
        error.textContent = String(problem);
        error.hidden = false;
        // The choice was not kept: show the one that still holds.
        autoRetry.querySelector(`input[value="${shown()}"]`).checked = true;
      }
    },
  });
  return h('section.panel', { id: 'settings-messages' }, h('h2.section-title', 'Messages'), field('Send with', keys), field('Auto-retry', autoRetry, error));
}

/**
 * A provider and its key, on one line. One that has a key is marked and in full ink; one that has
 * none is greyed. The key is kept when its field is left or Enter is pressed. One of the user's own
 * asks for no key, and in place of its key's removal has its own, on a second click.
 */
function providerRow(provider, refresh) {
  const { custom } = provider;
  const keyed = provider.has_key;
  let storing = false;

  const store = (key) => track(keep(key));
  async function keep(key) {
    if (storing) return;
    storing = true;
    typedKeys.delete(provider.id);
    notices.delete(provider.id);
    try {
      await api.setKey(provider.id, key);
      if (!key.trim()) notices.set(provider.id, { removed: true });
    } catch (problem) {
      notices.set(provider.id, { error: String(problem) });
    }
    await refresh();
  }

  async function drop() {
    typedKeys.delete(provider.id);
    try {
      await api.removeProvider(provider.id);
    } catch (problem) {
      notices.set(provider.id, { error: String(problem) });
    }
    await refresh();
  }

  const input = h('input.input', {
    type: 'password',
    autocomplete: 'off',
    spellcheck: 'false',
    value: typedKeys.get(provider.id)?.value ?? '',
    placeholder: keyed && !custom ? 'Replace key' : 'API key',
    'aria-label': `API key for ${provider.name}`,
    oninput: () => (input.value.trim() ? typedKeys.set(provider.id, { name: provider.name, value: input.value }) : typedKeys.delete(provider.id)),
    onblur: () => input.value.trim() && store(input.value),
    onkeydown: (event) => {
      if (event.key === 'Enter' && input.value.trim()) store(input.value);
    },
  });
  let armed = false;
  const remove = custom
    ? h(
        'button.btn.danger.remove-provider',
        {
          type: 'button',
          onblur: () => {
            armed = false;
            remove.textContent = 'Delete';
          },
          onclick: () => {
            if (armed) return track(drop());
            armed = true;
            remove.textContent = 'Confirm';
          },
        },
        'Delete',
      )
    : h('button.btn.danger.remove-key', { type: 'button', hidden: !keyed, onclick: () => store('') }, 'Remove');

  const notice = notices.get(provider.id);
  notices.delete(provider.id);
  // Still keyed after a removal: the system's environment is supplying one.
  const said = notice?.error
    ? h('div.form-error', { role: 'alert' }, notice.error)
    : notice?.removed && keyed && !custom && hint({ role: 'status' }, 'Removed from Sage. A key from your system environment is still in use.');

  const mark = custom ? 'Your own' : keyed ? 'Key set' : 'No key';
  return h(
    `div.provider${keyed ? '' : '.is-unset'}`,
    { dataset: { provider: provider.id } },
    h('span.provider-mark', { 'data-tooltip': mark, role: 'img', 'aria-label': mark }, keyed && icon('check', 14)),
    h('span.provider-who', { 'data-tooltip': provider.note || null }, h('span.provider-name', provider.name), provider.serves ? h('span.provider-serves', provider.serves) : null),
    h('span.provider-url', { 'data-tooltip': provider.base_url, 'data-tooltip-overflow': '' }, provider.base_url),
    input,
    remove,
    said,
  );
}

/**
 * The dialog that adds a provider of the user's own: any address that answers as OpenAI's chat
 * completions do. Add (or Enter) adds it and closes; Cancel, the cross or Escape let it go.
 */
function providerDialog(refresh) {
  const error = h('div.form-error', { role: 'alert', hidden: true });
  const box = (key, label, placeholder, type = 'text') => h('input.input', { id: `new-provider-${key}`, type, autocomplete: 'off', spellcheck: 'false', placeholder, 'aria-label': label });
  const [name, url, models, key] = [box('name', 'Provider name', 'My provider'), box('url', 'Provider address', 'https://…/v1'), box('models', 'Models', 'model-a, model-b'), box('key', 'API key', 'Optional', 'password')];
  const add = h('button.btn.primary', { id: 'new-provider-add', type: 'submit' }, 'Add');

  openDialog('provider-dialog', (close) => {
    async function submit(event) {
      event.preventDefault();
      add.disabled = true;
      error.hidden = true;
      try {
        await track(api.addProvider({ name: name.value, baseUrl: url.value, models: models.value.split(/[,\n]/), key: key.value }));
        close();
        await refresh();
      } catch (problem) {
        error.textContent = String(problem);
        error.hidden = false;
        add.disabled = false;
      }
    }
    return [
      h('div.dialog-head', h('h2', 'Add provider'), closeButton(close)),
      h(
        'form.provider-new',
        { id: 'new-provider', onsubmit: submit },
        h(
          'div.provider-fields',
          h('p.field-hint', 'Any provider with an OpenAI-compatible chat completions endpoint.'),
          field('Name', name),
          field('Address', url),
          field('Models', models, hint('Separated by commas.')),
          field('API key', key, error),
        ),
        h('div.dialog-foot', h('button.btn', { type: 'button', onclick: close }, 'Cancel'), add),
      ),
    ];
  });
  name.focus();
}

function defaults(state, refresh) {
  const provider = state.providers.find((candidate) => candidate.id === state.provider && candidate.has_key);
  const error = h('div.form-error', { role: 'alert', hidden: !notices.has('defaults') }, notices.get('defaults')?.error ?? null);
  notices.delete('defaults');
  // Kept the moment it is chosen.
  const apply = async (change) => {
    try {
      await api.setDefaults(change);
    } catch (problem) {
      notices.set('defaults', { error: String(problem) });
    }
    await refresh();
  };

  return h(
    'section.panel',
    { id: 'settings-defaults' },
    h('h2.section-title', 'Defaults'),
    field(
      'Provider',
      picker({
        id: 'default-provider',
        name: 'Default provider',
        boxed: true,
        groups: [{ options: providerOptions(state.providers) }],
        value: provider?.id ?? null,
        // A saved model belongs to its provider, so the two are always saved together.
        onChange: (id) => apply({ provider: id, model: state.providers.find((candidate) => candidate.id === id).models[0] }),
      }),
    ),
    provider &&
      field(
        'Model',
        picker({ id: 'default-model', name: 'Default model', boxed: true, groups: [{ options: modelOptions(provider, state) }], value: state.model, onChange: (model) => apply({ provider: provider.id, model }) }),
      ),
    field('Effort', radioChips({ name: 'default-effort', label: 'Default effort', options: effortOptions(state.effort), value: state.effort, segmented: true, onChange: (effort) => apply({ effort }) }), error),
  );
}

/**
 * A text file being edited: the text area and at most one action beside saving. What is typed is
 * a draft until it is saved, with Ctrl/Cmd+S here or from the reminder on the way out.
 * `set` hands it a file and what its actions do.
 */
function fileEditor(label) {
  let key = '';
  let stored = '';
  let actions = {};
  const area = h('textarea.input.editor', {
    'aria-label': label,
    spellcheck: 'false',
    autocomplete: 'off',
    oninput: sync,
    onkeydown: (event) => {
      if (event.key.toLowerCase() !== 's' || !(event.ctrlKey || event.metaKey)) return;
      event.preventDefault();
      if (area.value !== stored) run(() => actions.save(area.value));
    },
  });
  const error = h('div.form-error', { role: 'alert', hidden: true });
  // An action that throws text away takes a second click.
  let armed = false;
  const disarm = () => {
    armed = false;
    if (actions.other) other.textContent = actions.other.label;
    other.classList.toggle('danger', actions.other?.danger === true);
  };
  const other = h('button.btn', {
    type: 'button',
    hidden: true,
    disabled: true,
    onblur: disarm,
    onclick: () => {
      if (actions.other.confirm && !armed) {
        armed = true;
        other.textContent = 'Confirm';
        other.classList.add('danger');
      } else {
        disarm();
        run(() => actions.other.run());
      }
    },
  });

  function sync() {
    if (!key) return;
    if (area.value !== stored) drafts.set(key, area.value);
    else drafts.delete(key);
    other.disabled = !actions.other?.enabled;
  }

  /** Runs an action. On success the action hands the editor its file again. */
  async function run(action) {
    other.disabled = true;
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
    el: h('div.editor-box', area, error, h('div.editor-actions', other)),
    area,
    /** `other` is `{ label, danger, confirm, enabled, run }` or null. An unsaved draft of the file comes back. */
    set(file) {
      key = file.key;
      stored = file.text;
      actions = file;
      const wanted = drafts.get(key) ?? stored;
      if (area.value !== wanted) area.value = wanted;
      error.hidden = true;
      other.hidden = !file.other;
      disarm();
      sync();
    },
  };
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
      const key = `skill:${NEW}`;
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
      const text = await api.skillFile(skill.name, null);
      // Another row was opened while this file was read: that one's load draws.
      if (openSkill !== skill.name) return;
      editor.set({
        key,
        text,
        save: async (text) => {
          await api.saveSkill(skill.name, text);
          await settle(key, skill.name);
        },
        // Deleting the user's version of a built-in skill gives the built-in back.
        other:
          mine && !skill.overrides_builtin
            ? { label: 'Delete', danger: true, confirm: true, enabled: true, run: () => api.deleteSkill(skill.name).then(() => settle(key, null)) }
            : { label: 'Reset', confirm: true, enabled: mine, run: () => api.deleteSkill(skill.name).then(() => settle(key, skill.name)) },
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
          { type: 'button', 'data-tooltip': skill.description || null, 'aria-expanded': String(open), dataset: { skill: skill.name }, onclick: () => show(open ? null : skill.name) },
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
    editor.set({ key, text: system, save: store, other: { label: 'Reset', confirm: true, enabled: custom, run: () => store(null) } });
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
  return h('section.panel', { id: 'settings-instructions' }, h('div.section-head', h('h2.section-title', 'Instructions'), h('span.section-file.mono', 'AGENTS.md')), editor.el);
}
