// The session view: header, timeline, follow-up box, and the inspector beside it.

import { api } from '../api.js';
import { h, icon, spinner } from '../dom.js';
import { duration, effortLabel, k, tilde } from '../format.js';
import * as store from '../store.js';
import { chatBar, fitBox, onSendKey, outcomeLook, sessionState } from './common.js';
import { AUTONOMIES, effortOptions, modelPicker, settingPicker } from './controls.js';
import { renderInspector } from './inspector.js';
import { openOutput } from './output.js';
import { openPush } from './push.js';
import { skillsButton } from './skills.js';
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
    if (jump.textContent !== label) jump.replaceChildren(icon('down', 12), label);
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

  // Where the workspace stands with git, for the Push button: read on arrival and after every run.
  let git = null;
  let gitRead = 0;
  let gone = false;
  function readGit() {
    const asked = ++gitRead;
    api.gitState(entry.id).then(
      (state) => {
        if (gone || asked !== gitRead) return;
        git = state;
        render();
      },
      // Without it there is no button; the session is whole all the same.
      () => {},
    );
  }

  let headShown = '';
  function renderHead(model) {
    const state = sessionState(entry.meta, entry);
    const { title, cwd, model: modelName, provider, effort, mode, autonomy, simulated } = model.meta;
    const skills = activeSkills(model).map(skillTitle);
    const waitingToPush = git?.repo ? git.files.length + git.ahead : 0;
    const shown = [state.phase, state.outcome, state.verified, state.approvals, title, cwd, modelName, provider, effort, mode, autonomy, simulated, git?.repo, git?.github, waitingToPush, ...skills].join('|');
    if (shown === headShown) return;
    headShown = shown;
    // Only for a folder that is a git repository; where the push goes is the dialog's to say.
    const push =
      git?.repo &&
      h(
        'button.btn',
        { id: 'push', type: 'button', onclick: () => openPush({ id: entry.id, title }, readGit) },
        icon(git.github ? 'github' : 'push', 14),
        git.github ? 'Push to GitHub' : 'Push',
        waitingToPush > 0 && h('span.btn-count', String(waitingToPush)),
      );
    // With what the session runs, as plain words after the folder.
    const facts = [
      modelName,
      `effort ${effortLabel(effort)}`,
      // The engine also reports ablations here, as "presage without <mechanism>".
      mode === 'classic' ? 'Classic' : mode === 'presage' ? 'Presage' : mode,
      skills.length > 0 && `${skills.length > 1 ? 'skills' : 'skill'} ${skills.join(', ')}`,
      autonomy === 'ask' && 'asks first',
      simulated && 'simulated',
    ].filter(Boolean);
    head.replaceChildren(
      h('div.session-top', h('h1.session-name', { title }, title), statusPill(state, model), push),
      h(
        'div.session-facts',
        h('span.session-cwd', { title: cwd }, h('bdi', tilde(cwd, app.state.home))),
        h('span.session-meta', { title: `${provider} · ${facts.join(' · ')}` }, `· ${facts.join(' · ')}`),
      ),
    );
  }

  let stopping = false;
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
  const box = h('textarea', {
    id: 'followup',
    rows: 1,
    'aria-label': 'Follow-up message',
    placeholder: 'Follow-up',
    oninput: () => fitBox(box),
    onkeydown: (event) => onSendKey(event, send),
  });
  const sendButton = h('button.btn.primary', { id: 'send-followup', type: 'button', onclick: send }, 'Send');
  // While a run is in progress the same place stops it.
  const stopButton = h('button.btn', { id: 'stop-session', type: 'button', hidden: true, onclick: requestStop });
  const sendError = h('div.form-error', { role: 'alert', hidden: true });
  let sending = false;
  // Between a message being accepted and its `user` event arriving, the run has not visibly started.
  // Holds how many messages the timeline had when the last one was sent, until it shows one more.
  // Not "until a redraw sees the run": a run can start and end between two redraws.
  let awaitingRun = null;
  const messages = () => entry.model.items.filter((item) => item.kind === 'user').length;

  async function send() {
    const text = box.value.trim();
    if (!text || sendButton.disabled || sendButton.hidden) return;
    const before = messages();
    sending = true;
    sendError.hidden = true;
    render();
    try {
      await api.sendMessage(entry.id, text, picked, changes());
      box.value = '';
      picked = [];
      fitBox(box);
      awaitingRun = before;
      pinned = true;
    } catch (problem) {
      sendError.textContent = String(problem);
      sendError.hidden = false;
    }
    sending = false;
    render();
  }

  // Under the box, what the next message goes with. The model, the effort and the autonomy show
  // what the session has until another is picked: that goes with the next message, and is the
  // session's from then on. Of the skills, what the session has is on for good.
  const pickSlot = h('span.contents');
  const swapNote = h('div.chat-note', { role: 'status', hidden: true });
  const wanted = { provider: null, model: null, effort: null, autonomy: null };
  // The policy as the pickers name it: the engine's `ratchet-high` is Auto.
  const policy = (effort) => (effort === 'ratchet-high' ? 'ratchet' : effort);

  /** What the next message would change: only what differs from what the session has. */
  function changes() {
    const { provider, model, effort, autonomy } = entry.model.meta;
    const next = {};
    if (wanted.provider !== null && (wanted.provider !== provider || wanted.model !== model)) Object.assign(next, { provider: wanted.provider, model: wanted.model });
    if (wanted.effort !== null && wanted.effort !== policy(effort)) next.effort = wanted.effort;
    if (wanted.autonomy !== null && wanted.autonomy !== (autonomy ?? 'auto')) next.autonomy = wanted.autonomy;
    return next;
  }

  /** What a change will cost that a message otherwise does not: said over the box, before it is sent. */
  function drawNote() {
    const next = changes();
    // No stats until the first answer has come.
    const size = entry.model.stats?.context_tokens ?? 0;
    const whole = `the whole conversation${size > 0 ? `: ${k(size)} tokens` : ''}`;
    const text = next.provider ? `${next.model ?? 'The new model'} will read ${whole}, none of it cached` : next.effort ? `On some providers a new effort rereads ${whole}` : '';
    swapNote.textContent = text;
    swapNote.hidden = !text;
  }

  let pickersShown = null;
  function renderPickers(model) {
    const { meta } = model;
    const locked = activeSkills(model);
    // The classic baseline takes no skills.
    const offered = meta.mode === 'classic' ? [] : skillList;
    picked = picked.filter((name) => !locked.includes(name));
    const signature = () => [meta.provider, meta.model, meta.effort, meta.autonomy, meta.mode, meta.simulated, offered.length, ...locked, '|', ...picked, '|', ...Object.values(wanted)].join(' ');
    drawNote();
    if (signature() === pickersShown) return;
    pickersShown = signature();
    // A choice made here is shown by its own picker: no redraw, which would take the focus from it.
    const choose = (change) => {
      Object.assign(wanted, change);
      pickersShown = signature();
      drawNote();
    };
    const effort = wanted.effort ?? policy(meta.effort);
    pickSlot.replaceChildren(
      ...[
        // A stand-in has no model to change.
        !meta.simulated && modelPicker(app.state, wanted.provider !== null ? wanted : meta, choose),
        offered.length > 0 && skillsButton({ skills: offered, value: picked, locked, onChange: (names) => ((picked = names), choose({})) }),
        settingPicker('effort', 'Effort', effortOptions(effort), effort, (value) => choose({ effort: value })),
        settingPicker('autonomy', 'Autonomy', AUTONOMIES, wanted.autonomy ?? meta.autonomy ?? 'auto', (value) => choose({ autonomy: value })),
      ].filter(Boolean),
    );
  }

  let stopShown = '';
  function renderFoot(model) {
    // A session from an earlier launch cannot be continued: its header says read-only.
    foot.hidden = !entry.live;
    if (!entry.live) return;
    if (!foot.firstChild) foot.append(chatBar([swapNote, sendError], box, [stopButton, sendButton], pickSlot));
    if (awaitingRun !== null && messages() > awaitingRun) awaitingRun = null;
    if (!model.running) stopping = false;
    sendButton.hidden = model.running;
    sendButton.disabled = sending || awaitingRun !== null;
    stopButton.hidden = !model.running;
    stopButton.disabled = stopping;
    if (stopShown !== String(stopping)) {
      stopShown = String(stopping);
      stopButton.replaceChildren(icon('stop', 16), stopping ? 'Stopping…' : 'Stop');
    }
    // Not while a menu or the Skills dialog may be open: a redraw would close the one and strand the other.
    if (!pickSlot.querySelector('[aria-expanded="true"]') && !document.querySelector('dialog.skills')) renderPickers(model);
  }

  const tick = () => {
    const now = Date.now();
    for (const el of main.querySelectorAll('[data-since]')) el.textContent = duration(Math.max(0, now - Number(el.dataset.since)));
  };

  let sideShown = null;
  let placed = false;
  let wasRunning = null;
  function render() {
    const model = entry.model;
    notice.hidden = Boolean(model);
    if (!model) {
      notice.replaceChildren(
        ...(entry.error ? [h('p', entry.error), h('button.btn', { type: 'button', onclick: () => store.reload(entry) }, 'Try again')] : [h('p', 'Loading…')]),
      );
      return;
    }
    // A run that has just ended may have left something to push.
    if (wasRunning !== model.running) {
      wasRunning = model.running;
      if (!model.running) readGit();
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
      if (gone) return;
      skillList = listed;
      render();
    },
    // Without the list, skills go by their names.
    () => {},
  );

  return {
    unmount() {
      gone = true;
      unlisten();
      clearInterval(ticker);
      cancelAnimationFrame(frame);
      places.set(entry.id, { pinned, top: scroller.scrollTop });
    },
  };
}

function statusPill(state, model) {
  if (state.approvals > 0) return h('span.status-pill', icon('bang'), 'Waiting for your approval');
  if (state.phase === 'running') return h('span.status-pill', spinner(), 'Running', h('span', { dataset: { since: model.runStartedAt } }));
  const look = outcomeLook(state.outcome, state.verified);
  const label = state.outcome ? look.label : state.phase === 'closed' ? 'Ended without a result' : 'Idle';
  return h('span.status-pill', icon(look.icon), label, state.phase === 'closed' && h('span.muted', '· read-only'));
}
