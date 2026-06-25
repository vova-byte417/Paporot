// ─── Paporot Dashboard Data Types ──────────────────────────

export interface DashboardData {
  project_name: string
  analyzed_at: string
  git_commit?: string
  git_ref?: string

  // L1: AST Analysis
  l1_analysis: L1Analysis

  // L2: Rule Engine
  l2_analysis: L2Analysis

  // L3: LLM Bridge (optional)
  l3_analysis?: L3Analysis

  // Feedback Loop (v3)
  feedback_loop: FeedbackLoopData

  // Snapshot History
  snapshot?: SnapshotOverview

  // Agent Trace Association
  trace_association?: TraceAssociation

  // Contract Verification
  contracts?: ContractOverview

  // Skills
  skills: SkillSummary[]
}

// ─── L1 ───

export interface L1Analysis {
  total_files: number
  total_changes: number
  changes: RawChange[]
  by_language: Record<string, number>
  by_type: Record<string, number>
  by_directory: DirectoryChange[]
  confidence_distribution: ConfidenceDist
}

export interface RawChange {
  id: string
  symbol: string
  file: string
  change_type: string
  confidence: number
  language: string
  module?: string
  line_start: number
  line_end: number
  tags: string[]
  suppressed?: SuppressionRecord | null
  rules: string[]
}

export interface DirectoryChange {
  directory: string
  file_count: number
  changes: number
  added: number
  removed: number
  modified: number
}

export interface ConfidenceDist {
  high: number   // ≥ 0.85
  medium: number // 0.5–0.85
  low: number    // < 0.5
}

// ─── L2 ───

export interface L2Analysis {
  total_matches: number
  matches: RuleMatch[]
  by_severity: Record<string, number>
  by_category: Record<string, number>
}

export interface RuleMatch {
  rule_id: string
  change_id: string
  severity: string
  category: string
  description: string
  matched_tags: string[]
}

// ─── L3 ───

export interface L3Analysis {
  fragment_count: number
  model_used?: string
}

// ─── Feedback Loop ───

export interface FeedbackLoopData {
  loaded: boolean
  exact_reject_count: number
  rule_suppression_count: number
  prefix_warning_count: number
  suppressions: RuleSuppressionInfo[]
  changes: FeedbackChange[]
}

export interface FeedbackChange {
  id: string
  symbol: string
  file: string
  change_type: string
  confidence: number
  rules: string[]
  tags: string[]
  suppressed: SuppressionRecord | null
}

export interface SuppressionRecord {
  level: 'Exact' | 'Rule' | 'Warning'
  reason: string
  new_confidence: number
  matched_rule: string | null
}

export interface RuleSuppressionInfo {
  rule_id: string
  file_pattern: string
  change_type?: string
  effect: string
  reason: string
  hit_count: number
  last_hit?: string
  status: string
}

// ─── Snapshot ───

export interface SnapshotOverview {
  current_version: string
  version_count: number
  versions: VersionSummary[]
  current_diff: CapabilityDiff | null
}

export interface VersionSummary {
  version_id: string
  git_commit?: string
  created_at: string
  capability_count: number
  message?: string
}

export interface CapabilityDiff {
  added: CapabilitySummary[]
  modified: CapabilitySummary[]
  deleted: CapabilitySummary[]
  unchanged_count: number
}

export interface CapabilitySummary {
  id: string
  name: string
  status: string
  module?: string
  confidence: number
  evidence: string[]
  categories: string[]
}

// ─── Trace ───

export interface TraceAssociation {
  matched_traces: TraceMatch[]
  trajectory_analysis?: TrajectoryAnalysis
}

export interface TraceMatch {
  trace_id: string
  confidence: number
  match_level: 'commit' | 'file_overlap' | 'time_window'
  agent_platform?: string
  tool_count?: number
  duration_secs?: number
}

export interface TrajectoryAnalysis {
  tool_churn_score: number
  phase_reorder_score: number
  capability_shift_score: number
  tool_count_a: number
  tool_count_b: number
  shared_tool_count: number
  added_tool_count: number
  deleted_tool_count: number
  evaluation?: TrajectoryEval
}

export interface TrajectoryEval {
  verdict: 'improved' | 'degraded' | 'unchanged'
  score: number
  details: string[]
}

// ─── Contract ───

export interface ContractOverview {
  total: number
  passed: number
  failed: number
  results: ContractResult[]
}

export interface ContractResult {
  artifact_type: string
  artifact_id: string
  status: 'PASS' | 'FAIL'
  rules_checked: number
  rules_failed: number
  suggestions: string[]
}

// ─── Skills ───

export interface SkillSummary {
  name: string
  status: 'ok' | 'skipped' | 'failed'
  duration_ms: number
  output_summary: string
  error?: string
}
