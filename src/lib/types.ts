// Mirrors the Rust types in src-tauri/src/commands/*.rs.
// Keep in sync; if you change one side, change both.

export interface WorkspaceTemplate {
  name: string;
  view: string[];
  options: string;
}

export interface PerforceSection {
  server: string;
  user: string;
  ticket: string;
  workspace_template: WorkspaceTemplate;
  local_root_default: string;
}

export interface TailscaleSection {
  auth_key: string;
  tags: string[];
}

export interface InviteData {
  v: number;
  project_name: string;
  project_id: string;
  issued_at: string;
  expires_at: string;
  issued_by: string;
  perforce: PerforceSection;
  tailscale: TailscaleSection;
}

export interface Project {
  project_id: string;
  project_name: string;
  issued_by: string;
  expires_at: string;
  workspace_name: string;
  workspace_root: string;
  server: string;
  user: string;
  ticket: string;
  last_sync_at: string | null;
  last_sync_files: number | null;
}

export interface FriendlyError {
  title: string;
  body: string;
  suggested_action: string;
  details: string;
}

export interface TailscaleStatus {
  up: boolean;
  backend_state: string;
  self_ip: string | null;
  tailnet_name: string | null;
}

// Onboarding event shapes — emitted by the Rust onboarding chain.
export type OnboardingEvent =
  | { kind: "step"; step: OnboardingStepId; status: StepStatus; detail: string | null }
  | { kind: "sync_progress"; line: string; files_so_far: number }
  | { kind: "complete"; project: Project }
  | { kind: "failed"; step: OnboardingStepId; error: FriendlyError };

export type OnboardingStepId =
  | "parse"
  | "service"
  | "connect"
  | "server"
  | "ticket"
  | "workspace"
  | "sync";

export type StepStatus = "running" | "installing" | "done" | "failed";

// ---- v0.5.1 daily-ops types — mirror perforce.rs ----

export type FileStatus = "M" | "A" | "D";

export interface ChangedFile {
  status: FileStatus;
  local_path: string;
  depot_path: string;
  size: number | null;
}

export interface ChangeCounts {
  local_pending: number;
  remote_unseen: number;
  /** True when the project's workspace folder no longer exists on disk. */
  folder_missing: boolean;
}

export interface PullProgressEvent {
  project_id: string;
  line: string;
  force?: boolean;
}

export const STEP_LABELS: Record<OnboardingStepId, string> = {
  parse: "Reading invite",
  service: "Preparing connection",
  connect: "Joining the network",
  server: "Reaching the project server",
  ticket: "Saving your access pass",
  workspace: "Setting up your workspace",
  sync: "Downloading your project",
};
