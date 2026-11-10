// Number and time formatting, matching the CLI so both front ends read the same.

export function k(n) {
  if (n < 1_000) return String(n);
  if (n < 100_000) return `${(n / 1e3).toFixed(1)}k`;
  if (n < 1_000_000) return `${Math.floor(n / 1_000)}k`;
  return `${(n / 1e6).toFixed(2)}M`;
}

export const pct = (part, whole) => (whole > 0 ? `${Math.round((part * 100) / whole)}%` : '–');

export function duration(ms) {
  if (ms < 10_000) return `${(ms / 1000).toFixed(1)}s`;
  if (ms < 60_000) return `${Math.floor(ms / 1000)}s`;
  return `${Math.floor(ms / 60_000)}m${String(Math.floor((ms % 60_000) / 1000)).padStart(2, '0')}s`;
}

export function bytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function ago(ts, now = Date.now()) {
  const minutes = Math.floor((now - ts) / 60_000);
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m`;
  if (minutes < 60 * 24) return `${Math.floor(minutes / 60)}h`;
  if (minutes < 60 * 24 * 7) return `${Math.floor(minutes / (60 * 24))}d`;
  return new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** Shortens a path under the home directory to `~/…`. */
export function tilde(path, home) {
  if (!home || !path) return path ?? '';
  if (path === home) return '~';
  return path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path;
}

export const firstLine = (text) => String(text ?? '').split('\n', 1)[0];

export const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;
