// The right-hand column: Economy, Watches, and Notes and lessons for the open session; a short
// legend when no session is open.

import { h, icon } from '../dom.js';
import { bytes, duration, k, pct, plural } from '../format.js';
import { known, tag } from './common.js';

export function renderInspector(el, model) {
  el.replaceChildren(economy(model), watches(model), notes(model));
}

export function renderLegend(el) {
  const entry = (mark, term, meaning) => [h('div.legend-mark', mark), h('div', h('b', term), h('span.muted', meaning))];
  el.replaceChildren(
    section(
      'Reading a session',
      null,
      h(
        'div.legend',
        entry(tag('expect exit 0', 'azure'), 'Prediction', 'What the model says a step will do, before it runs.'),
        entry(tag('held', 'teal'), 'Held', 'Reality matched. The model gets a one-line receipt instead of the output.'),
        entry(tag('surprise', 'amber'), 'Surprise', 'Reality differed. The script halts there and the model sees the evidence.'),
        entry(tag('skipped'), 'Skipped', 'The rest of a halted script, finish included. Nothing after a surprise runs.'),
        entry(tag('error', 'red'), 'Error', 'A step that could not be carried out, or a run that failed.'),
      ),
    ),
    section('Inspector', null, h('p.side-note', 'Economy, watches, notes and lessons appear here when a session is open.')),
  );
}

function section(title, badge, ...content) {
  return h('section.side-section', h('div.side-title', h('span.label', title), badge), ...content);
}

/** One line of the Economy block: label, value, an optional muted qualifier that wraps as a unit. */
function stat(label, value, sub, tone) {
  return h('div.stat', h('span.stat-label', label), h(`span.stat-value${tone ? `.tone-${tone}` : ''}`, String(value)), sub && h('span.stat-sub', sub));
}

/** A share bar: `part` of `whole` in one tone, the remainder in another. */
function bar(part, whole, tone, restTone = 'plain') {
  if (!(whole > 0)) return null;
  const share = Math.max(0, Math.min(100, (part * 100) / whole));
  return h('div.bar', { 'aria-hidden': 'true' }, h(`span.tone-${tone}`, { style: `width:${share}%` }), h(`span.tone-${restTone}`, { style: `width:${100 - share}%` }));
}

function economy(model) {
  const s = model.stats;
  const simulated = model.meta.simulated === true;
  const badge = simulated && h('span.badge', { title: 'A scripted stand-in played the model: token counts and cost are fabricated.' }, 'SIMULATED');
  if (!s) return section('Economy', badge, h('p.side-note', model.running ? 'Numbers appear after the first turn.' : 'No numbers: this session ended before its first turn was complete.'));

  const cost = simulated ? stat('Cost', 'fabricated', 'simulated run', 'amber') : s.priced ? stat('Cost', `$${s.cost.toFixed(4)}`, 'at list price') : stat('Cost', 'no price table for this model', null, 'plain');
  return section(
    'Economy',
    badge,
    stat('Turns', s.turns),
    stat('Steps', s.steps, s.skipped > 0 && `${s.skipped} skipped`),
    stat('Surprises', s.surprises, null, s.surprises > 0 && 'amber'),
    stat('Predictions', s.predictions > 0 ? `${s.predictions_held} of ${s.predictions} held` : 'none made'),
    bar(s.predictions_held, s.predictions, 'teal', 'amber'),
    stat('Prompt tokens', k(s.prompt_tokens), s.prompt_tokens > 0 && `${pct(s.cached_tokens, s.prompt_tokens)} cached`),
    bar(s.cached_tokens, s.prompt_tokens, 'teal'),
    stat('Output tokens', k(s.output_tokens), s.output_tokens > 0 && `${pct(s.reasoning_tokens, s.output_tokens)} thinking`),
    bar(s.reasoning_tokens, s.output_tokens, 'azure'),
    cost,
    stat('Context', `${k(s.context_tokens)} tokens`),
    stat('Time', duration(s.elapsed_ms), `model ${duration(s.model_ms)} · tools ${duration(s.tool_ms)}`),
    bar(s.model_ms, s.model_ms + s.tool_ms, 'azure', 'teal'),
    stat('Kept out of context', bytes(s.hidden_bytes)),
    stat('Repairs', s.repairs),
    stat('Retries', s.retries),
    stat('Cache misses', s.cache_misses, s.cache_busts > 0 && `${plural(s.cache_busts, 'bust')} by the harness`),
  );
}

const WATCH_LOOKS = { pass: ['check', 'teal', 'passing'], fail: ['bang', 'amber', 'failing'] };

function watches(model) {
  if (!model.watches.size) return section('Watches', null, h('p.side-note', 'No watches. A watch is a check the harness re-runs after every change.'));
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
  const entries = [...model.lessons.map((lesson) => ({ ...lesson, tone: 'teal' })), ...model.notes.map((note) => ({ ...note, tone: 'azure' }))];
  if (!entries.length) return section('Notes and lessons', null, h('p.side-note', 'Nothing yet. What the harness tells the model, and the error → fix pairs it learns, collect here.'));
  return section(
    'Notes and lessons',
    null,
    entries.map((entry) => h('div.note', h(`span.note-label.tone-${entry.tone}`, entry.label), h('div.note-body', entry.text))),
  );
}
