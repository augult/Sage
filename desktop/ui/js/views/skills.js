// Skills in a chat bar. A skill is a page of instructions that goes to the model once it is
// switched on: with the task of a new session, or with the next message of one under way. A bar
// has no button for them: they are commands, typed at the start of its box. `/` lists them by
// name, `/<name>` switches one on, and `/skills` opens the dialog, which shows the skills on offer
// as cards, two to a row: what each is for, and what can be done with it: use it, read it whole,
// copy its text. Those switched on for the message sit in the bar over its text, each a chip with
// a cross that takes it off.

import { copyButton, h, icon } from '../dom.js';
import { chip } from './attach.js';
import { closeButton, openDialog } from './common.js';
import { prose } from './prose.js';

// The command that opens the dialog. A skill that has taken the name goes by its card there.
const BROWSE = 'skills';

/**
 * The skills of the chat bar whose text box is `box`. `list()` is the engine's list as it stands,
 * `usable()` whether the session takes skills at all (the conventional loop takes none), and
 * `locked()` the names the session already has: on, and not to be switched off. `value` is the
 * names switched on here; `onChange(names)` runs when they change.
 *
 * Gives `menu` and `chips` for the bar to hold; `key(event)` for the box's keydown, true when the
 * key was the menu's; `take()` for a box about to be sent, true when a command was all there was
 * in it; `remove(sent)` for skills sent with a message; and `refresh()` for when the list, the
 * lock or the mode may have changed.
 */
