// Form controls shared by the New session, Session and Settings views.

import { h, icon } from '../dom.js';
import { effortLabel } from '../format.js';

/**
 * A radio group drawn as chips. `options` is `[{ value, label, tag, disabled, title }]`;
 * `segmented` joins the chips into one control. `onChange(value)` runs on selection.
 */
export function radioChips({ name, label, options, value, onChange, segmented = false }) {
  const group = h(`div.chips${segmented ? '.segmented' : ''}`, { role: 'radiogroup', 'aria-label': label });
  for (const option of options) {
    const input = h('input', {
      type: 'radio',
      name,
      value: option.value,
      checked: option.value === value,
      disabled: option.disabled,
      onchange: () => onChange(option.value),
    });
    const face = h('span.chip-face', option.label, option.tag && h('span.chip-tag', option.tag));
    group.append(h('label.chip-option', { title: option.title }, input, face));
  }
  return group;
}

/**
 * A button that shows the current choice and opens a menu of the others.
 * `groups` is `[{ label, options: [{ value, label, title, disabled }] }]`; a group's label is
 * optional, and groups without one are parted by a rule. `glyph` is an icon and `prefix` the
 * setting's name in front of the value, `empty` stands in when nothing is chosen. `boxed` draws
 * the button as a field, for a form. `onChange(value)` runs on selection.
 */
export function picker({ id, name, glyph, prefix, groups, value, empty = 'None', onChange, disabled = false, align, boxed = false }) {
  const options = groups.flatMap((group) => group.options);
  let chosen = value;

  const shown = h('span.pick-value');
  const button = h(
    `button.pick${boxed ? '.boxed' : ''}`,
    { id, type: 'button', disabled: disabled || !options.length, 'aria-haspopup': 'listbox', 'aria-expanded': 'false', onclick: () => (menu.hidden ? open() : close(false)) },
    glyph && icon(glyph, 14),
    prefix && h('span.pick-key', prefix),
    shown,
    icon('caret', 12),
  );
  const menu = h('div.menu', { id: `${id}-menu`, role: 'listbox', 'aria-label': name, hidden: true, onkeydown: onKey });
  for (const group of groups) {
    if (group.label) menu.append(h('div.menu-group', group.label));
    else if (menu.childElementCount && group.options.length) menu.append(h('div.menu-rule'));
    for (const option of group.options) {
      menu.append(
        h(
          'button.menu-item',
          { type: 'button', role: 'option', title: option.title || null, disabled: option.disabled, dataset: { value: option.value }, onclick: () => choose(option.value) },
          h('span.menu-text', option.label),
          icon('check', 12),
        ),
      );
    }
  }
  const wrap = h(`div.pick-wrap${align === 'end' ? '.align-end' : ''}`, button, menu);
  const enabled = () => [...menu.querySelectorAll('.menu-item:not(:disabled)')];

  function paint() {
    const on = options.find((option) => option.value === chosen);
    shown.textContent = on ? on.label : empty;
    shown.hidden = !shown.textContent;
    shown.classList.toggle('is-empty', !on);
    button.title = on ? `${name}: ${on.label}` : name;
    button.setAttribute('aria-label', button.title);
    for (const item of menu.querySelectorAll('.menu-item')) item.setAttribute('aria-selected', String(chosen != null && item.dataset.value === String(chosen)));
  }

  const outside = (event) => {
    if (!wrap.contains(event.target)) close(false);
  };

  function open() {
    menu.hidden = false;
    button.setAttribute('aria-expanded', 'true');
    // Above the button when there is no room below it.
    wrap.classList.toggle('opens-up', innerHeight - button.getBoundingClientRect().bottom < menu.offsetHeight + 12);
    (menu.querySelector('.menu-item[aria-selected="true"]:not(:disabled)') ?? enabled()[0])?.focus();
    document.addEventListener('pointerdown', outside, true);
  }

  function close(refocus) {
    if (menu.hidden) return;
    menu.hidden = true;
    button.setAttribute('aria-expanded', 'false');
    document.removeEventListener('pointerdown', outside, true);
    if (refocus) button.focus();
  }

  function choose(next) {
    chosen = next;
    close(true);
    paint();
    onChange(next);
  }

  function onKey(event) {
    const items = enabled();
    const at = items.indexOf(document.activeElement);
    const to = { ArrowDown: at + 1, ArrowUp: at - 1, Home: 0, End: items.length - 1 }[event.key];
    if (event.key === 'Escape') {
      event.stopPropagation();
      close(true);
    } else if (event.key === 'Tab') {
      close(false);
    } else if (to !== undefined && items.length) {
      event.preventDefault();
      items[(to + items.length) % items.length].focus();
    }
  }

  paint();
  return wrap;
}

