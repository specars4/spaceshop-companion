import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  changeCounts,
  listProjects,
  parseInvite,
  relocateProject,
} from "../lib/invoke";
import type { ChangeCounts, InviteData, Project } from "../lib/types";

interface Props {
  onInvite: (code: string, parsed: InviteData) => void;
  onOpenProject: (project: Project) => void;
}

// How often we re-poll p4 status hints for tiles on the Welcome page.
// Token cost is tiny (p4 changes -m1 + p4 reconcile -n per project), but we
// pause while the window is hidden — see the visibilitychange handler below.
const REFRESH_INTERVAL_MS = 3 * 60 * 1000;

export function Welcome({ onInvite, onOpenProject }: Props) {
  const [code, setCode] = useState("");
  const [parseError, setParseError] = useState<string | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [countsByProject, setCountsByProject] = useState<
    Record<string, ChangeCounts | "offline" | "loading">
  >({});
  // Project IDs whose background re-poll is currently in flight. Drives the
  // subtle "checking" dot on each tile and lets us skip overlapping fetches.
  const [checkingIds, setCheckingIds] = useState<Set<string>>(() => new Set());

  // Refs used by the background poller. They survive re-renders and let the
  // interval callback read the latest projects / counts / mounted-state
  // without re-creating the interval every render.
  const mountedRef = useRef(true);
  const inFlightRef = useRef<Set<string>>(new Set());
  const projectsRef = useRef<Project[]>([]);
  const countsRef = useRef(countsByProject);

  useEffect(() => {
    projectsRef.current = projects;
  }, [projects]);
  useEffect(() => {
    countsRef.current = countsByProject;
  }, [countsByProject]);

  // Initial load: list projects + fetch first-pass counts for each. This is
  // the only path that sets a project's counts to "loading" — the background
  // poll below leaves stale counts visible while it refetches so the tile
  // doesn't flicker its hint text every 3 minutes.
  const loadProjects = useCallback(async () => {
    try {
      const list = await listProjects();
      if (!mountedRef.current) return;
      setProjects(list);
      list.forEach((p) => {
        setCountsByProject((prev) => ({ ...prev, [p.project_id]: "loading" }));
        inFlightRef.current.add(p.project_id);
        changeCounts(p.project_id)
          .then((c) => {
            if (!mountedRef.current) return;
            setCountsByProject((prev) => ({ ...prev, [p.project_id]: c }));
          })
          .catch(() => {
            if (!mountedRef.current) return;
            setCountsByProject((prev) => ({
              ...prev,
              [p.project_id]: "offline",
            }));
          })
          .finally(() => {
            inFlightRef.current.delete(p.project_id);
          });
      });
    } catch {
      if (!mountedRef.current) return;
      setProjects([]);
    }
  }, []);

  // Re-poll counts only (does NOT re-list projects). Skips projects whose
  // last result was folder_missing (no point — folder needs relocating) and
  // skips projects with a request already in flight from a previous tick.
  const refreshCounts = useCallback(() => {
    const list = projectsRef.current;
    const currentCounts = countsRef.current;
    list.forEach((p) => {
      if (inFlightRef.current.has(p.project_id)) return;
      const existing = currentCounts[p.project_id];
      if (
        existing !== undefined &&
        existing !== "loading" &&
        existing !== "offline" &&
        existing.folder_missing
      ) {
        return; // folder is gone — operator has to relocate first
      }
      inFlightRef.current.add(p.project_id);
      setCheckingIds((prev) => {
        const next = new Set(prev);
        next.add(p.project_id);
        return next;
      });
      changeCounts(p.project_id)
        .then((c) => {
          if (!mountedRef.current) return;
          setCountsByProject((prev) => ({ ...prev, [p.project_id]: c }));
        })
        .catch(() => {
          if (!mountedRef.current) return;
          setCountsByProject((prev) => ({
            ...prev,
            [p.project_id]: "offline",
          }));
        })
        .finally(() => {
          inFlightRef.current.delete(p.project_id);
          if (!mountedRef.current) return;
          setCheckingIds((prev) => {
            if (!prev.has(p.project_id)) return prev;
            const next = new Set(prev);
            next.delete(p.project_id);
            return next;
          });
        });
    });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    loadProjects();
    const unlisten = listen<string>("deep-link-invite", (event) => {
      setCode(event.payload);
    });

    // Background poll lifecycle. We keep a single interval handle and (re)create
    // it on visibility change. The 3-minute setInterval naturally satisfies the
    // "don't fire immediately on mount" constraint — first tick lands at +3min.
    let interval: number | null = null;
    const start = () => {
      if (interval !== null) return;
      interval = window.setInterval(refreshCounts, REFRESH_INTERVAL_MS);
    };
    const stop = () => {
      if (interval !== null) {
        window.clearInterval(interval);
        interval = null;
      }
      // Drop in-flight tracking so the next visible-tick starts fresh. The
      // promises themselves still resolve; mountedRef guards the setState.
      inFlightRef.current.clear();
      setCheckingIds((prev) => (prev.size === 0 ? prev : new Set()));
    };

    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        // Refresh immediately on re-show so a user who left for >3 min
        // doesn't see stale hints, then resume the cadence.
        refreshCounts();
        start();
      } else {
        stop();
      }
    };

    if (document.visibilityState === "visible") start();
    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      mountedRef.current = false;
      stop();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      unlisten.then((fn) => fn());
    };
  }, [loadProjects, refreshCounts]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setParseError(null);
    if (!code.trim()) {
      setParseError("Paste the invite code from your project lead.");
      return;
    }
    try {
      const parsed = await parseInvite(code.trim());
      onInvite(code.trim(), parsed);
    } catch (err) {
      const e = err as { title?: string; body?: string } | string;
      const message =
        typeof e === "string"
          ? e
          : (e?.body ?? e?.title ?? "That invite code doesn't look right.");
      setParseError(message);
    }
  }

  const hasProjects = projects.length > 0;

  return (
    <>
      <h1 className="page-title">
        {hasProjects ? (
          <>Welcome <em>back</em></>
        ) : (
          <>Let's get you <em>connected</em></>
        )}
      </h1>
      <p className="page-subtitle">
        {hasProjects
          ? "Open a project, or add a new one from an invite."
          : "Paste the invite your project lead sent you. Companion handles the rest."}
      </p>

      {hasProjects && (
        <>
          <div className="section-label">Y O U R &nbsp; P R O J E C T S</div>
          {projects.map((p) => (
            <ProjectTile
              key={p.project_id}
              project={p}
              counts={countsByProject[p.project_id]}
              checking={checkingIds.has(p.project_id)}
              onOpen={() => onOpenProject(p)}
              onRelocated={() => loadProjects()}
            />
          ))}
          <hr className="rule" />
        </>
      )}

      <div className="section-label">
        {hasProjects
          ? "A D D   N E W   P R O J E C T"
          : "I N V I T E   C O D E"}
      </div>
      <form className="card" onSubmit={handleSubmit}>
        <textarea
          placeholder="Paste your invite code here…"
          value={code}
          onChange={(e) => setCode(e.target.value)}
          spellCheck={false}
        />
        {parseError && (
          <div
            style={{
              color: "var(--danger)",
              fontSize: 12,
              marginTop: 10,
              fontFamily: "var(--font-serif)",
              fontStyle: "italic",
            }}
          >
            {parseError}
          </div>
        )}
        <div style={{ marginTop: 16, display: "flex", gap: 12 }}>
          <button className="btn" type="submit">
            Continue
          </button>
        </div>
      </form>
    </>
  );
}

