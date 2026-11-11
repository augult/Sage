// The session view: header, timeline, follow-up box, and the inspector beside it.

import { api } from '../api.js';
import { h, icon, spinner } from '../dom.js';
import { duration, tilde } from '../format.js';
import * as store from '../store.js';
import { MOD_KEY, outcomeLook, sessionState, tag } from './common.js';
import { effortLabel, skillsPicker } from './controls.js';
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
    const label = waiting ? 'Approval needed' : 'Jump to the end';
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

  // The skills on offer in this workspace, for their titles and for the follow-up box.
  let skillList = [];
  let picked = [];
  const activeSkills = (model) => (Array.isArray(model.meta.skills) ? model.meta.skills : []);
  const skillTitle = (name) => skillList.find((skill) => skill.name === name)?.title ?? name;

  let stopping = false;
  let headShown = '';
  function renderHead(model) {
    const state = sessionState(entry.meta, entry);
    if (state.phase !== 'running') stopping = false;
    const { title, cwd, model: modelName, provider, effort, mode, autonomy, simulated } = model.meta;
    const skills = activeSkills(model).map(skillTitle);
    const shown = [state.phase, state.outcome, state.verified, state.approvals, stopping, title, cwd, modelName, provider, effort, mode, autonomy, simulated, ...skills].join('|');
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
          tag(modelName, 'plain', provider),
          skills.length > 0 && tag(`${skills.length > 1 ? 'skills' : 'skill'} ${skills.join(', ')}`),
          tag(`effort ${effortLabel(effort)}`),
          // The engine also reports ablations here, as "presage without <mechanism>".
          mode === 'classic' ? tag('Classic') : tag(mode === 'presage' ? 'Presage' : mode, 'azure'),
          autonomy === 'ask' && tag('asks first', 'amber'),
          simulated && h('span.badge', 'SIMULATED'),
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
    placeholder: 'Follow-up',
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
      await api.sendMessage(entry.id, text, picked);
      box.value = '';
      picked = [];
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

  // The Skills button of the follow-up box: what the session has is on for good, the rest can be
  // switched on with the next message.
  const skillsSlot = h('span.pick-slot');
  let slotShown = null;
  function renderSkillsSlot(model) {
    const locked = activeSkills(model);
    // The classic baseline takes no skills.
    const offered = model.meta.mode === 'classic' ? [] : skillList;
    picked = picked.filter((name) => !locked.includes(name));
    const signature = () => [offered.length, ...locked, '|', ...picked].join(' ');
    if (signature() === slotShown) return;
    slotShown = signature();
    const choose = (names) => {
      picked = names;
      slotShown = signature();
    };
    skillsSlot.replaceChildren(...(offered.length ? [skillsPicker({ skills: offered, value: picked, locked, align: 'end', onChange: choose })] : []));
  }

  function renderFoot(model) {
    // A session from an earlier launch cannot be continued: its header says read-only.
    foot.hidden = !entry.live;
    if (!entry.live) return;
    if (!foot.firstChild) foot.append(h('div.composer', h('div.composer-box', sendError, box), skillsSlot, h('span.kbd', `${MOD_KEY}+Enter`), sendButton));
    if (model.running) awaitingRun = false;
    sendButton.disabled = model.running || sending || awaitingRun;
    // Not while its menu may be open: a redraw would close it.
    if (!skillsSlot.querySelector('[aria-expanded="true"]')) renderSkillsSlot(model);
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
        ...(entry.error ? [h('p', entry.error), h('button.btn', { type: 'button', onclick: () => store.reload(entry) }, 'Try again')] : [h('p', 'Loading…')]),
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
  api.skills(entry.meta.cwd).then(
    (listed) => {
      skillList = listed;
      render();
    },
    // Without the list, skills go by their names.
    () => {},
  );

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
  return h(`span.status-pill.tone-${look.tone}`, icon(look.icon), label, state.phase === 'closed' && tag('read-only'));
}
