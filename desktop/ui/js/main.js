const out = document.getElementById('out');
const { invoke, Channel } = window.__TAURI__.core;
try {
  const hooks = await invoke('ui_hooks');
  const meta = await invoke('start_demo');
  const channel = new Channel();
  let live = 0;
  channel.onmessage = (ev) => { live += 1; out.textContent = `hooks ${JSON.stringify(hooks)}\nsession ${meta.id}\nlive events ${live} last ${ev.type} seq ${ev.seq}`; };
  const backlog = await invoke('subscribe', { id: meta.id, onEvent: channel });
  out.textContent += `\nbacklog ${backlog.length}`;
} catch (e) { out.textContent = `error: ${e}`; }
