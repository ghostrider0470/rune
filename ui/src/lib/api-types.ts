// Health & Status
export interface HealthResponse {
  status: string;
  service: string;
  version: string;
  uptime_seconds: number;
  session_count: number;
  ws_subscribers: number;
}

export interface StatusPaths {
  sessions_dir: string;
  memory_dir: string;
  logs_dir: string;
}

export interface StatusResponse {
  status: string;
  version: string;
  bind: string;
  auth_enabled: boolean;
  configured_model_providers: number;
  active_model_backend: string;
  registered_tools: number;
  session_count: number;
  cron_job_count: number;
  ws_subscribers: number;
  uptime_seconds: number;
  config_paths: StatusPaths;
}

// Dashboard
export interface DashboardSummaryResponse {
  gateway_status: string;
  bind: string;
  uptime_seconds: number;
  default_model: string | null;
  provider_count: number;
  configured_model_count: number;
  session_count: number;
  auth_enabled: boolean;
  ws_subscribers: number;
  channels: string[];
}

export interface DashboardModelItem {
  provider_name: string;
  provider_kind: string;
  model_id: string;
  raw_model: string;
  is_default: boolean;
}

export interface DashboardSessionItem {
  id: string;
  kind: string;
  status: string;
  channel_ref: string | null;
  routing_ref: string | null;
  created_at: string;
  last_activity_at: string;
}

export interface DashboardDiagnosticItem {
  level: string;
  source: string;
  message: string;
  observed_at: string;
}

export interface DashboardDiagnosticsResponse {
  structured_errors_available: boolean;
  items: DashboardDiagnosticItem[];
}

// Actions
export interface ActionResponse {
  success: boolean;
  message: string;
}

// Cron
export interface CronStatusResponse {
  total_jobs: number;
  enabled_jobs: number;
  due_jobs: number;
}

export interface CronScheduleAt {
  kind: "at";
  at: string;
}

export interface CronScheduleEvery {
  kind: "every";
  every_ms: number;
  anchor_ms?: number;
}

export interface CronScheduleCron {
  kind: "cron";
  expr: string;
  tz?: string;
}

export type CronSchedule = CronScheduleAt | CronScheduleEvery | CronScheduleCron;

export interface CronPayloadSystemEvent {
  kind: "system_event";
  text: string;
}

export interface CronPayloadAgentTurn {
  kind: "agent_turn";
  message: string;
  model?: string;
  timeout_seconds?: number;
}

export type CronPayload = CronPayloadSystemEvent | CronPayloadAgentTurn;

export interface CronJobResponse {
  id: string;
  name: string | null;
  schedule: CronSchedule;
  payload: CronPayload;
  session_target: string;
  enabled: boolean;
  created_at: string;
  last_run_at: string | null;
  next_run_at: string | null;
  run_count: number;
}

export interface CronRunResponse {
  job_id: string;
  started_at: string;
  finished_at: string | null;
  status: string;
  output: string | null;
}

export interface CronMutationResponse {
  success: boolean;
  job_id: string;
  message: string;
}

export interface CronWakeResponse {
  success: boolean;
  mode: string;
  text: string;
  context_messages: number | null;
  message: string;
}

export interface CronJobRequest {
  name?: string;
  schedule: CronSchedule;
  payload: CronPayload;
  sessionTarget: string;
  enabled?: boolean;
}

export interface CronUpdateRequest {
  name?: string;
  enabled?: boolean;
  schedule?: CronSchedule;
  payload?: CronPayload;
}

export interface CronWakeRequest {
  text: string;
  mode?: string;
  contextMessages?: number;
}

// Sessions
export interface SessionListItem {
  id: string;
  kind: string;
  status: string;
  requester_session_id?: string | null;
  channel: string | null;
  created_at: string;
  last_activity_at?: string;
  updated_at?: string;
  turn_count: number;
  usage_prompt_tokens: number;
  usage_completion_tokens: number;
  latest_model: string | null;
  preview?: string;
}

export interface SessionResponse {
  id: string;
  kind: string;
  status: string;
  requester_session_id: string | null;
  channel_ref: string | null;
  created_at: string;
  updated_at: string;
  turn_count: number;
  latest_model: string | null;
  usage_prompt_tokens: number;
  usage_completion_tokens: number;
  last_turn_started_at: string | null;
  last_turn_ended_at: string | null;
}

