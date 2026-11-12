// The Economy tab of the pane beside the conversation: what the session is and how it stands, then
// Economy, Watches, and Notes.

import { h, icon } from '../dom.js';
import { bytes, duration, effortLabel, k, pct, plural, tilde } from '../format.js';
import { known, outcomeLook, tag } from './common.js';
import { modeLabel } from './controls.js';

/** `about` is `{ state, home, skills }`: the session's state now, the home folder, its skills by title. */
export function renderInspector(el, model, about) {
  el.replaceChildren(session(model, about), economy(model), watches(model), notes(model));
}

function section(title, badge, ...content) {
  return h('section.side-section', h('div.side-title', h('span.label', title), badge), ...content);
}

/** One line of the Economy block: label, value, an optional muted qualifier that wraps as a unit. */
function stat(label, value, sub, tone) {
  return h('div.stat', h('span.stat-label', label), h(`span.stat-value${tone ? `.tone-${tone}` : ''}`, String(value)), sub && h('span.stat-sub', sub));
}

/** A share bar, as a ruler of ticks: `part` of `whole` in one tone, the remainder in another. */
function bar(part, whole, tone, restTone = 'plain') {
  if (!(whole > 0)) return null;
  const share = Math.max(0, Math.min(100, (part * 100) / whole));
  return h('div.bar', { 'aria-hidden': 'true' }, h(`span.tone-${tone}`, { style: `width:${share}%` }), h(`span.tone-${restTone}`, { style: `width:${100 - share}%` }));
}

/** How the session stands, in words. */
export function standing(state) {
  if (state.approvals > 0) return 'Waiting for your approval';
  if (state.phase === 'running') return 'Running';
  const words = state.outcome ? outcomeLook(state.outcome, state.verified).label : state.phase === 'closed' ? 'Ended without a result' : 'Idle';
  return state.phase === 'closed' ? `${words} · read-only` : words;
}

/** One line of what the session is: a name, and a value that keeps to its line. A path keeps its end. */
function fact(label, value, path = false) {
  return h('div.fact', h('span.fact-label', label), h(`span.fact-value${path ? '.is-path' : ''}`, { title: value }, path ? h('bdi', value) : value));
}

function session(model, { state, home, skills }) {
  const { title, cwd, model: name, effort, mode, autonomy } = model.meta;
  return section(
    'Session',
    null,
    h('div.side-name', title),
    fact('Status', standing(state)),
    // A session that cannot be continued says why, in a word.
    state.phase === 'closed' && state.why && fact('Why', state.why),
    fact('Folder', tilde(cwd, home), true),
    fact('Model', name),
    fact('Effort', effortLabel(effort).replace(/^./, (first) => first.toUpperCase())),
    // The engine also reports ablations here, as "presage without <mechanism>".
    fact('Mode', modeLabel(mode)),
    fact('Autonomy', autonomy === 'ask' ? 'Ask' : 'Auto'),
    skills.length > 0 && fact('Skills', skills.join(', ')),
  );
}

function economy(model) {
  const s = model.stats;
  const simulated = model.meta.simulated === true;
  const badge = simulated && h('span.badge', 'Simulated');
  if (!s) return section('Economy', badge, h('p.side-note', 'No data'));

  const cost = simulated ? stat('Cost', 'fabricated', null, 'alert') : s.priced ? stat('Cost', `$${s.cost.toFixed(4)}`, 'at list price') : stat('Cost', 'unpriced', null, 'plain');
  return section(
    'Economy',
    badge,
    stat('Turns', s.turns),
    s.goal_checks > 0 && stat('Goal checks', s.goal_checks),
    stat('Steps', s.steps, s.skipped > 0 && `${s.skipped} skipped`),
    stat('Surprises', s.surprises, null, s.surprises > 0 && 'alert'),
    stat('Predictions', s.predictions > 0 ? `${s.predictions_held} of ${s.predictions} held` : 'none made'),
    bar(s.predictions_held, s.predictions, 'good', 'alert'),
    stat('Prompt tokens', k(s.prompt_tokens), s.prompt_tokens > 0 && `${pct(s.cached_tokens, s.prompt_tokens)} cached`),
    bar(s.cached_tokens, s.prompt_tokens, 'good'),
    stat('Output tokens', k(s.output_tokens), s.output_tokens > 0 && `${pct(s.reasoning_tokens, s.output_tokens)} thinking`),
    bar(s.reasoning_tokens, s.output_tokens, 'quiet'),
    cost,
    stat('Context', `${k(s.context_tokens)} tokens`),
    stat('Time', duration(s.elapsed_ms), `model ${duration(s.model_ms)} · tools ${duration(s.tool_ms)}`),
    bar(s.model_ms, s.model_ms + s.tool_ms, 'quiet', 'good'),
    stat('Kept out of context', bytes(s.hidden_bytes)),
    stat('Repairs', s.repairs),
    stat('Retries', s.retries),
    stat('Cache misses', s.cache_misses, s.cache_busts > 0 && plural(s.cache_busts, 'bust')),
  );
}

const WATCH_LOOKS = { pass: ['check', 'good', 'passing'], fail: ['bang', 'alert', 'failing'] };

function watches(model) {
  if (!model.watches.size) return section('Watches', null, h('p.side-note', 'No watches'));
  return section(
    'Watches',
    null,
    [...model.watches.values()].map((watch) => {
      const [mark, tone, word] = known(WATCH_LOOKS, watch.state) ?? ['dash', 'plain', 'not run'];
      return h(
        'div.watch',
        h(`span.glyph.tone-${tone}`, { role: 'img', 'aria-label': word }, icon(mark)),
        h(
          'div.watch-body',
          h('div.watch-name.mono', watch.name, tag(word, tone)),
          h('div.watch-cmd.mono', watch.cmd),
          watch.detail && h('div.watch-detail', { title: watch.detail }, watch.detail.replace(/^watch [^:]*:\s*/, '')),
        ),
      );
    }),
  );
}

function notes(model) {
  const entries = model.notes;
  if (!entries.length) return section('Notes', null, h('p.side-note', 'None'));
  return section(
    'Notes',
    null,
    entries.map((entry) => h('div.note', h('span.note-label', entry.label), h('div.note-body', entry.text))),
  );
}
