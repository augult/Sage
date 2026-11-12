'use strict';

// entries -> { total, days, byProject: { name: minutes } }
function summarize(entries) {
  const byProject = {};
  const days = new Set();
  let total = 0;
  for (const entry of entries) {
    byProject[entry.project] = (byProject[entry.project] || 0) + entry.minutes;
    days.add(entry.date);
    total += entry.minutes;
  }
  return { total, days: days.size, byProject };
}

// The n projects with the most time, as [name, minutes], longest first; ties by name.
function topProjects(summary, n) {
  return Object.entries(summary.byProject)
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, n);
}

module.exports = { summarize, topProjects };
