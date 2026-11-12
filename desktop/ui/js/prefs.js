// Choices the window remembers between launches, in localStorage. Nothing here reaches the engine.

const PREFIX = 'sage.';
// Choices saved before the rename are still read, until they are next changed.
const FORMER_PREFIX = 'presage.';

export function get(key, fallback = null) {
  try {
    return localStorage.getItem(PREFIX + key) ?? localStorage.getItem(FORMER_PREFIX + key) ?? fallback;
  } catch {
    return fallback;
  }
}

export function set(key, value) {
  try {
    localStorage.setItem(PREFIX + key, value);
  } catch {
    // A profile that cannot be written keeps the defaults.
  }
}
