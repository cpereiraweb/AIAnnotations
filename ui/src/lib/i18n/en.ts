// English texts: direct translations of the v1 pt-BR texts (Inv §1). `{name}` is a parameter;
// an object picks the plural form of `{count}` with Intl.PluralRules.

export type Plural = { one: string; other: string };
export type Entry = string | Plural;

export const en = {
  // Layout
  'nav.today': '☀ Today',
  'nav.projects': '▦ Projects',
  'nav.sessions': '☰ Sessions',
  'nav.main': 'Main navigation',
  'nav.search': 'search',

  // Toasts and errors
  'toast.copied': 'command copied',
  'toast.copy_failed': 'could not copy the command',
  'toast.briefing_running': 'The briefing is already being generated.',
  'toast.import_running': 'The history is already being imported.',
  'toast.import_started': 'Importing history…',
  'error.generic': 'Something went wrong: {message}',

  // Today: briefing
  'briefing.title': 'Briefing for {date}',
  'briefing.generating': 'generating…',
  'briefing.failed': 'Could not generate the briefing right now.',
  'briefing.regenerate': 'regenerate',
  'briefing.paused': 'Summaries paused: the briefing is not generated on its own. Use “regenerate” to generate it now.',

  // Today: ongoings
  'ongoing.title': 'In progress · {count}',
  'ongoing.none_recent': 'No ongoing changed in the last {days} days.',
  'ongoing.stale': 'Untouched for more than {days} days · {count}',

  // Today: inbox
  'inbox.title': 'Needs you · {count}',
  'inbox.empty': 'Nothing pending. Good job.',
  'inbox.older': 'Untouched for longer · {count}',
  'inbox.dismiss': 'seen',
  'inbox.dismiss_all': 'seen all',
  'inbox.ahead': { one: '{count} commit ahead, not merged', other: '{count} commits ahead, not merged' },

  // Today: next steps, backfill, empty database
  'today.next_steps': 'Suggested next steps',
  'today.backfill': 'history: {done}/{total} summarized',
  'today.empty_db': 'No sessions yet.',
  'today.import': 'Import history',

  // Inbox badges
  'badge.interrupted': 'interrupted',
  'badge.half_done': 'half done',
  'badge.stale_branch': { one: 'not merged for {count} day', other: 'not merged for {count} days' },
  'badge.uncommitted_changes': 'uncommitted changes',
  'badge.worktree': 'idle worktree',
  'badge.reminder': 'reminder',
  'badge.ongoing': 'ongoing',

  // Inbox group counters, in CountKey order
  'counts.interrupted': { one: '{count} interrupted', other: '{count} interrupted' },
  'counts.half_done': { one: '{count} half done', other: '{count} half done' },
  'counts.ongoing': { one: '{count} ongoing', other: '{count} ongoings' },
  'counts.reminder': { one: '{count} reminder', other: '{count} reminders' },
  'counts.stale_branch': { one: '{count} branch', other: '{count} branches' },
  'counts.worktree': { one: '{count} worktree', other: '{count} worktrees' },
  'counts.dirty': { one: '{count} with changes', other: '{count} with changes' },

  // Run card (Inv §1.5)
  'run.changed_files': { one: '✎ changed {count} file', other: '✎ changed {count} files' },
  'run.changed_files_unknown': '✎ changed files',
  'run.talk': '💬 talk only',
  'run.next_step': 'Next step: {step}',
  'run.summary_pending': 'summary pending',
  'run.summary_failed': 'summary unavailable',
  'hidden.empty': 'empty',
  'hidden.automation': 'automation',
  'hidden.subagent': 'subagent',
  'hidden.internal': 'internal',
  'end.in_progress': 'in progress',
  'end.exit': '/exit',
  'end.clear': '/clear',
  'end.logout': 'logout',
  'end.idle': 'no /exit (idle)',
  'end.clean': 'ended',
  'end.killed': 'Ctrl+C/closed',
  'interval.today': 'today',
  'interval.yesterday': 'yesterday',

  // Buttons
  'action.resume': '▶ resume',
  'action.resume_tool': '▶ resume ({tool})',
  'action.open_folder': 'open folder',

  // Projects (Inv §1.2, §1.3)
  'projects.title': 'Projects',
  'projects.empty': 'No project registered yet.',
  'projects.ongoing_count': { one: '{count} ongoing in progress', other: '{count} ongoings in progress' },
  'project.ongoing': '🔄 In progress · {count}',
  'project.sessions': 'Sessions',
  'project.show_hidden': 'show hidden sessions',
  'project.no_sessions': 'No session recorded in this project.',
  'project.not_found': 'Project not found.',
  'project.clear_chain': 'continued via /clear',

  // Pager
  'pager.label': 'Pagination',
  'pager.newer': '← newer',
  'pager.page': 'page {page} of {pages}',
  'pager.older': 'older →',

  // Sessions (Inv §1.4)
  'sessions.title': 'Sessions',
  'sessions.empty': 'No session found.',
  'sessions.tool': 'Tool',
  'sessions.project': 'Project',
  'sessions.kind': 'Kind',
  'sessions.all_tools': 'all tools',
  'sessions.all_projects': 'all projects',
  'sessions.all_kinds': '✎ and 💬',
  'sessions.changed': '✎ changed files',
  'sessions.talk': '💬 talk only',

  // First-run wizard
  'toast.invalid_file': 'The config file is invalid; nothing was changed.',
  'toast.cli_missing': 'The aiannotations executable was not found next to the app.',
  'toast.tool_cli_missing': "The tool's CLI was not found.",
  'toast.onboarding_pending': 'Finish the first-run wizard first.',
  'toast.hook_written': 'Hook written.',
  'toast.autostart_failed': 'Could not start with the system: {message}',
  'onboarding.title': 'First run',
  'onboarding.step': 'Step {n} of {total}',
  'onboarding.back': 'Back',
  'onboarding.next': 'Continue',
  'onboarding.finish': 'Finish',
  'onboarding.detect.title': 'Detection',
  'onboarding.detect.found': '{tool}: found at {bin}',
  'onboarding.detect.not_found': '{tool}: not found',
  'onboarding.hooks.title': 'Hooks',
  'onboarding.hooks.none': 'No AI CLI found: there are no hooks to write. You can skip.',
  'onboarding.hooks.path': 'File: {path}',
  'onboarding.hooks.write': 'Write hook',
  'onboarding.hooks.current': 'Hook already installed',
  'onboarding.hooks.invalid': 'Invalid file; nothing was changed',
  'onboarding.hooks.no_cli': 'The aiannotations executable was not found next to the app',
  'onboarding.hooks.codex_trust': 'Open /hooks in Codex and trust the AIAnnotations hooks; without that they do not run.',
  'onboarding.hooks.skip': 'Skipping is fine: you can write the hooks later.',
  'onboarding.processing.title': 'Processing',
  'onboarding.processing.none': 'No AI CLI detected: summaries stay off until you install one.',
  'onboarding.processing.provider': 'CLI that generates summaries and the briefing',
  'onboarding.processing.summary_model': 'Summary model',
  'onboarding.processing.briefing_model': 'Briefing model',
  'onboarding.history.title': 'Import history',
  'onboarding.history.import': 'Import',
  'onboarding.history.progress': 'Importing…',
  'onboarding.history.counts': '{sessions} sessions in {projects} projects — {claude} Claude Code, {codex} Codex',
  'onboarding.history.error': 'Import failed: {message}',
  'onboarding.history.summarize': 'Summarize the history with AI',
  'onboarding.history.estimate': {
    one: 'Uses your {cli} quota: about {count} run, one every 2 minutes.',
    other: 'Uses your {cli} quota: about {count} runs, one every 2 minutes.',
  },
  'onboarding.autostart.title': 'Start with the system',
  'onboarding.autostart.label': 'Start AIAnnotations when you log in',
  'today.hooks_outdated': 'The hooks need to be written again',

  // Search (Inv §1.6)
  'search.label': 'Search sessions',
  'search.placeholder': 'Search projects, branches and summaries…',
  'search.empty': 'Nothing found.',
  'search.no_project': 'no project',
} satisfies Record<string, Entry>;

export type Key = keyof typeof en;
