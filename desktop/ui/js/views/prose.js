// What a model wrote, set as prose: the tree js/markdown.js reads, built into elements. Every piece
// of text becomes a text node. A link is shown and can be copied, never followed: the window has
// one page, and a click must not replace it with a web site.

import { copyButton, copyText, h } from '../dom.js';
import { parse } from '../markdown.js';

const INLINE = {
  text: (node) => node.text.split('\n').flatMap((line, i) => (i ? [h('br'), line] : [line])),
  code: (node) => h('code', node.text),
  strong: (node) => h('strong', spans(node.children)),
  em: (node) => h('em', spans(node.children)),
  strike: (node) => h('s', spans(node.children)),
  link: (node) =>
    h(
      'span.link',
      {
        role: 'button',
        tabindex: '0',
        title: `${node.href}\nClick to copy`,
        onclick: () => copyText(node.href),
        onkeydown: (event) => {
          if (event.key === 'Enter') copyText(node.href);
        },
      },
      spans(node.children),
    ),
};

const spans = (nodes) => nodes.map((node) => INLINE[node.type](node));

/** A fenced block: its language and a copy button above the code. */
export function codeBlock(text, lang = '') {
  return h('div.code', h('div.code-head', h('span.code-lang', lang || 'text'), copyButton(() => text)), h('pre', h('code', text)));
}

const BLOCKS = {
  paragraph: (node) => h('p', spans(node.children)),
  // A reply is not a document: its headings step down so that none outranks the session's title.
  heading: (node) => h(`h${Math.min(node.level + 2, 6)}`, spans(node.children)),
  code: (node) => codeBlock(node.text, node.lang),
  rule: () => h('hr'),
  quote: (node) => h('blockquote', node.children.map(block)),
  list: (node) => h(node.ordered ? 'ol' : 'ul', { start: node.ordered && node.start !== 1 ? node.start : null }, node.items.map((item) => h('li', item.map(block)))),
  table: (node) =>
    h(
      'div.table-wrap',
      h(
        'table',
        h('thead', h('tr', node.head.map((cell, i) => h('th', { align: node.align[i] }, spans(cell))))),
        h('tbody', node.rows.map((row) => h('tr', row.map((cell, i) => h('td', { align: node.align[i] }, spans(cell)))))),
      ),
    ),
};

const block = (node) => BLOCKS[node.type](node);

/** `text` as prose. `caret` puts the mark of text still arriving at its end. */
export function prose(text, { caret = false } = {}) {
  const el = h('div.prose', parse(text).map(block));
  if (caret) {
    // After the last words, wherever they are: in a paragraph, in a list item, or at the very start.
    let last = el;
    while (last.lastElementChild && /^(P|UL|OL|LI|BLOCKQUOTE|H[3-6])$/.test(last.lastElementChild.tagName)) last = last.lastElementChild;
    last.append(h('span.caret'));
  }
  return el;
}