interface TileProps {
  project: Project;
  counts: ChangeCounts | "offline" | "loading" | undefined;
  /** True while a background re-poll is in flight for this project. */
  checking: boolean;
  onOpen: () => void;
  onRelocated: () => void;
}

function ProjectTile({
  project,
  counts,
  checking,
  onOpen,
  onRelocated,
}: TileProps) {
  const [relocating, setRelocating] = useState(false);
  const [relocateError, setRelocateError] = useState<string | null>(null);

  let statusPill = <span className="status-pill ok">Connected</span>;
  let hint: React.ReactNode = null;
  const folderMissing =
    counts !== undefined &&
    counts !== "loading" &&
    counts !== "offline" &&
    counts.folder_missing;

  if (counts === "loading" || counts === undefined) {
    hint = <span style={{ color: "var(--warm-gray)" }}>Checking…</span>;
  } else if (counts === "offline") {
    statusPill = <span className="status-pill bad">Offline</span>;
    hint = (
      <span
        style={{
          color: "var(--warm-gray)",
          fontStyle: "italic",
          fontFamily: "var(--font-serif)",
        }}
      >
        Status from last connection.
      </span>
    );
  } else if (folderMissing) {
    statusPill = <span className="status-pill bad">Folder missing</span>;
    hint = (
      <span style={{ color: "var(--danger)" }}>
        ● Project folder is gone:{" "}
        <span
          style={{
            fontFamily: "var(--font-mono)",
            color: "var(--warm-gray)",
          }}
        >
          {project.workspace_root}
        </span>
      </span>
    );
  } else {
    const { local_pending, remote_unseen } = counts;
    if (local_pending === 0 && remote_unseen === 0) {
      hint = (
        <span style={{ color: "var(--cream-dim)" }}>
          Nothing new — you're up to date.
        </span>
      );
    } else {
      const parts: React.ReactNode[] = [];
      if (local_pending > 0) {
        parts.push(
          <span key="lp" style={{ color: "var(--gold)" }}>
            ● {local_pending} local change
            {local_pending === 1 ? "" : "s"} not sent
          </span>,
        );
      }
      if (remote_unseen > 0) {
        parts.push(
          <span key="ru" style={{ color: "var(--cream-dim)" }}>
            ● {remote_unseen} change
            {remote_unseen === 1 ? "" : "s"} on server
          </span>,
        );
      }
      hint = <span style={{ display: "flex", gap: 14 }}>{parts}</span>;
    }
  }

  async function handleRelocate() {
    setRelocateError(null);
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: `Pick a new folder for ${project.project_name}`,
    });
    if (typeof picked !== "string") return;

    setRelocating(true);
    try {
      await relocateProject(project.project_id, picked);
      onRelocated();
    } catch (err) {
      const e = err as { body?: string; title?: string };
      setRelocateError(e?.body ?? e?.title ?? "Could not relocate project.");
    } finally {
      setRelocating(false);
    }
  }

  return (
    <div className="project-tile">
      <div>
        <h3 style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span>{project.project_name}</span>
          {/* Subtle "background re-poll in flight" indicator. Only shown
              when we already have counts displayed — during the very first
              load the "Checking…" hint covers it, so no need to double up. */}
          {checking && counts !== "loading" && counts !== undefined && (
            <span
              className="checking-dot"
              title="Refreshing status…"
              aria-label="Refreshing status"
            />
          )}
        </h3>
        <div
          style={{
            display: "flex",
            gap: 14,
            alignItems: "center",
            marginTop: 6,
            flexWrap: "wrap",
          }}
        >
          {statusPill}
          <span
            style={{
              fontSize: 11,
              color: "var(--warm-gray)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {project.user} · last opened{" "}
            {project.last_sync_at
              ? new Date(project.last_sync_at).toLocaleDateString()
              : "—"}
          </span>
        </div>
        <div style={{ marginTop: 8, fontSize: 11 }}>{hint}</div>
        {relocateError && (
          <div
            style={{
              marginTop: 8,
              fontSize: 11,
              color: "var(--danger)",
              fontStyle: "italic",
              fontFamily: "var(--font-serif)",
            }}
          >
            {relocateError}
          </div>
        )}
      </div>
      <div className="actions">
        {folderMissing ? (
          <button
            className="btn btn-small"
            onClick={handleRelocate}
            disabled={relocating}
          >
            {relocating ? "Setting up…" : "Pick new folder…"}
          </button>
        ) : (
          <button className="btn btn-small" onClick={onOpen}>
            Open
          </button>
        )}
      </div>
    </div>
  );
}
