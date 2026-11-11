// Form controls shared by the New session, Session and Settings views.

import { h, icon } from '../dom.js';

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
 * optional. `prefix` names the setting in front of the value, `empty` stands in when nothing is
 * chosen. `onChange(value)` runs on selection.
 *
 * With `multiple`, `value` is an array, an option toggles and the menu stays open. `locked` values
 * are on and cannot be switched off; `onChange` gets the others that are on.
 */
export function picker({ id, name, prefix, groups, value, empty = 'None', onChange, multiple = false, locked = [], disabled = false, align }) {
  const options = groups.flatMap((group) => group.options);
  const chosen = new Set(multiple ? value : value == null ? [] : [value]);
  const isOn = (wanted) => chosen.has(wanted) || locked.includes(wanted);

  const shown = h('span.pick-value');
  const button = h(
    'button.pick',
    { id, type: 'button', disabled: disabled || !options.length, 'aria-haspopup': 'listbox', 'aria-expanded': 'false', onclick: () => (menu.hidden ? open() : close(false)) },
    prefix && h('span.pick-key', prefix),
    shown,
    icon('caret', 12),
  );
  const menu = h('div.menu', { id: `${id}-menu`, role: 'listbox', 'aria-label': name, 'aria-multiselectable': multiple && 'true', hidden: true, onkeydown: onKey });
  for (const group of groups) {
    if (group.label) menu.append(h('div.menu-group', group.label));
    for (const option of group.options) {
      menu.append(
        h(
          'button.menu-item',
          { type: 'button', role: 'option', title: option.title || null, disabled: option.disabled || locked.includes(option.value), dataset: { value: option.value }, onclick: () => choose(option.value) },
          icon('check', 12),
          h('span.menu-text', option.label),
        ),
      );
    }
  }
  const wrap = h(`div.pick-wrap${align === 'end' ? '.align-end' : ''}`, button, menu);
  const enabled = () => [...menu.querySelectorAll('.menu-item:not(:disabled)')];

  function paint() {
    const on = options.filter((option) => isOn(option.value));
    // A labelled multi-select shows how many are on: the names would not fit beside its neighbours.
    const several = multiple && prefix ? String(on.length) : on[0]?.label + (on.length > 1 ? ` +${on.length - 1}` : '');
    shown.textContent = on.length ? several : empty;
    shown.hidden = !shown.textContent;
    shown.classList.toggle('is-empty', !on.length);
    button.title = on.length ? `${name}: ${on.map((option) => option.label).join(', ')}` : name;
    for (const item of menu.querySelectorAll('.menu-item')) item.setAttribute('aria-selected', String(isOn(item.dataset.value)));
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
    if (!multiple) {
      chosen.clear();
      chosen.add(next);
      close(true);
    } else if (!chosen.delete(next)) {
      chosen.add(next);
    }
    paint();
    onChange(multiple ? options.map((option) => option.value).filter((option) => chosen.has(option)) : next);
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

/**
 * The Skills button. `skills` is the engine's list, `value` the names switched on here, `locked`
 * the names the session already has.
 */
export function skillsPicker({ skills, value, locked = [], disabled = false, align, onChange }) {
  const options = skills.map((skill) => ({ value: skill.name, label: skill.title, title: skill.description }));
  return picker({ id: 'pick-skills', name: 'Skills', prefix: 'Skills', groups: [{ options }], value, locked, empty: '', multiple: true, disabled, align, onChange });
}

/** A form row: label on the left, the control and an optional status line on the right. */
export function field(label, control, status) {
  return h('div.field', h('div.field-label', label), h('div.field-body', control, status));
}

export const hint = (...content) => h('div.field-hint', ...content);

/** Provider choices; a provider without a key is listed but cannot be picked. */
export function providerOptions(providers) {
  return providers.map((provider) => ({
    value: provider.id,
    label: provider.name,
    tag: provider.has_key ? null : 'needs key',
    disabled: !provider.has_key,
    title: provider.base_url,
  }));
}

/** Efforts on offer, plus the configured default when it is a variant not in the list. */
export function effortOptions(current) {
  const values = ['ratchet', 'low', 'high', 'max'];
  if (current && !values.includes(current)) values.push(current);
  return values.map((value) => ({ value, label: value }));
}

/** A provider's models, plus the saved default when it is one the list does not carry. */
export function modelOptions(provider, state) {
  const models = [...provider.models];
  if (provider.id === state.provider && state.model && !models.includes(state.model)) models.push(state.model);
  return models.map((value) => ({ value, label: value }));
}
