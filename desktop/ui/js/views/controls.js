// Form controls shared by the New session and Settings views.

import { h } from '../dom.js';

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
