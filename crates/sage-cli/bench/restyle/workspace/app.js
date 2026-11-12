'use strict';

// Behaviour of the settings page. Elements are found by id and by data-* attribute only, so the
// markup around them is free to change.

(function () {
  const byId = (id) => document.getElementById(id);

  const form = byId('settings-form');
  const save = byId('save');
  const reset = byId('reset');
  const status = byId('status');
  const fields = ['display-name', 'email', 'timezone', 'notify', 'bio'].map(byId);
  const valueOf = (field) => (field.type === 'checkbox' ? field.checked : field.value);
  let saved = fields.map(valueOf);

  function refreshSave() {
    const dirty = fields.some((field, index) => valueOf(field) !== saved[index]);
    save.disabled = !dirty;
    reset.disabled = !dirty;
  }

  function say(message) {
    status.textContent = message;
  }

  form.addEventListener('input', () => {
    say('');
    refreshSave();
  });
  form.addEventListener('change', refreshSave);
  form.addEventListener('submit', (event) => {
    event.preventDefault();
    if (!/^\S+@\S+\.\S+$/.test(byId('email').value)) {
      say('Enter a valid contact email.');
      return;
    }
    saved = fields.map(valueOf);
    refreshSave();
    say('Saved.');
  });
  reset.addEventListener('click', (event) => {
    event.preventDefault();
    fields.forEach((field, index) => {
      if (field.type === 'checkbox') field.checked = saved[index];
      else field.value = saved[index];
    });
    refreshSave();
    say('Changes discarded.');
  });

  // Members.
  const members = byId('members');
  const filter = byId('member-filter');
  const count = byId('member-count');

  function refreshMembers() {
    const needle = filter.value.trim().toLowerCase();
    let shown = 0;
    members.querySelectorAll('[data-member]').forEach((row) => {
      const match = row.dataset.member.toLowerCase().includes(needle);
      row.hidden = !match;
      if (match) shown += 1;
    });
    count.textContent = shown === 1 ? '1 member' : `${shown} members`;
  }

  filter.addEventListener('input', refreshMembers);
  members.addEventListener('click', (event) => {
    const remove = event.target.closest('[data-action="remove"]');
    if (!remove) return;
    const row = remove.closest('[data-member]');
    say(`Removed ${row.dataset.member}.`);
    row.remove();
    refreshMembers();
  });

  // Invite dialog.
  const dialog = byId('invite-dialog');
  const inviteEmail = byId('invite-email');
  const inviteSend = byId('invite-send');

  function refreshInvite() {
    inviteSend.disabled = !/^\S+@\S+\.\S+$/.test(inviteEmail.value);
  }

  function openDialog() {
    inviteEmail.value = '';
    refreshInvite();
    dialog.hidden = false;
    if (typeof dialog.showModal === 'function') dialog.showModal();
    inviteEmail.focus();
  }

  function closeDialog() {
    if (typeof dialog.close === 'function') dialog.close();
    dialog.hidden = true;
    byId('invite-open').focus();
  }

  byId('invite-open').addEventListener('click', openDialog);
  byId('invite-cancel').addEventListener('click', closeDialog);
  inviteEmail.addEventListener('input', refreshInvite);
  inviteSend.addEventListener('click', () => {
    say(`Invitation sent to ${inviteEmail.value}.`);
    closeDialog();
  });
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !dialog.hidden) closeDialog();
  });

  refreshSave();
  refreshMembers();
})();