export function skillCommands({ box, list, usable = () => true, locked = () => [], value = [], onChange }) {
  let chosen = [...value];
  const offered = () => (usable() ? list() : []);
  const isLocked = (name) => locked().includes(name);
  const isOn = (name) => chosen.includes(name) || isLocked(name);
  const browse = () => openSkills({ skills: offered(), isOn, isLocked, toggle });

  const chips = h('div.attached', { hidden: true });
  // A press on the menu leaves the focus in the box.
  const menu = h('div.menu.slash-menu', { id: `${box.id}-commands`, role: 'listbox', 'aria-label': 'Commands', hidden: true, onmousedown: (event) => event.preventDefault() });
  box.setAttribute('aria-controls', menu.id);

  // What the menu lists, the word it lists them for, which of them the arrow keys are on, and the
  // text Escape closed it over: it stays closed until the text is another. It is closed, too,
  // while the focus is elsewhere.
  let listed = [];
  let drawn = null;
  let active = 0;
  let dismissed = null;
  let away = false;
  // What `refresh` last drew for.
  let seen = null;

  function paint() {
    const on = list().filter((skill) => chosen.includes(skill.name));
    chips.hidden = !usable() || !on.length;
    chips.replaceChildren(
      ...on.map((skill) => {
        const el = chip('slash', skill.title, skill.description, () => toggle(skill.name));
        el.dataset.skill = skill.name;
        return el;
      }),
    );
  }

  /** Tells whoever keeps them: in the list's order, whatever order they were switched on in. */
  const report = () => onChange?.(list().map((skill) => skill.name).filter((name) => chosen.includes(name)));

  function toggle(name) {
    if (isLocked(name)) return;
    chosen = chosen.includes(name) ? chosen.filter((each) => each !== name) : [...chosen, name];
    paint();
    report();
  }

  /** Sets the box's text as typing would: whoever keeps it hears of it, and the menu follows. */
  function setBox(text) {
    box.value = text;
    box.dispatchEvent(new Event('input', { bubbles: true }));
  }

  /** The commands a box that holds `/` and then `typed` may mean. */
  function commands(typed) {
    const skills = offered();
    // Without a list the dialog would have nothing to show either.
    if (!skills.length) return [];
    const all = [{ name: BROWSE, about: 'Browse the skills on offer' }, ...skills.filter((skill) => skill.name !== BROWSE).map((skill) => ({ name: skill.name, about: skill.description, skill }))];
    return all.filter((command) => command.name.startsWith(typed));
  }

  const row = (command, at) =>
    h(
      'button.menu-item',
      {
        id: `${menu.id}-${at}`,
        type: 'button',
        role: 'option',
        tabindex: '-1',
        'data-tooltip': command.about,
        'data-tooltip-overflow': '',
        disabled: Boolean(command.skill) && isLocked(command.name),
        'aria-selected': String(Boolean(command.skill) && isOn(command.name)),
        dataset: { command: command.name },
        onmousemove: () => {
          if (active === at) return;
          active = at;
          mark();
        },
        onclick: () => accept(command),
      },
      h('span.slash-name', `/${command.name}`),
      h('span.slash-about', command.about),
      icon('check', 12),
    );

  /** Shows which row the arrow keys are on, to the eye and to a screen reader, and keeps it in view. */
  function mark() {
    const rows = [...menu.querySelectorAll('.menu-item')];
    rows.forEach((item, at) => item.classList.toggle('is-active', at === active));
    const current = rows[active];
    if (!current) return box.removeAttribute('aria-activedescendant');
    box.setAttribute('aria-activedescendant', current.id);
    if (current.offsetTop < menu.scrollTop) menu.scrollTop = current.offsetTop;
    else if (current.offsetTop + current.offsetHeight > menu.scrollTop + menu.clientHeight) menu.scrollTop = current.offsetTop + current.offsetHeight - menu.clientHeight;
  }

  /** The menu is open while the box holds one word that starts with `/`, and nothing else. */
  function draw() {
    const typed = /^\/(\S*)$/.exec(box.value)?.[1];
    if (box.value !== dismissed) dismissed = null;
    const open = typed !== undefined && dismissed === null && !away;
    listed = open ? commands(typed) : [];
    // Another word is another list: the arrow keys start from its top.
    if (typed !== drawn) active = 0;
    drawn = open ? typed : null;
    active = Math.max(0, Math.min(active, listed.length - 1));
    // Where no skills are taken the menu says so, in place of the ones the word could have meant.
    const refused = open && !usable() && [BROWSE, ...list().map((skill) => skill.name)].some((name) => name.startsWith(typed));
    menu.replaceChildren(...(refused ? [h('div.menu-note', 'Default mode takes no skills')] : listed.map(row)));
    menu.hidden = !refused && !listed.length;
    mark();
  }

  function accept(command) {
    // The command was all there was in the box.
    setBox('');
    if (command.skill) toggle(command.name);
    else browse();
  }

  /**
   * Takes the commands `text` starts with off it: a skill's name switches the skill on, `/skills`
   * opens the dialog. While typing, a command is one a space has ended; `whole` also takes the
   * one the text ends with, as sending does. Gives what is left of the text.
   */
  function strip(text, whole) {
    let rest = text;
    let dialog = false;
    for (;;) {
      const [found, name, after] = /^\s*\/([a-z0-9-]+)(\s+|$)/.exec(rest) ?? [];
      if (!found || (!after && !whole)) break;
      const skills = offered();
      if (name === BROWSE && skills.length) dialog = true;
      else if (!skills.some((skill) => skill.name === name)) break;
      else if (!isOn(name)) toggle(name);
      rest = rest.slice(found.length);
    }
    if (dialog) browse();
    return rest;
  }

  /** Takes the commands that typing has ended, and draws the menu for what is left. */
  function follow() {
    const rest = strip(box.value, false);
    // Set again, the box says so itself, and the menu is drawn then.
    if (rest !== box.value) setBox(rest);
    else draw();
  }

  box.addEventListener('input', follow);
  // The menu is the box's while the box has the focus (a press on the menu leaves it there).
  for (const [type, left] of [['focus', false], ['blur', true]]) {
    box.addEventListener(type, () => {
      away = left;
      draw();
    });
  }
  paint();

  return {
    menu,
    chips,
    key(event) {
      if (menu.hidden) return false;
      if (event.key === 'Escape') dismissed = box.value;
      else if (!listed.length) return false;
      else if (event.key === 'ArrowDown') active = (active + 1) % listed.length;
      else if (event.key === 'ArrowUp') active = (active + listed.length - 1) % listed.length;
      // Enter that ends an IME composition belongs to the composition.
      else if ((event.key !== 'Enter' && event.key !== 'Tab') || event.isComposing || event.shiftKey) return false;
      event.preventDefault();
      if (event.key === 'Enter' || event.key === 'Tab') accept(listed[active]);
      else if (event.key === 'Escape') draw();
      else mark();
      return true;
    },
    take() {
      const rest = strip(box.value, true);
      if (rest === box.value) return false;
      setBox(rest);
      return !rest.trim();
    },
    remove(sent) {
      const remaining = chosen.filter((name) => !sent.includes(name));
      if (remaining.length === chosen.length) return;
      chosen = remaining;
      paint();
      report();
    },
    refresh() {
      const known = list();
      const state = JSON.stringify([usable(), known.map((skill) => [skill.name, skill.title]), locked()]);
      if (state === seen) return;
      seen = state;
      // Another folder may not have what was picked, and what the session has taken is its own now.
      // A list that has not come yet says nothing about either.
      const kept = chosen.filter((name) => !isLocked(name) && (!known.length || known.some((skill) => skill.name === name)));
      const fewer = kept.length < chosen.length;
      chosen = kept;
      paint();
      // A command typed before its skill was on offer is taken now.
      follow();
      if (fewer) report();
    },
  };
}

