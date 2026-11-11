// Form controls shared by the New session and Settings views.

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
 * `groups` is `[{ label, note, options: [{ value, label, hint, disabled }] }]`; a group's label and
 * note are optional. `prefix` names the setting in front of the value, `empty` stands in when
 * nothing is chosen. `onChange(value)` runs on selection.
 */
export function picker({ id, name, prefix, groups, value, empty = 'None', onChange }) {
  let current = value;
  const options = groups.flatMap((group) => group.options);
  const labelOf = (wanted) => options.find((option) => option.value === wanted)?.label ?? empty;

  const shown = h('span.pick-value');
  const button = h(
    'button.pick',
    { id, type: 'button', 'aria-haspopup': 'listbox', 'aria-expanded': 'false', onclick: () => (menu.hidden ? open() : close(false)) },
    prefix && h('span.pick-key', prefix),
    shown,
    icon('caret', 12),
  );
  const menu = h('div.menu', { id: `${id}-menu`, role: 'listbox', 'aria-label': name, hidden: true, onkeydown: onKey });
  for (const group of groups) {
    if (group.label) menu.append(h('div.menu-group', group.label, group.note && h('span.menu-note', group.note)));
    for (const option of group.options) {
      menu.append(
        h(
          'button.menu-item',
          { type: 'button', role: 'option', disabled: option.disabled, dataset: { value: option.value }, onclick: () => choose(option.value) },
          icon('check', 12),
          h('span.menu-text', h('span', option.label), option.hint && h('span.menu-hint', option.hint)),
        ),
      );
    }
  }
  const wrap = h('div.pick-wrap', button, menu);
  const enabled = () => [...menu.querySelectorAll('.menu-item:not(:disabled)')];

  function paint() {
    shown.textContent = labelOf(current);
    button.title = `${name}: ${labelOf(current)}`;
    for (const item of menu.querySelectorAll('.menu-item')) item.setAttribute('aria-selected', String(item.dataset.value === current));
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
    current = next;
    paint();
    close(true);
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

/** A form row: label on the left, the control and an optional hint on the right. */
export function field(label, control, hint) {
  return h('div.field', h('div.field-label', label), h('div.field-body', control, hint));
}

export const hint = (...content) => h('div.field-hint', ...content);

/** Provider choices; a provider without a key is listed but cannot be picked. */
export function providerOptions(providers) {
  return providers.map((provider) => ({
    value: provider.id,
    label: provider.name,
    tag: provider.has_key ? null : 'needs key',
    disabled: !provider.has_key,
    title: provider.has_key ? provider.base_url : `Add a key for ${provider.name} in Settings, or set ${provider.key_env}`,
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
