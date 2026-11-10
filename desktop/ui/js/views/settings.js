// Settings: an API key per provider, and the defaults for new sessions.
// Keys only ever travel towards the engine; a stored key is never read back or shown.

import { api } from '../api.js';
import { h } from '../dom.js';
import { tilde } from '../format.js';
import { tag } from './common.js';
import { effortOptions, field, hint, modelOptions, providerOptions, radioChips } from './controls.js';

// What the last save or removal did, per provider, to show once the view is redrawn.
const notices = new Map();

export function mountSettings(main, app) {
  const body = h('div.view-column');
  main.append(h('header.view-head', h('h1', 'Settings')), h('div.view-body', body));

  /** Reloads the engine state and redraws, since a key changes what every section offers. */
  async function refresh() {
    await app.reloadState();
    draw();
  }

  function draw() {
    const { state } = app;
    body.replaceChildren(
      h('section.panel', h('div.label.panel-title', 'API keys'), state.providers.map((provider) => providerCard(provider, refresh))),
      defaults(state, refresh),
      h('p.field-hint', `Sessions, saved outputs, lessons and keys live in ${tilde(state.data_dir, state.home)}. Presage ${state.version}.`),
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
    placeholder: provider.has_key && !custom ? 'Paste a new key to replace the current one' : 'Paste an API key',
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
  status.hidden = !status.textContent;
  notices.delete(provider.id);

  async function store(key) {
    save.disabled = remove.disabled = true;
    try {
      await api.setKey(provider.id, key);
      notices.set(provider.id, key.trim() ? 'Key saved.' : 'Stored key removed.');
    } catch (problem) {
      notices.set(provider.id, String(problem));
    }
    await refresh();
  }

  const state = custom ? tag('custom endpoint') : provider.has_key ? tag('key configured', 'teal') : tag('no key');
  return h(
    'div.provider',
    { dataset: { provider: provider.id } },
    h('div.provider-head', h('span.provider-name', provider.name), h('span.provider-url', provider.base_url), state),
    provider.note && hint(provider.note),
    h('div.field-row', input, save, remove),
    hint('Or set ', h('code', provider.key_env), ' in the environment, which takes precedence over a stored key.'),
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
      notices.set('defaults', 'Saved.');
    } catch (problem) {
      notices.set('defaults', String(problem));
    }
    await refresh();
  };
  saved.textContent = notices.get('defaults') ?? '';
  notices.delete('defaults');

  return h(
    'section.panel.form',
    h('div.label', 'Defaults for new sessions'),
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
    field(
      'Model',
      provider
        ? radioChips({ name: 'default-model', label: 'Default model', options: modelOptions(provider, state), value: state.model, onChange: (model) => apply({ provider: provider.id, model }) })
        : hint('Models are listed once a provider has a key.'),
    ),
    field('Effort', radioChips({ name: 'default-effort', label: 'Default effort', options: effortOptions(state.effort), value: state.effort, segmented: true, onChange: (effort) => apply({ effort }) }), saved),
  );
}
