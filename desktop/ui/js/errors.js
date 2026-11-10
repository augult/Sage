// Loaded before the app modules, as a classic script, so that a failure anywhere (including a
// module that does not load) still explains itself on screen instead of leaving a blank window.
(() => {
  const banner = document.getElementById('error-banner');
  const lines = document.getElementById('error-lines');
  const MAX_LINES = 4;

  function show(text) {
    const line = document.createElement('div');
    line.textContent = text;
    lines.append(line);
    while (lines.childElementCount > MAX_LINES) lines.firstElementChild.remove();
    banner.hidden = false;
  }

  // Capturing, because failed script and stylesheet loads do not bubble.
  window.addEventListener(
    'error',
    (event) => {
      if (event instanceof ErrorEvent) {
        const where = event.filename ? ` (${event.filename.split('/').pop()}:${event.lineno})` : '';
        show(`${event.message}${where}`);
      } else if (event.target?.src || event.target?.href) {
        show(`Could not load ${event.target.src || event.target.href}`);
      }
    },
    true,
  );
  window.addEventListener('unhandledrejection', (event) => show(String(event.reason?.message ?? event.reason)));

  document.getElementById('error-dismiss').addEventListener('click', () => {
    lines.replaceChildren();
    banner.hidden = true;
  });
})();
