import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CopyField } from "../components/CopyField";
import { ForceResyncConfirm } from "../components/ForceResyncConfirm";
import {
  changeCounts,
  forceResync,
  openInUnreal,
  openProjectFolder,
  pullLatest,
  removeProject,
} from "../lib/invoke";
import type { ChangeCounts, Project } from "../lib/types";

interface UpdateInfo {
  current_version: string;
  new_version: string;
  notes: string | null;
}

interface Props {
  project: Project;
  fresh: boolean;
  onBack: () => void;
  onRemoved: () => void;
  onOpenChanges: () => void;
}

type RefreshState = "idle" | "loading" | "error";

export function ProjectView({
  project,
  fresh,
  onBack,
  onRemoved,
  onOpenChanges,
}: Props) {
  const [counts, setCounts] = useState<ChangeCounts | null>(null);
  const [refreshState, setRefreshState] = useState<RefreshState>("idle");
  const [pulling, setPulling] = useState(false);
  const [pullMessage, setPullMessage] = useState<string | null>(null);
  const [showForceConfirm, setShowForceConfirm] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [openingUnreal, setOpeningUnreal] = useState(false);
  const [unrealError, setUnrealError] = useState<string | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [removeConfirmText, setRemoveConfirmText] = useState("");
  const removeArmed = removeConfirmText.trim().toLowerCase() === "yes";

  async function handleCheckForUpdates() {
    setCheckingUpdate(true);
    setUpdateStatus(null);
    try {
      const info = (await invoke("check_for_updates")) as UpdateInfo | null;
      if (info) {
        setUpdateStatus(
          `Update available: v${info.current_version} → v${info.new_version}. (Banner at the top will let you install it.)`,
        );
      } else {
        setUpdateStatus("You're on the latest version.");
      }
    } catch (err) {
      const e = err as { body?: string; title?: string; details?: string };
      const head = e?.body ?? e?.title ?? "Update check failed.";
      // Surface the raw error too so it's possible to diagnose what
      // actually failed (network vs signature vs parse vs ...). Without
      // this, the friendly translator hides the only useful information.
      const msg = e?.details ? `${head} (${e.details})` : head;
      setUpdateStatus(msg);
    } finally {
      setCheckingUpdate(false);
    }
  }

  const refreshCounts = useCallback(async () => {
    setRefreshState("loading");
    try {
      const c = await changeCounts(project.project_id);
      setCounts(c);
      setRefreshState("idle");
    } catch {
      setRefreshState("error");
    }
  }, [project.project_id]);

  useEffect(() => {
    refreshCounts();
  }, [refreshCounts]);

  async function handlePull() {
    setPulling(true);
    setPullMessage(null);
    const start = Date.now();
    try {
      const n = await pullLatest(project.project_id);
      const elapsed = ((Date.now() - start) / 1000).toFixed(1);
      setPullMessage(
        n > 0
          ? `✓ Pulled ${n} file${n === 1 ? "" : "s"} from server (${elapsed}s)`
          : `✓ Already up to date with server — nothing new (${elapsed}s)`,
      );
      await refreshCounts();
      // Auto-clear the message after a beat so it doesn't linger forever.
      window.setTimeout(() => setPullMessage(null), 12000);
    } catch (err) {
      const e = err as { title?: string; body?: string };
      setPullMessage(`✗ Pull failed: ${e?.body ?? e?.title ?? "unknown error"}`);
    } finally {
      setPulling(false);
    }
  }

  async function handleOpenFolder() {
    try {
      await openProjectFolder(project.project_id);
    } catch (err) {
      console.error(err);
    }
  }

  async function handleOpenUnreal() {
    setOpeningUnreal(true);
    setUnrealError(null);
    try {
      await openInUnreal(project.project_id);
      // Brief success feedback (no toast yet; the button text reverts).
      window.setTimeout(() => setOpeningUnreal(false), 1500);
    } catch (err) {
      const e = err as { body?: string; title?: string };
      setUnrealError(e?.body ?? e?.title ?? "Could not open Unreal.");
      setOpeningUnreal(false);
    }
  }

  async function handleForceResync() {
    setShowForceConfirm(false);
    setPulling(true);
    setPullMessage("Force re-downloading…");
    try {
      const n = await forceResync(project.project_id);
      setPullMessage(`Force re-download complete (${n} files).`);
      await refreshCounts();
    } catch (err) {
      const e = err as { title?: string; body?: string };
      setPullMessage(e?.body ?? "Force re-download failed.");
    } finally {
      setPulling(false);
    }
  }

  async function handleRemove() {
    try {
      await removeProject(project.project_id);
      onRemoved();
    } catch (err) {
      console.error(err);
    }
  }

  const localPending = counts?.local_pending ?? 0;
  const remoteUnseen = counts?.remote_unseen ?? 0;

  return (
    <>
      <div
        style={{
          display: "flex",
          alignItems: "flex-start",
          gap: 18,
          marginBottom: 4,
        }}
      >
        <div style={{ flex: 1, minWidth: 0 }}>
          <h1 className="page-title" style={{ marginBottom: 6 }}>
            {fresh ? (
              <>You're <em>connected</em></>
            ) : (
              <span>{project.project_name}</span>
            )}
          </h1>
          <p className="page-subtitle" style={{ marginBottom: 0 }}>
            {fresh
              ? `Your project files are in ${project.workspace_root}. Open Unreal next.`
              : "Your project is ready. Edit assets from Unreal."}
          </p>
        </div>
        <button
          className="btn open-unreal"
          style={{ flex: "0 0 auto" }}
          onClick={handleOpenUnreal}
          disabled={openingUnreal}
          title="Launch the project's .uproject in Unreal Editor with Perforce already configured."
        >
          <span className="glyph">▶</span>{" "}
          {openingUnreal ? "Opening…" : "Open in Unreal"}
        </button>
      </div>
      {unrealError && (
        <div
          style={{
            border: "1px solid var(--danger)",
            background: "rgba(196, 108, 90, 0.08)",
            padding: "10px 14px",
            borderRadius: "var(--radius-s)",
            fontSize: 12,
            color: "var(--cream)",
            margin: "12px 0",
          }}
        >
          {unrealError}
        </div>
      )}
      <hr className="rule" style={{ margin: "18px 0" }} />

      {/* Status row */}
      <div className="status-row">
        <div className="pill-block">
          <span className="pill-label">Connection</span>
          <span className="status-pill ok" style={{ fontSize: 11 }}>
            Connected
          </span>
        </div>
        <div className="divider" />
        <div className="pill-block">
          <span className="pill-label">Last download</span>
          <span className="pill-value">
            {project.last_sync_at
              ? `${relativeTime(project.last_sync_at)} · ${project.last_sync_files ?? "?"} files`
              : "never"}
          </span>
        </div>
        <div className="divider" />
        <div className="pill-block">
          <span className="pill-label">Folder</span>
          <span
            className="pill-value"
            title={project.workspace_root}
            style={{
              maxWidth: 260,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {shortenPath(project.workspace_root)}
          </span>
        </div>
        <div className="right">
          <button
            className="btn btn-ghost"
            title="Re-check connection and look for new local changes"
            onClick={refreshCounts}
            disabled={refreshState === "loading"}
            style={{ fontSize: 13, padding: "10px 18px", display: "inline-flex", gap: 8, alignItems: "center" }}
          >
            <span style={{ fontSize: 16 }}>↻</span>
            {refreshState === "loading" ? "Refreshing…" : "Refresh"}
          </button>
        </div>
      </div>

      {/* Badges */}
      <div style={{ display: "flex", gap: 12, marginBottom: 18 }}>
        <div
          style={{
            flex: 1,
            padding: "14px 18px",
            background:
              localPending > 0 ? "rgba(212, 180, 122, 0.08)" : "var(--ink-2)",
            border: `1px solid ${localPending > 0 ? "var(--gold-dim)" : "var(--rule)"}`,
            borderRadius: "var(--radius-m)",
            opacity: localPending === 0 ? 0.6 : 1,
          }}
        >
          <div
            style={{
              fontSize: 10,
              textTransform: "uppercase",
              letterSpacing: "var(--tracking-caps)",
              color: localPending > 0 ? "var(--gold)" : "var(--warm-gray)",
              marginBottom: 4,
            }}
          >
            {localPending > 0
              ? `${localPending} local change${localPending === 1 ? "" : "s"} not sent`
              : "No local changes"}
          </div>
          <div
            style={{
              fontSize: 12,
              color: "var(--cream-dim)",
              fontFamily: "var(--font-serif)",
              fontStyle: "italic",
            }}
          >
            Files you've edited that aren't on the server yet.
          </div>
          <div style={{ marginTop: 10 }}>
            <button
              className="btn btn-small"
              onClick={onOpenChanges}
              disabled={localPending === 0}
            >
              Review &amp; submit
            </button>
          </div>
        </div>

        <div
          style={{
            flex: 1,
            padding: "14px 18px",
            background: "var(--ink-2)",
            border: "1px solid var(--rule)",
            borderRadius: "var(--radius-m)",
            opacity: remoteUnseen === 0 ? 0.6 : 1,
          }}
        >
          <div
            style={{
              fontSize: 10,
              textTransform: "uppercase",
              letterSpacing: "var(--tracking-caps)",
              color: "var(--cream-dim)",
              marginBottom: 4,
            }}
          >
            {remoteUnseen > 0
              ? `${remoteUnseen} change${remoteUnseen === 1 ? "" : "s"} on server`
              : "No new changes on server"}
          </div>
          <div
            style={{
              fontSize: 12,
              color: "var(--cream-dim)",
              fontFamily: "var(--font-serif)",
              fontStyle: "italic",
            }}
          >
            Files updated since your last download.
          </div>
          <div style={{ marginTop: 10 }}>
            <button
              className="btn btn-small"
              onClick={handlePull}
              disabled={pulling}
            >
              {pulling
                ? "Pulling…"
                : remoteUnseen > 0
                  ? "Pull latest"
                  : "Re-check & pull"}
            </button>
          </div>
        </div>
      </div>

      {pullMessage && (
        <div
          style={{
            padding: "14px 18px",
            background: pullMessage.startsWith("✓")
              ? "rgba(127, 168, 115, 0.10)"
              : "rgba(196, 108, 90, 0.10)",
            border: `1px solid ${
              pullMessage.startsWith("✓")
                ? "var(--ok)"
                : "var(--danger)"
            }`,
            borderRadius: "var(--radius-m)",
            fontSize: 13,
            color: "var(--cream)",
            fontFamily: "var(--font-serif)",
            fontStyle: "italic",
            marginBottom: 18,
          }}
        >
          {pullMessage}
        </div>
      )}

      <div className="action-bar">
        <button className="btn btn-ghost" onClick={handleOpenFolder}>
          <span className="glyph">▤</span> Open project folder
        </button>
      </div>

      <details className="card collapsible" open={fresh}>
        <summary className="section-label" style={{ margin: 0 }}>
          <span>S E R V E R &nbsp; D E T A I L S</span>
          <span className="caret">›</span>
        </summary>
        <div
          style={{
            fontSize: 12,
            color: "var(--cream-dim)",
            marginBottom: 14,
            fontFamily: "var(--font-serif)",
            fontStyle: "italic",
          }}
        >
          If you set Unreal up by hand: Editor Preferences → Source Control →
          Provider: Perforce. Paste each line, then click <strong>Accept Settings</strong>.
          The big <em>Open in Unreal</em> button above does this automatically.
        </div>
        <CopyField label="Server" value={project.server} />
        <CopyField label="User name" value={project.user} />
        <CopyField label="Workspace" value={project.workspace_name} />
        <div
          style={{
            marginTop: 14,
            paddingTop: 12,
            borderTop: "1px solid var(--rule)",
            fontSize: 11,
            color: "var(--warm-gray)",
          }}
        >
          Your access pass is already saved on this computer. Unreal will find
          it automatically — no password to type.
        </div>
      </details>

      <details className="card collapsible">
        <summary className="section-label" style={{ margin: 0 }}>
          <span>A D V A N C E D</span>
          <span className="caret">›</span>
        </summary>
        <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <div>
            <div
              style={{
                fontFamily: "var(--font-serif)",
                fontSize: 14,
                marginBottom: 4,
              }}
            >
              Check for Companion updates
            </div>
            <div
              style={{
                fontSize: 12,
                color: "var(--cream-dim)",
                marginBottom: 8,
              }}
            >
              Companion checks for updates automatically when it starts.
              You can also check manually here. Updates install in seconds
              and the app restarts on its own.
            </div>
            <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
              <button
                className="btn btn-small"
                onClick={handleCheckForUpdates}
                disabled={checkingUpdate}
              >
                {checkingUpdate ? "Checking…" : "Check for updates"}
              </button>
              {updateStatus && (
                <span
                  style={{
                    fontSize: 11.5,
                    color: "var(--cream-dim)",
                    fontStyle: "italic",
                    fontFamily: "var(--font-serif)",
                  }}
                >
                  {updateStatus}
                </span>
              )}
            </div>
          </div>
          <div style={{ borderTop: "1px solid var(--rule)", paddingTop: 14 }}>
            <div
              style={{
                fontFamily: "var(--font-serif)",
                fontSize: 14,
                marginBottom: 4,
              }}
            >
              Force re-download everything
            </div>
            <div
              style={{
                fontSize: 12,
                color: "var(--cream-dim)",
                marginBottom: 8,
              }}
            >
              Replaces every file in your project folder with the server's
              current copy.{" "}
              <em style={{ color: "var(--danger)", fontStyle: "italic" }}>
                Any local changes you haven't submitted will be lost.
              </em>{" "}
              Only use if your workspace is in a weird state.
            </div>
            <button
              className="btn btn-small btn-danger"
              onClick={() => setShowForceConfirm(true)}
              disabled={pulling}
            >
              Force re-download…
            </button>
          </div>
          <div
            style={{
              borderTop: "1px solid var(--rule)",
              paddingTop: 14,
            }}
          >
            <div
              style={{
                fontFamily: "var(--font-serif)",
                fontSize: 14,
                marginBottom: 4,
              }}
            >
              Stop showing this project in Companion
            </div>
            <div
              style={{
                fontSize: 12,
                color: "var(--cream-dim)",
                marginBottom: 8,
              }}
            >
              Removes this project from your Companion sidebar only. Nothing
              else changes — <strong>your local files stay where they are</strong>,
              your user account stays valid on the server, and you can re-add
              this project later by pasting the same invite. To actually lose
              access, your project lead has to remove your user on the server.
            </div>
            {!confirmRemove ? (
              <button
                className="btn btn-small btn-danger"
                onClick={() => setConfirmRemove(true)}
              >
                Stop showing in Companion…
              </button>
            ) : (
              <div
                style={{
                  fontSize: 12,
                  color: "var(--cream-dim)",
                  fontFamily: "var(--font-serif)",
                  fontStyle: "italic",
                }}
              >
                Your local files in {project.workspace_root} stay put. Type{" "}
                <strong
                  style={{
                    color: "var(--cream)",
                    fontFamily: "var(--font-mono)",
                    fontStyle: "normal",
                  }}
                >
                  YES
                </strong>{" "}
                to confirm:
                <input
                  autoFocus
                  value={removeConfirmText}
                  onChange={(e) => setRemoveConfirmText(e.target.value)}
                  placeholder="Type YES to confirm"
                  style={{ marginTop: 8, marginBottom: 8 }}
                />
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    className="btn btn-small btn-danger"
                    onClick={handleRemove}
                    disabled={!removeArmed}
                    style={
                      removeArmed
                        ? {
                            background: "var(--danger)",
                            color: "var(--ink)",
                            borderColor: "var(--danger)",
                          }
                        : undefined
                    }
                  >
                    Stop showing
                  </button>
                  <button
                    className="btn btn-small btn-ghost"
                    onClick={() => {
                      setConfirmRemove(false);
                      setRemoveConfirmText("");
                    }}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      </details>

      <hr className="rule" />

      <div className="section-label">D E T A I L S</div>
      <div className="card card-tight">
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "150px 1fr",
            rowGap: 6,
            fontSize: 12,
            color: "var(--cream-dim)",
          }}
        >
          <span>Project folder</span>
          <span style={{ fontFamily: "var(--font-mono)" }}>
            {project.workspace_root}
          </span>
          <span>Invited by</span>
          <span>{project.issued_by}</span>
          <span>Expires</span>
          <span>{new Date(project.expires_at).toLocaleString()}</span>
          {project.last_sync_at && (
            <>
              <span>Last download</span>
              <span>
                {new Date(project.last_sync_at).toLocaleString()} ·{" "}
                {project.last_sync_files ?? "?"} files
              </span>
            </>
          )}
        </div>
      </div>

      <div style={{ marginTop: 22, display: "flex", gap: 12 }}>
        <button className="btn btn-ghost" onClick={onBack}>
          ‹ All projects
        </button>
      </div>

      {showForceConfirm && (
        <ForceResyncConfirm
          projectName={project.project_name}
          localChanges={localPending}
          onConfirm={handleForceResync}
          onCancel={() => setShowForceConfirm(false)}
        />
      )}
    </>
  );
}

function shortenPath(p: string): string {
  if (p.length < 40) return p;
  const parts = p.split(/[\\/]/);
  if (parts.length < 3) return p;
  return `…\\${parts.slice(-2).join("\\")}`;
}

function relativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "—";
  const diffMs = Date.now() - then;
  const min = Math.floor(diffMs / 60_000);
  if (min < 1) return "just now";
  if (min < 60) return `${min} min ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr} hour${hr === 1 ? "" : "s"} ago`;
  const d = Math.floor(hr / 24);
  return `${d} day${d === 1 ? "" : "s"} ago`;
}
