import { invoke } from "@tauri-apps/api/core";
import type {
  ChangeCounts,
  ChangedFile,
  InviteData,
  Project,
  TailscaleStatus,
} from "./types";

export async function parseInvite(code: string): Promise<InviteData> {
  return invoke<InviteData>("parse_invite_cmd", { code });
}

export async function tailscaleStatus(): Promise<TailscaleStatus> {
  return invoke<TailscaleStatus>("tailscale_status");
}

export async function listProjects(): Promise<Project[]> {
  return invoke<Project[]>("list_projects");
}

export async function removeProject(projectId: string): Promise<void> {
  return invoke<void>("remove_project", { projectId });
}

export async function openProjectFolder(projectId: string): Promise<void> {
  return invoke<void>("open_project_folder", { projectId });
}

export async function applyInvite(
  code: string,
  workspaceRoot: string,
): Promise<Project> {
  return invoke<Project>("apply_invite", { code, workspaceRoot });
}

// ---- v0.5.1 daily ops ----

export async function listChanges(projectId: string): Promise<ChangedFile[]> {
  return invoke<ChangedFile[]>("p4_list_changes", { projectId });
}

export async function changeCounts(projectId: string): Promise<ChangeCounts> {
  return invoke<ChangeCounts>("p4_change_counts", { projectId });
}

export async function submitChanges(
  projectId: string,
  files: string[],
  description: string,
): Promise<number> {
  return invoke<number>("p4_submit_changes", { projectId, files, description });
}

export async function restoreFile(
  projectId: string,
  localPath: string,
): Promise<void> {
  return invoke<void>("p4_restore_file", { projectId, localPath });
}

export async function pullLatest(projectId: string): Promise<number> {
  return invoke<number>("p4_pull_latest", { projectId });
}

export async function forceResync(projectId: string): Promise<number> {
  return invoke<number>("p4_force_resync", { projectId });
}

export async function repairWorkspace(projectId: string): Promise<number> {
  return invoke<number>("p4_repair_workspace", { projectId });
}

export async function relocateProject(
  projectId: string,
  newRoot: string,
): Promise<number> {
  return invoke<number>("p4_relocate_project", { projectId, newRoot });
}

export async function revealInExplorer(localPath: string): Promise<void> {
  return invoke<void>("p4_reveal_in_explorer", { localPath });
}

export interface OpenInUnrealResult {
  uproject_path: string;
  config_path: string;
}

export async function openInUnreal(projectId: string): Promise<OpenInUnrealResult> {
  return invoke<OpenInUnrealResult>("open_in_unreal", { projectId });
}

// ---- v0.6 clean uninstall ----

/// Where Tailscale came from, from Companion's point of view.
/// Mirrors the Rust enum in src-tauri/src/commands/uninstall.rs.
export type TailscaleOrigin =
  | "installedByUs"
  | "preExisting"
  | "notInstalled"
  | "unknown";

export async function detectTailscaleOrigin(): Promise<TailscaleOrigin> {
  return invoke<TailscaleOrigin>("detect_tailscale_origin_cmd");
}

/// Result of a Clean Uninstall run. Mirrors `CleanUninstallResult` in
/// `uninstall.rs`. The UninstallConfirm modal uses these flags to show
/// a brief "what we did" summary in the second-or-so between the IPC
/// reply and Companion's process exit.
export interface CleanUninstallResult {
  p4ticketsScrubbed: boolean;
  appdataRemoved: boolean;
  localappdataRemoved: boolean;
  tailscaleUninstalled: boolean;
  companionUninstallFired: boolean;
  fellBackToSettings: boolean;
}

export async function cleanUninstall(
  removeTailscale: boolean,
): Promise<CleanUninstallResult> {
  return invoke<CleanUninstallResult>("clean_uninstall_cmd", {
    removeTailscale,
  });
}
