import { useCallback, useEffect, useState } from "react";
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

const REFRESH_INTERVAL_MS = 3 * 60 * 1000;

export function Welcome({ onInvite, onOpenProject }: Props) {
  const [code, setCode] = useState("");
  const [parseError, setParseError] = useState<string | null>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [countsByProject, setCountsByProject] = useState<
    Record<string, ChangeCounts | "offline" | "loading">
  >({});

  const loadProjects = useCallback(async () => {
    try {
      const list = await listProjects();
      setProjects(list);
      // Kick off background count poll for each project
      list.forEach((p) => {
        setCountsByProject((prev) => ({ ...prev, [p.project_id]: "loading" }));
        changeCounts(p.project_id)
          .then((c) =>
            setCountsByProject((prev) => ({ ...prev, [p.project_id]: c })),
          )
          .catch(() =>
            setCountsByProject((prev) => ({ ...prev, [p.project_id]: "offline" })),
          );
      });
    } catch {
      setProjects([]);
    }
  }, []);

  useEffect(() => {
    loadProjects();
    const interval = window.setInterval(loadProjects, REFRESH_INTERVAL_MS);
    const unlisten = listen<string>("deep-link-invite", (event) => {
      setCode(event.payload);
    });
    return () => {
      window.clearInterval(interval);
      unlisten.then((fn) => fn());
    };
  }, [loadProjects]);

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
  onOpen: () => void;
  onRelocated: () => void;
}

function ProjectTile({ project, counts, onOpen, onRelocated }: TileProps) {
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
        <h3>{project.project_name}</h3>
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