export interface SessionStatusResponse {
  session_id: string;
  runtime: string;
  status: string;
  current_model: string | null;
  model_override: string | null;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  estimated_cost: string | null;
  turn_count: number;
  uptime_seconds: number;
  last_turn_started_at: string | null;
  last_turn_ended_at: string | null;
  reasoning: string;
  verbose: boolean;
  elevated: boolean;
  approval_mode: string;
  security_mode: string;
  subagent_lifecycle: string | null;
  subagent_runtime_status: string | null;
  subagent_runtime_attached: boolean | null;
  subagent_status_updated_at: string | null;
  subagent_last_note: string | null;
  unresolved: string[];
}

export interface CreateSessionRequest {
  kind?: string;
  workspace_root?: string;
  requester_session_id?: string;
  channel_ref?: string;
}

export interface PendingAttachment {
  name: string;
  mime_type?: string;
  size_bytes?: number;
}

export interface SendMessageRequest {
  content: string;
  model?: string;
  attachments?: File[];
}

export interface MessageResponse {
  turn_id: string;
  assistant_reply: string | null;
  usage: {
    prompt_tokens: number;
    completion_tokens: number;
  };
  latency_ms: number;
}

export interface TranscriptEntry {
  id: string;
  turn_id: string | null;
  seq: number;
  kind: string;
  payload: unknown;
  created_at: string;
}


export interface AgentListItem {
  id: string;
  default: boolean;
  model: string | null;
  workspace: string | null;
  system_prompt: string | null;
}

export interface SkillItem {
  name: string;
  description: string;
  enabled: boolean;
  binary_path: string | null;
  source_dir: string;
  parameters: unknown;
}

// Approvals
export interface ApprovalRequestResponse {
  id: string;
  subject_type: string;
  subject_id: string;
  reason: string;
  decision: string | null;
  decided_by: string | null;
  decided_at: string | null;
  approval_status: string | null;
  approval_status_updated_at: string | null;
  resumed_at: string | null;
  completed_at: string | null;
  resume_result_summary: string | null;
  command: string | null;
  presented_payload: unknown;
  created_at: string;
}

export interface SubmitApprovalDecisionRequest {
  id: string;
  decision: string;
  decided_by?: string;
}

export interface ApprovalPolicyResponse {
  tool_name: string;
  decision: string;
  decided_at: string;
}

export interface SetApprovalPolicyRequest {
  decision: string;
}

// Heartbeat
export interface HeartbeatState {
  enabled: boolean;
  last_heartbeat_at: string | null;
  interval_seconds: number;
}

// Reminders
export interface ReminderResponse {
  id: string;
  message: string;
  target: string;
  fire_at: string;
  delivered: boolean;
  created_at: string;
  delivered_at: string | null;
}

export interface ReminderAddRequest {
  message: string;
  fire_at: string;
  target?: string;
}

// WebSocket
export interface SessionEvent {
  session_id: string;
  kind: string;
  payload: unknown;
}

export type A2uiTarget = "inline" | "panel";

export interface A2uiComponent {
  type: string;
  id: string;
  [key: string]: unknown;
}

export interface A2uiPushEvent {
  action: "push";
  session_id: string;
  component: A2uiComponent;
  target: A2uiTarget;
  timestamp: string;
}

export interface A2uiRemoveEvent {
  action: "remove";
  session_id: string;
  component_id: string;
  timestamp: string;
}

export interface A2uiResetEvent {
  action: "reset";
  session_id: string;
  timestamp: string;
}

export interface A2uiFormSubmitEvent {
  action: "form_submit";
  session_id: string;
  callback_id: string;
  data: Record<string, unknown>;
  timestamp: string;
}

export interface A2uiActionEvent {
  action: "action";
  session_id: string;
  component_id: string;
  action_target: string;
  timestamp: string;
}

export type A2uiEvent =
  | A2uiPushEvent
  | A2uiRemoveEvent
  | A2uiResetEvent
  | A2uiFormSubmitEvent
  | A2uiActionEvent;

export interface A2uiFormSubmitRequest {
  session_id: string;
  callback_id: string;
  data: Record<string, unknown>;
}

export interface A2uiActionRequest {
  session_id: string;
  component_id: string;
  action_target: string;
}

// TTS/STT
export interface TtsVoiceEntry {
  id: string;
  name: string;
  language: string | null;
}

export interface TtsStatusResponse {
  available: boolean;
  enabled: boolean;
  provider: string;
  voice: string;
  model: string;
  auto_mode: string;
  voices: TtsVoiceEntry[];
}

export interface SttStatusResponse {
  available: boolean;
  enabled: boolean;
  provider: string;
  model: string;
}

export interface TranscribeResponse {
  text: string;
  language: string | null;
  duration_seconds: number | null;
}
