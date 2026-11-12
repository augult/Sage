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

/** Shortens a path under the home directory to `~/…`. */
/** A Windows path without the verbatim prefix its system gives it: `\\?\C:\work` as `C:\work`. */
export function plainPath(path) {
  const text = String(path ?? '');
  if (text.startsWith('\\\\?\\UNC\\')) return `\\\\${text.slice(8)}`;
  return /^\\\\\?\\[A-Za-z]:/.test(text) ? text.slice(4) : text;
}

export function tilde(path, home) {
  const [plain, base] = [plainPath(path), plainPath(home)];
  if (!base || !plain) return plain;
  if (plain === base) return '~';
  return plain.startsWith(base) && /[\\/]/.test(plain[base.length]) ? `~${plain.slice(base.length)}` : plain;
}

/** An effort as the interface names it. The engine's `ratchet` starts high and steps up when predictions fail: Auto. */
export function effortLabel(effort) {
  const text = String(effort ?? '');
  const start = /^ratchet(?:[-:](.+))?$/.exec(text);
  if (!start) return text;
  return !start[1] || start[1] === 'high' ? 'auto' : `auto from ${start[1]}`;
}

export const firstLine = (text) => String(text ?? '').split('\n', 1)[0];

export const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;
