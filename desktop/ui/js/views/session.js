// The session view: the timeline, the follow-up box, and the pane beside them.

import { api } from '../api.js';
import { h, icon, spinner } from '../dom.js';
import { duration, k } from '../format.js';
import * as store from '../store.js';
import { attachments } from './attach.js';
import { barButton, chatBar, fitBox, markIdle, onSendKey, sessionState } from './common.js';
import { AUTONOMIES, effortOptions, modelPicker, plainMode, settingPicker } from './controls.js';
import { dictation } from './dictate.js';
import { mountPanel } from './panel.js';
import { openOutput } from './output.js';
import { skillCommands } from './skills.js';
import { createTimeline } from './timeline.js';

// Where each session's timeline was left, what was unfolded in it and what was typed in its box
// and not sent, so coming back loses none of them.
const places = new Map();
const unfolded = new Map();
const drafts = new Map();

export function mountSession(main, side, app, meta) {
  const entry = store.open(meta);
  const panel = mountPanel(document.getElementById('inspector-tabs'), side, { id: entry.id, title: meta.title });
  const notice = h('div.view-message', { hidden: true });
  const timelineEl = h('div.timeline');
  const scroller = h('div.timeline-scroll', notice, timelineEl);
  const jump = h('button.jump', { type: 'button', hidden: true, onclick: () => follow(true) });
  const foot = h('footer.session-foot');
  main.append(scroller, jump, foot);

  if (!unfolded.has(entry.id)) unfolded.set(entry.id, new Map());
  const timeline = createTimeline(timelineEl, {
    expanded: unfolded.get(entry.id),
    canAnswer: () => entry.live && entry.model.running,
    answer: (approval, allow) => api.approve(entry.id, approval, allow),
    // A question is answered when no run is in progress; the answer goes as the next message.
    canReply: () => !entry.model.running,
    reply: (text) => send(text),
    openOutput: (handle) => openOutput(entry.id, handle),
  });

  // Follow the newest content while the user is at the end; leave the scroll alone once they are not.
  const place = places.get(entry.id) ?? { pinned: entry.live || entry.resumable, top: 0 };
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
    oninput: () => {
      fitBox(box);
      markIdle(sendButton, box);
    },
    onkeydown: (event) => skills.key(event) || onSendKey(event, send),
  });
  box.value = drafts.get(entry.id) ?? '';
  // Skills go with the next message, as commands typed into the box. What the session has is on
  // for good, and the conventional loop takes none.
  const skills = skillCommands({
    box,
    list: () => skillList,
    usable: () => !plainMode(entry.model?.meta.mode),
    locked: () => (entry.model ? activeSkills(entry.model) : []),
    onChange: (names) => (picked = names),
  });
  const sendButton = barButton('send-followup', 'Send', icon('send', 16), send);
  // While a run is in progress the same place stops it.
  const stopButton = barButton('stop-session', 'Stop', icon('stop', 12), requestStop, 'soft');
  stopButton.hidden = true;
  const files = attachments({ start: () => entry.meta.cwd });
  const sendError = h('div.form-error', { role: 'alert', hidden: true });
  const fail = (problem) => {
    sendError.textContent = String(problem);
    sendError.hidden = false;
  };
  const mic = dictation({ box, fail });
  let sending = false;
  // Between a message being accepted and its `user` event arriving, the run has not visibly started.
  // Holds how many messages the timeline had when the last one was sent, until it shows one more.
  // Not "until a redraw sees the run": a run can start and end between two redraws.
  let awaitingRun = null;
  const messages = () => entry.model.items.filter((item) => item.kind === 'user').length;

  /**
   * Sends what is in the box, or `answer` (a question's, which leaves the box alone and throws a
   * failure back to the question's card, where it is shown).
   */
  async function send(answer) {
    const own = typeof answer === 'string';
    // A command typed out and sent is taken now. Alone in the box, it was all there was to do.
    if (!own && skills.take()) return;
    const text = own ? answer : box.value.trim();
    if (!text || sendButton.disabled || sendButton.hidden) return;
    const before = messages();
    sending = true;
    sendError.hidden = true;
    mic.stop();
    render();
    try {
      await api.sendMessage(entry.id, text, picked, changes(), files.paths());
      // A session from an earlier launch has been brought back by the message: follow it from here.
      store.resume(entry);
      if (!own) box.value = '';
      skills.clear();
      files.clear();
      fitBox(box);
      awaitingRun = before;
      pinned = true;
    } catch (problem) {
      if (own) throw problem;
      fail(problem);
    } finally {
      sending = false;
      render();
    }
  }

  // In the bar and under it, what the next message goes with. The effort, the model and the
  // autonomy show what the session has until another is picked: that goes with the next message,
  // and is the session's from then on.
  const effortSlot = h('span.contents');
  const modelSlot = h('span.contents');
  const autonomySlot = h('span.contents');
  const hint = h('span.chat-hint', 'Type / for skills');
  const swapNote = h('div.chat-note', { role: 'status', hidden: true });
  // The session's goal, while it has one: what the end of each run is held to. `/goal` sets and clears it.
  const goalText = h('span.goal-text');
  const goalNote = h('div.chat-note.goal-note', { role: 'status', hidden: true }, icon('flag', 12), goalText);
  function drawGoal(model) {
    const goal = model.meta.goal;
    goalNote.hidden = !goal?.text;
    if (!goal?.text) return;
    const text = `${goal.met ? 'Goal met' : 'Goal'}: ${goal.text}`;
    goalText.textContent = text;
    goalNote.title = text;
  }
  const wanted = { provider: null, model: null, effort: null, autonomy: null };
  // The policy as the pickers name it: the engine's `ratchet-low` is Auto.
  const policy = (effort) => (effort === 'ratchet-low' ? 'ratchet' : effort);

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
    const signature = () => [meta.provider, meta.model, meta.effort, meta.autonomy, meta.simulated, '|', ...Object.values(wanted)].join(' ');
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
    effortSlot.replaceChildren(settingPicker('effort', 'Effort', effortOptions(effort), effort, (value) => choose({ effort: value })));
    // A stand-in has no model to change.
    modelSlot.replaceChildren(...(meta.simulated ? [] : [modelPicker(app.state, wanted.provider !== null ? wanted : meta, choose, 'end')]));
    autonomySlot.replaceChildren(settingPicker('autonomy', 'Autonomy', AUTONOMIES, wanted.autonomy ?? meta.autonomy ?? 'auto', (value) => choose({ autonomy: value })));
  }

  let stopShown = '';
  function renderFoot(model) {
    // A session from an earlier launch that cannot be continued is read-only, as the inspector says.
    foot.hidden = !entry.live && !entry.resumable;
    if (foot.hidden) return;
    drawGoal(model);
    if (!foot.firstChild) {
      foot.append(chatBar({ over: [goalNote, swapNote, sendError], files, skills, box, start: effortSlot, end: [modelSlot, mic.button, stopButton, sendButton], under: [autonomySlot, hint] }));
      // As tall as a draft brought back from an earlier visit.
      fitBox(box);
    }
    if (awaitingRun !== null && messages() > awaitingRun) awaitingRun = null;
    if (!model.running) stopping = false;
    sendButton.hidden = model.running;
    sendButton.disabled = sending || awaitingRun !== null;
    stopButton.hidden = !model.running;
    stopButton.disabled = stopping;
    if (stopShown !== String(stopping)) {
      stopShown = String(stopping);
      stopButton.replaceChildren(stopping ? spinner() : icon('stop', 12));
    }
    markIdle(sendButton, box);
    hint.hidden = plainMode(model.meta.mode);
    skills.refresh();
    // Not while a menu may be open: a redraw would close it.
    if (!foot.querySelector('.pick[aria-expanded="true"]')) renderPickers(model);
  }

  const tick = () => {
    const now = Date.now();
    for (const el of main.querySelectorAll('[data-since]')) el.textContent = duration(Math.max(0, now - Number(el.dataset.since)));
  };

  let gone = false;
  let placed = false;
  let wasRunning = null;
  function render() {
    // A send or a stop that settles after the view was left has nothing to draw into.
    if (gone) return;
    const model = entry.model;
    notice.hidden = Boolean(model);
    if (!model) {
      notice.replaceChildren(
        ...(entry.error ? [h('p', entry.error), h('button.btn', { type: 'button', onclick: () => store.reload(entry) }, 'Try again')] : [h('p', 'Loading…')]),
      );
      return;
    }
    timeline.update(model.items);
    renderFoot(model);
    // A run that has just ended may have changed the files and left something to push.
    if (wasRunning !== model.running) {
      wasRunning = model.running;
      if (wasRunning === false && placed) panel.runEnded();
    }
    panel.update(model, { state: sessionState(entry.meta, entry), home: app.state.home, skills: activeSkills(model).map(skillTitle) });
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
      if (box.value.trim()) drafts.set(entry.id, box.value);
      else drafts.delete(entry.id);
      mic.stop();
      panel.unmount();
    },
  };
}
