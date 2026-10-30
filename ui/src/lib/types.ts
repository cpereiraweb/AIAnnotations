// Literal mirror of the JSON the Rust commands return (serde, snake_case). Instants are
// milliseconds UTC; formatting is the frontend's job. Keep in sync with:
// - crates/core: model.rs, inbox/mod.rs, briefing/mod.rs, ongoings.rs, search.rs, settings.rs
// - crates/services: views/{today,runs,projects}.rs, worker.rs

export type Tool = 'claude' | 'codex';
export type SummaryStatus = 'pending' | 'done' | 'failed' | 'skipped';
export type BriefingStatus = 'generating' | 'done' | 'failed';
export type EndReason = 'prompt_input_exit' | 'clear' | 'logout' | 'other' | 'idle';
export type Lang = 'en' | 'pt-BR';

export type ItemKind = 'interrupted' | 'stale_branch' | 'dirty' | 'worktree' | 'reminder' | 'ongoing';
export type Badge =
  | 'interrupted'
  | 'half_done'
  | 'stale_branch'
  | 'uncommitted_changes'
  | 'worktree'
  | 'reminder'
  | 'ongoing';
/** In display order (the Rust enum's `Ord`). */
export const COUNT_KEYS = [
  'interrupted',
  'half_done',
  'ongoing',
  'reminder',
  'stale_branch',
  'worktree',
  'dirty',
] as const;
export type CountKey = (typeof COUNT_KEYS)[number];
export type HiddenLabel = 'empty' | 'automation' | 'subagent' | 'internal';
export type Job = 'ingest' | 'summarize' | 'collect' | 'briefing' | 'maintenance';

/** `inbox::InboxItem`. */
export interface InboxItem {
  key: string;
  kind: ItemKind;
  badge: Badge;
  project_id: number;
  title: string;
  /** Data only (worktree path, project name); the wording is the UI's. */
  detail: string | null;
  state_hash: string;
  session_id: number | null;
  reference_ms: number | null;
  path: string | null;
  ended_at_ms: number | null;
  ahead: number | null;
  age_days: number | null;
}

/** `inbox::InboxGroup`; `counts` is a list of `[key, n]` tuples. */
export interface InboxGroup {
  project_id: number;
  name: string;
  items: InboxItem[];
  expanded: boolean;
  latest_ms: number;
  counts: [CountKey, number][];
}

export interface GroupedInbox {
  recent: InboxGroup[];
  older: InboxGroup[];
}

/** `briefing::NextStep`: keeps the v1 field names stored in `briefings.next_steps`. */
export interface NextStep {
  project_id: number;
  projeto: string;
  passo: string;
}

export interface Briefing {
  id: number;
  date: string;
  status: BriefingStatus;
  body: string | null;
  next_steps: NextStep[];
  covers_from_ms: number;
  covers_until_ms: number;
  generated_at_ms: number | null;
}

/** `ongoings::Ongoing`. */
export interface Ongoing {
  project_id: number;
  project_name: string;
  project_path: string;
  file: string;
  name: string;
  title: string;
  modified_ms: number;
}

export interface OngoingGroup {
  project_id: number;
  project_name: string;
  items: Ongoing[];
}

/** `views::runs::RunCard`. */
export interface RunCard {
  run_id: number;
  session_id: number;
  tool: Tool;
  tool_session_id: string;
  project_id: number;
  project_name: string;
  cwd: string | null;
  branch: string | null;
  title: string | null;
  summary: string | null;
  next_step: string | null;
  summary_status: SummaryStatus;
  started_at_ms: number;
  ended_at_ms: number | null;
  end_reason: EndReason | null;
  ended_cleanly: boolean;
  changed_files: boolean | null;
  files_count: number;
  hidden_label: HiddenLabel | null;
  root_session_id: number;
}

export interface Page<T> {
  items: T[];
  page: number;
  pages: number;
  total: number;
}

/** `views::today::TodayView`. */
export interface TodayView {
  briefing: Briefing | null;
  briefing_running: boolean;
  ongoings_recent: OngoingGroup[];
  ongoings_stale: OngoingGroup[];
  inbox: GroupedInbox;
  /** Last run of every session cited by the inbox, by `session_id` (JSON object keys are strings). */
  inbox_last_runs: Record<string, RunCard>;
  /** Only when the briefing is `done`. */
  next_steps: NextStep[];
  /** `[done, total]` while a backfill still has runs to summarize. */
  backfill: [number, number] | null;
  is_empty_db: boolean;
}

export interface ToolUsage {
  tool: Tool;
  changed_files: boolean;
}

export interface ProjectCard {
  id: number;
  name: string;
  path: string | null;
  remote_url: string | null;
  last_activity_ms: number | null;
  ongoing_count: number;
  tools_14d: ToolUsage[];
  last_run: RunCard | null;
}

export interface ProjectDetail {
  card: ProjectCard;
  ongoings: Ongoing[];
}

/** `search::SearchHit`. */
export interface SearchHit {
  run_id: number;
  ai_session_id: number;
  project_id: number;
  project_name: string;
  tool: Tool;
  branch: string | null;
  title: string | null;
  summary: string | null;
  next_step: string | null;
  started_at_ms: number;
}

/** `worker::WorkerStatus`. */
export interface WorkerStatus {
  last_tick_ms: number | null;
  paused_until_ms: number | null;
  pause_reason: string | null;
  summarizing_paused_by_user: boolean;
  last_errors: string[];
}

export type RunKind = 'changed' | 'talk';

/** Argument of the `sessions` command (`commands::SessionFilterArg`). */
export interface SessionFilter {
  tool?: Tool | null;
  project_id?: number | null;
  kind?: RunKind | null;
}