/** What a session can be run for. `title` is shown on hover. */
export const MODES = [
  { value: 'ecosage', label: 'Ecosage', title: 'Few requests and little money, at a little cost to the result' },
  { value: 'default', label: 'Default', title: 'The loop other harnesses run: one tool call after another, nothing of Sage around it' },
  { value: 'prosage', label: 'Prosage', title: 'For the result alone: nothing hurries the run, examiners who never see the work check it against the task, referees decide which of their findings stand, skills are switched on unasked. Costs more requests and time' },
];

/** The conventional loop, which takes no skills. Sessions stored before it was renamed call it `classic`. */
export const plainMode = (mode) => mode === 'default' || mode === 'classic';

/** A session's mode as its record spells it (`prosage without examiner`), with the mode's name as the app shows it. */
export function modeLabel(mode) {
  const [name, ...rest] = String(mode ?? '').split(' ');
  const known = MODES.find((option) => option.value === (name === 'classic' ? 'default' : name));
  return known ? [known.label, ...rest].join(' ') : mode;
}

export const AUTONOMIES = [
  { value: 'auto', label: 'Auto' },
  { value: 'ask', label: 'Ask' },
];

/** A run setting shows only its value; its accessible label and tooltip retain the setting name. */
export function settingPicker(key, name, options, value, onChange) {
  return picker({ id: `pick-${key}`, name, groups: [{ options }], value, onChange });
}

/**
 * The model, as one choice with the provider that serves it: models are listed under their
 * providers, and only providers that are connected (have a key) are listed. The provider in use
 * stays, greyed, if its key has gone since, so that the choice still shows. With none connected,
 * the picker says where to add one. `current` is `{ provider, model }`; `onChange({ provider, model })`.
 * `align` is the picker's: `end` where it stands at the right of its row.
 */
export function modelPicker(state, current, onChange, align) {
  const served = new Map();
  const listed = connected(state.providers, current.provider);
  const groups = keyedFirst(listed).map((provider) => {
    const models = modelOptions(provider, state);
    // The one in use is listed even where the provider's own list does not carry it.
    if (provider.id === current.provider && current.model && !models.some((option) => option.value === current.model)) models.push({ value: current.model, label: current.model });
    // A provider that lists no models still has its own default.
    const options = (models.length ? models : [{ value: null, label: 'Provider default' }]).map((option) => {
      const value = `${provider.id}:${option.value ?? ''}`;
      served.set(value, { provider: provider.id, model: option.value });
      return { value, label: option.label, disabled: !provider.has_key };
    });
    return { label: provider.name, options };
  });
  const empty = listed.length ? 'No model yet' : 'Add an API key in Settings';
  return picker({ id: 'pick-model', name: 'Model', groups, value: current.provider ? `${current.provider}:${current.model ?? ''}` : null, empty, align, onChange: (value) => onChange(served.get(value)) });
}

/** The providers that have a key, and the one named `keep` (the one in use) whether it has or not. */
export const connected = (providers, keep = null) => providers.filter((provider) => provider.has_key || provider.id === keep);

/** A form row: label on the left, the control and an optional status line on the right. */
export function field(label, control, status) {
  return h('div.field', h('div.field-label', label), h('div.field-body', control, status));
}

export const hint = (...content) => h('div.field-hint', ...content);

/** Providers with those that have a key ahead of those that have none, each group in its own order. */
export const keyedFirst = (providers) => [...providers].sort((a, b) => Number(b.has_key) - Number(a.has_key));

/** Provider choices: the connected ones, and `keep` (the one chosen now), greyed if it has no key. */
export function providerOptions(providers, keep = null) {
  return keyedFirst(connected(providers, keep)).map((provider) => ({ value: provider.id, label: provider.name, disabled: !provider.has_key, title: provider.has_key ? provider.base_url : 'No API key' }));
}

/** Efforts on offer, plus the configured default when it is a variant not in the list. */
export function effortOptions(current) {
  const values = ['ratchet', 'low', 'high', 'max'];
  if (current && !values.includes(current)) values.push(current);
  return values.map((value) => {
    const label = effortLabel(value);
    return { value, label: label.charAt(0).toUpperCase() + label.slice(1) };
  });
}

/** A provider's models, plus the saved default when it is one the list does not carry. */
export function modelOptions(provider, state) {
  const models = [...provider.models];
  if (provider.id === state.provider && state.model && !models.includes(state.model)) models.push(state.model);
  return models.map((value) => ({ value, label: value }));
}
