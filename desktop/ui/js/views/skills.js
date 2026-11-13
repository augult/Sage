// The Skills button and the dialog it opens. A skill is a page of instructions that goes to the
// model once it is switched on: with the task of a new session, or with the next message of one
// under way. The dialog lists the skills on offer as cards, two to a row: what each is for, and
// what can be done with it: use it, read it whole, copy its text.

import { copyButton, h, icon } from '../dom.js';
import { closeButton, openDialog } from './common.js';
import { prose } from './prose.js';

/**
 * The button. `skills` is the engine's list, `value` the names switched on here, `locked` the
 * names the session already has: on, and not to be switched off. `onChange(names)`.
 */
export function skillsButton({ skills, value, locked = [], disabled = false, onChange }) {
  const chosen = new Set(value);
  const isOn = (name) => chosen.has(name) || locked.includes(name);
  const count = h('span.pick-value');
  const button = h(
    'button.pick',
    { id: 'pick-skills', type: 'button', disabled: disabled || !skills.length, 'aria-haspopup': 'dialog', onclick: () => openSkills({ skills, isOn, isLocked: (name) => locked.includes(name), toggle }) },
    h('span.pick-key', 'Skills'),
    count,
  );

  function paint() {
    const on = skills.filter((skill) => isOn(skill.name));
    count.textContent = String(on.length);
    count.hidden = !on.length;
    button.title = on.length ? `Skills: ${on.map((skill) => skill.title).join(', ')}` : 'Skills';
  }

  function toggle(name) {
    if (!chosen.delete(name)) chosen.add(name);
    paint();
    onChange(skills.map((skill) => skill.name).filter((each) => chosen.has(each)));
  }

  paint();
  return button;
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
  const back = h('button.act', { type: 'button', title: 'Back', 'aria-label': 'Back', hidden: true, onclick: () => read(null) }, icon('back', 14));
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
