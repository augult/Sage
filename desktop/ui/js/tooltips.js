// One delegated tooltip for the whole window. No native title attributes are
// left behind, including titles supplied later by rendered Markdown or plugins.
const GAP = 7;
const INSET = 8;
const DELAY = 350;
const tip = document.createElement('div');
tip.id = 'ui-tooltip';
tip.className = 'ui-tooltip';
tip.setAttribute('role', 'tooltip');
tip.hidden = true;
const topLayer = typeof tip.showPopover === 'function';
if (topLayer) tip.setAttribute('popover', 'manual');
let owner = null;
let timer = null;
let described = null;

export function dismissTooltip() {
  clearTimeout(timer);
  timer = null;
  if (described) {
    const ids = (described.getAttribute('aria-describedby') || '').split(/\s+/).filter(id => id && id !== tip.id);
    if (ids.length) described.setAttribute('aria-describedby', ids.join(' '));
    else described.removeAttribute('aria-describedby');
  }
  described = null;
  owner = null;
  if (topLayer && tip.matches(':popover-open')) tip.hidePopover();
  tip.hidden = true;
}

function textFor(el) {
  if (!el?.isConnected || el.closest('[inert], [hidden]') || !el.getClientRects().length) return '';
  const text = el.dataset.tooltip?.trim();
  if (!text || (el.hasAttribute('aria-haspopup') && el.getAttribute('aria-expanded') === 'true')) return '';
  if (el.dataset.tooltipWhen === 'icon' && el.querySelector('.tab-label')?.getBoundingClientRect().width) return '';
  if (el.hasAttribute('data-tooltip-overflow')) {
    const candidates = [el, ...el.querySelectorAll('*')];
    if (!candidates.some(node => node.clientWidth > 0 && (node.scrollWidth > node.clientWidth + 1 || node.scrollHeight > node.clientHeight + 1))) return '';
  }
  return text;
}

function show(el) {
  const text = textFor(el);
  if (owner !== el || !text) return dismissTooltip();
  // Staying in the active dialog also keeps the description out of inert content.
  (el.closest('dialog[open]') || document.body).append(tip);
  tip.textContent = text;
  tip.hidden = false;
  if (topLayer) tip.showPopover();
  // Measure after entering the top layer, then flip and clamp both axes.
  const rect = el.getBoundingClientRect();
  const box = tip.getBoundingClientRect();
  const x = Math.max(INSET, Math.min(rect.left + (rect.width - box.width) / 2, innerWidth - box.width - INSET));
  const above = rect.top - box.height - GAP;
  const below = rect.bottom + GAP;
  const y = above >= INSET ? above : Math.min(below, innerHeight - box.height - INSET);
  tip.style.left = `${x}px`;
  tip.style.top = `${Math.max(INSET, y)}px`;
  const focused = document.activeElement;
  const control = el.contains(focused) && focused.matches('input, button, a, [tabindex]') ? focused : el;
  const ids = new Set((control.getAttribute('aria-describedby') || '').split(/\s+/).filter(Boolean));
  ids.add(tip.id);
  control.setAttribute('aria-describedby', [...ids].join(' '));
  described = control;
}

function schedule(el, delay = DELAY) {
  if (el === owner) return;
  dismissTooltip();
  if (!textFor(el)) return;
  owner = el;
  timer = setTimeout(() => show(el), delay);
}

const trigger = node => node instanceof Element ? node.closest('[data-tooltip]') : null;
document.addEventListener('pointerover', event => {
  if (event.pointerType === 'touch') return;
  const el = trigger(event.target);
  if (el && !el.contains(event.relatedTarget)) schedule(el);
});
document.addEventListener('pointerout', event => {
  if (owner?.contains(event.target) && !owner.contains(event.relatedTarget)) dismissTooltip();
});
document.addEventListener('focusin', event => {
  const el = trigger(event.target);
  if (el && event.target.matches(':focus-visible')) schedule(el, 0);
});
document.addEventListener('focusout', dismissTooltip);
document.addEventListener('pointerdown', dismissTooltip, true);
document.addEventListener('click', dismissTooltip, true);
document.addEventListener('scroll', dismissTooltip, true);
document.addEventListener('keydown', event => { if (event.key === 'Escape') dismissTooltip(); });
window.addEventListener('blur', dismissTooltip);
window.addEventListener('resize', dismissTooltip);

function migrate(el) {
  if (!(el instanceof Element)) return;
  if (el.hasAttribute('title')) {
    const text = el.getAttribute('title');
    if (text && !el.hasAttribute('data-tooltip')) el.dataset.tooltip = text;
    el.removeAttribute('title');
  }
  // Icon actions must keep their name even when no tooltip is visible.
  if (el.hasAttribute('data-tooltip') && el.matches('button, a, [role="button"]') && !el.hasAttribute('aria-label') && !el.textContent.trim()) el.setAttribute('aria-label', el.dataset.tooltip);
}
function scan(root) {
  migrate(root);
  root.querySelectorAll?.('[title], [data-tooltip]').forEach(migrate);
}
scan(document);
new MutationObserver(records => {
  for (const record of records) {
    if (record.type === 'attributes') migrate(record.target);
    else record.addedNodes.forEach(scan);
  }
  if (owner && !textFor(owner)) dismissTooltip();
  else if (described && tip.textContent !== textFor(owner)) dismissTooltip();
}).observe(document.documentElement, { subtree: true, childList: true, attributes: true, attributeFilter: ['title', 'data-tooltip', 'hidden', 'disabled', 'aria-expanded'] });