function openSkills({ skills, isOn, isLocked, toggle }) {
  // Every Use button there is, on a card or over the skill being read: they change together.
  const marks = [];
  const useButton = (skill) => {
    const button = h('button.btn.skill-use', {
      type: 'button',
      disabled: isLocked(skill.name),
      onclick: () => {
        toggle(skill.name);
        for (const mark of marks) mark();
      },
    });
    const mark = () => {
      const on = isOn(skill.name);
      button.setAttribute('aria-pressed', String(on));
      button.classList.toggle('primary', on);
      button.replaceChildren(...(on ? [icon('check', 12), 'In use'] : ['Use']));
    };
    marks.push(mark);
    mark();
    return button;
  };

  const title = h('h2', 'Skills');
  const back = h('button.act', { type: 'button', 'data-tooltip': 'Back', 'aria-label': 'Back', hidden: true, onclick: () => read(null) }, icon('back', 14));
  const tools = h('span.skill-tools', { hidden: true });
  const page = h('div.skill-page', { hidden: true });
  const grid = h(
    'div.skill-grid',
    skills.map((skill) =>
      h(
        'article.skill-card',
        { dataset: { skill: skill.name } },
        h('h3.skill-title', skill.title),
        h('p.skill-about', skill.description),
        h('div.skill-acts', useButton(skill), h('button.btn.quiet.skill-view', { type: 'button', onclick: () => read(skill) }, 'View'), copyButton(() => skill.body)),
      ),
    ),
  );

  // The View button the skill being read was opened from: the focus goes back to it.
  let opener = null;

  /** Shows `skill` whole, in place of the cards; with none, the cards again. */
  function read(skill) {
    title.textContent = skill ? skill.title : 'Skills';
    back.hidden = tools.hidden = page.hidden = !skill;
    grid.hidden = Boolean(skill);
    // What had the focus is hidden now, which would leave the focus nowhere.
    if (!skill) return opener?.focus();
    opener = document.activeElement;
    tools.replaceChildren(useButton(skill), copyButton(() => skill.body));
    page.replaceChildren(prose(skill.body));
    page.scrollTop = 0;
    back.focus();
  }

  openDialog('skills', (close) => [h('div.dialog-head', back, title, tools, closeButton(close)), grid, page]);
}
