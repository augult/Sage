// The session view: header, timeline, follow-up box, and the inspector beside it.

import { api } from '../api.js';
import { h, icon, spinner } from '../dom.js';
import { duration, tilde } from '../format.js';
import * as store from '../store.js';
import { MOD_KEY, outcomeLook, sessionState, tag } from './common.js';
import { renderInspector } from './inspector.js';
import { openOutput } from './output.js';
import { createTimeline } from './timeline.js';

// Where each session's timeline was left and what was unfolded in it, so coming back loses neither.
const places = new Map();
const unfolded = new Map();

export function mountSession(main, side, app, meta) {
  const entry = store.open(meta);
  const head = h('header.session-head', { 'data-tauri-drag-region': 'deep' });
  const notice = h('div.view-message', { hidden: true });
  const timelineEl = h('div.timeline');
  const scroller = h('div.timeline-scroll', notice, timelineEl);
  const jump = h('button.jump', { type: 'button', hidden: true, onclick: () => follow(true) });
  const foot = h('footer.session-foot');
  main.append(head, scroller, jump, foot);

  if (!unfolded.has(entry.id)) unfolded.set(entry.id, new Map());
  const timeline = createTimeline(timelineEl, {
    expanded: unfolded.get(entry.id),
    canAnswer: () => entry.live && entry.model.running,
    answer: (approval, allow) => api.approve(entry.id, approval, allow),
    openOutput: (handle) => openOutput(entry.id, handle),
  });

  // Follow the newest content while the user is at the end; leave the scroll alone once they are not.
  const place = places.get(entry.id) ?? { pinned: entry.live, top: 0 };
  let pinned = place.pinned;
  const atEnd = () => scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 32;
  function follow(force) {
    if (force) pinned = true;
    // Hide the bar first: it takes room from the scroller, which moves where the end is.
    jump.hidden = pinned || atEnd();
    if (pinned) scroller.scrollTop = scroller.scrollHeight;
    const waiting = sessionState(entry.meta, entry).approvals > 0;
    const label = waiting ? 'An approval request is waiting below' : 'Jump to the end';
    if (jump.textContent !== label) {
      jump.className = `jump tone-${waiting ? 'amber' : 'plain'}`;
      jump.replaceChildren(icon('down', 12), label);
    }
  }
  scroller.addEventListener(
    'scroll',
    () => {
      pinned = atEnd();
      jump.hidden = pinned;
    },
    { passive: true },
  );

  let stopping = false;
  let headShown = '';
  function renderHead(model) {
    const state = sessionState(entry.meta, entry);
    if (state.phase !== 'running') stopping = false;
    const { title, cwd, model: modelName, provider, effort, mode, autonomy, simulated } = model.meta;
    const shown = [state.phase, state.outcome, state.verified, state.approvals, stopping, title, cwd, modelName, provider, effort, mode, autonomy, simulated].join('|');
    if (shown === headShown) return;
    headShown = shown;
    const stop =
      state.phase === 'running' &&
      h('button.btn.danger', { id: 'stop-session', type: 'button', disabled: stopping, onclick: requestStop }, icon('stop', 12), stopping ? 'Stopping…' : 'Stop');
    head.replaceChildren(
      h('div.session-top', h('h1.session-name', { title }, title), statusPill(state, model), stop),
      h(
        'div.session-facts',
        h('span.session-cwd', { title: cwd }, h('bdi', tilde(cwd, app.state.home))),
        h(
          'div.session-tags',
          tag(modelName, 'plain', `Model, through ${provider}`),
          tag(`effort ${effort}`, 'plain', 'Reasoning effort policy'),
          // The engine also reports ablations here, as "presage without <mechanism>".
          mode === 'classic'
            ? tag('Classic', 'plain', 'Conventional tool loop: no scripts, predictions, watches or finish gate')
            : tag(mode === 'presage' ? 'Presage' : mode, 'azure', 'Scripts, predictions, watches and the evidence gate'),
          autonomy === 'ask' && tag('asks first', 'amber', 'Every step that changes something waits for your approval'),
          simulated && h('span.badge', { title: 'A scripted stand-in is playing the model: token counts and cost are fabricated.' }, 'SIMULATED'),
        ),
      ),
    );
  }

  function requestStop() {
    stopping = true;
    render();
    api.stopSession(entry.id).catch((problem) => {
      stopping = false;
      render();
      throw problem;
    });
  }

  // Follow-up box. Typing ahead is fine; sending waits for the run to end.
  const box = h('textarea.input', {
    id: 'followup',
    rows: 1,
    'aria-label': 'Follow-up message',
    title: 'The agent still has its context from this session',
    placeholder: 'Send a follow-up',
    oninput: fitBox,
    onkeydown: (event) => {
      if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
        event.preventDefault();
        send();
      }
    },
  });
  const sendButton = h('button.btn.primary', { id: 'send-followup', type: 'button', onclick: send }, 'Send');
  const sendError = h('div.form-error', { role: 'alert', hidden: true });
  let sending = false;
  // Between a message being accepted and its `user` event arriving, the run has not visibly started.
  let awaitingRun = false;

  function fitBox() {
    box.style.height = 'auto';
    box.style.height = `${Math.min(box.scrollHeight + 2, 160)}px`;
  }

  async function send() {
    const text = box.value.trim();
    if (!text || sendButton.disabled) return;
    sending = true;
    sendError.hidden = true;
    render();
    try {
      await api.sendMessage(entry.id, text);
      box.value = '';
      fitBox();
      awaitingRun = true;
      pinned = true;
    } catch (problem) {
      sendError.textContent = String(problem);
      sendError.hidden = false;
    }
    sending = false;
    render();
  }

  function renderFoot(model) {
    if (!foot.firstChild) {
      foot.append(
        entry.live
          ? h('div.composer', h('div.composer-box', sendError, box), h('span.kbd', `${MOD_KEY}+Enter`), sendButton)
          : h('p.foot-note', 'Read-only replay. This session ran in an earlier launch of the app, so it cannot be continued.'),
      );
    }
    if (model.running) awaitingRun = false;
    const busy = model.running || sending || awaitingRun;
    sendButton.disabled = busy;
    sendButton.title = busy ? 'A follow-up can be sent when this run ends' : '';
  }

  const tick = () => {
    const now = Date.now();
    for (const el of main.querySelectorAll('[data-since]')) el.textContent = duration(Math.max(0, now - Number(el.dataset.since)));
  };

  let sideShown = null;
  let placed = false;
  function render() {
    const model = entry.model;
    notice.hidden = Boolean(model);
    if (!model) {
      notice.replaceChildren(
        ...(entry.error ? [h('p', entry.error), h('button.btn', { type: 'button', onclick: () => store.reload(entry) }, 'Try again')] : [h('p', 'Loading the session…')]),
      );
      return;
    }
    renderHead(model);
    timeline.update(model.items);
    renderFoot(model);
    if (sideShown?.model !== model || sideShown.rev !== model.sideRev) {
      sideShown = { model, rev: model.sideRev };
      renderInspector(side, model);
    }
    if (!placed && !pinned) scroller.scrollTop = place.top;
    placed = true;
    follow();
    tick();
  }

  let frame = 0;
  const unlisten = store.onChange((changed) => {
    if (changed === entry) frame ||= requestAnimationFrame(() => ((frame = 0), render()));
  });
  const ticker = setInterval(tick, 200);
  render();

  return {
    unmount() {
      unlisten();
      clearInterval(ticker);
      cancelAnimationFrame(frame);
      places.set(entry.id, { pinned, top: scroller.scrollTop });
    },
  };
}

function statusPill(state, model) {
  if (state.approvals > 0) return h('span.status-pill.tone-amber', icon('bang'), 'Waiting for your approval');
  if (state.phase === 'running') return h('span.status-pill.tone-azure', spinner(), 'Running', h('span', { dataset: { since: model.runStartedAt } }));
  const look = outcomeLook(state.outcome, state.verified);
  const label = state.outcome ? look.label : state.phase === 'closed' ? 'Ended without a result' : 'Idle';
  return h(`span.status-pill.tone-${look.tone}`, icon(look.icon), label, state.phase === 'closed' && tag('read-only', 'plain', 'From an earlier launch of the app'));
}
