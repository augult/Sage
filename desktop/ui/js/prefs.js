// Choices the window remembers between launches, in localStorage. Nothing here reaches the engine.

const PREFIX = 'presage.';

export function get(key, fallback = null) {
  try {
    return localStorage.getItem(PREFIX + key) ?? fallback;
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
