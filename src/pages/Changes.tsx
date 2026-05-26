import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  listChanges,
  reconcileWorkspace,
  restoreFile,
  revealInExplorer,
  submitChanges,
} from "../lib/invoke";
import type { ChangedFile, FileStatus, Project } from "../lib/types";

interface Props {
  project: Project;
  onBack: () => void;
  onSubmitted: (changelist: number) => void;
}

const STATUS_GLYPH: Record<FileStatus, string> = {
  M: "M",
  A: "+",
  D: "−",
};

export function Changes({ project, onBack, onSubmitted }: Props) {
  const [files, setFiles] = useState<ChangedFile[]>([]);
  const [selected, setSelected] = useState<Record<string, boolean>>({});
  const [description, setDescription] = useState("");
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  // v0.6.5 — Scan-for-new-files button. Separate spinner from `loading`
  // so we can keep the existing change list visible while the scan
  // runs (which can take 30-60s on big workspaces).
  const [scanning, setScanning] = useState(false);
  const [scanMessage, setScanMessage] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [openMenuFor, setOpenMenuFor] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const list = await listChanges(project.project_id);
      setFiles(list);
      // Default: all selected
      const next: Record<string, boolean> = {};
      list.forEach((f) => (next[f.local_path] = true));
      setSelected(next);
    } catch (err) {
      const e = err as { body?: string; title?: string };
      setErrorMessage(e?.body ?? e?.title ?? "Could not load changes.");
    } finally {
      setLoading(false);
    }
  }, [project.project_id]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Close ︙ menu on outside click
  useEffect(() => {
    function handler(e: MouseEvent) {
      if (
        menuRef.current &&
        !menuRef.current.contains(e.target as Node)
      ) {
        setOpenMenuFor(null);
      }
    }
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  const selectedCount = useMemo(
    () => files.filter((f) => selected[f.local_path]).length,
    [files, selected],
  );

  const totalBytes = useMemo(
    () =>
      files
        .filter((f) => selected[f.local_path])
        .reduce((acc, f) => acc + (f.size ?? 0), 0),
    [files, selected],
  );

  function toggleAll(checked: boolean) {
    const next: Record<string, boolean> = {};
    files.forEach((f) => (next[f.local_path] = checked));
    setSelected(next);
  }

  function toggleOne(path: string) {
    setSelected((s) => ({ ...s, [path]: !s[path] }));
  }

  function selectOnly(statuses: FileStatus[]) {
    const next: Record<string, boolean> = {};
    files.forEach((f) => (next[f.local_path] = statuses.includes(f.status)));
    setSelected(next);
  }

  async function handleSubmit() {
    if (selectedCount === 0) return;
    if (!description.trim()) {
      setErrorMessage("Add a short description of what changed.");
      return;
    }
    setSubmitting(true);
    setErrorMessage(null);
    try {
      const picked = files
        .filter((f) => selected[f.local_path])
        .map((f) => f.local_path);
      const cl = await submitChanges(project.project_id, picked, description.trim());
      onSubmitted(cl);
    } catch (err) {
      const e = err as { body?: string; title?: string };
      setErrorMessage(e?.body ?? e?.title ?? "Submit failed.");
    } finally {
      setSubmitting(false);
    }
  }

  // v0.6.5 — "Scan for new/changed files" button handler. Runs
  // `p4 reconcile //...` server-side, which OPENS files for add/edit/
  // delete based on disk-vs-depot comparison. The list then re-renders
  // through `listChanges`'s `p4 opened` query.
  //
  // The Unreal Editor's source-control plugin already auto-runs
  // `p4 edit` on checkout and `p4 add` on new assets in `Content/`,
  // so this button is mostly for files Unreal doesn't see: assets
  // dragged into `sharedassets/` via Windows Explorer, manual notepad
  // edits to `Config/Default*.ini`, etc.
  async function handleScan() {
    setScanning(true);
    setScanMessage(null);
    setErrorMessage(null);
    try {
      const opened = await reconcileWorkspace(project.project_id);
      if (opened === 0) {
        setScanMessage(
          "No new or changed files found outside what's already tracked.",
        );
      } else {
        setScanMessage(
          `Opened ${opened} file${opened === 1 ? "" : "s"} for add / edit / delete.`,
        );
      }
      await refresh();
    } catch (err) {
      const e = err as { body?: string; title?: string };
      setErrorMessage(
        e?.body ?? e?.title ?? "Could not scan for unmanaged files.",
      );
    } finally {
      setScanning(false);
    }
  }

  async function handleRestore(localPath: string) {
    setOpenMenuFor(null);
    try {
      await restoreFile(project.project_id, localPath);
      await refresh();
    } catch (err) {
      const e = err as { body?: string; title?: string };
      setErrorMessage(e?.body ?? "Could not restore that file.");
    }
  }

  async function handleReveal(localPath: string) {
    setOpenMenuFor(null);
    try {
      await revealInExplorer(localPath);
    } catch (err) {
      console.error(err);
    }
  }

  function copyPath(localPath: string) {
    setOpenMenuFor(null);
    navigator.clipboard.writeText(localPath).catch(() => {});
  }

  return (
    <>
      <h1 className="page-title">
        Your <em>local changes</em>
      </h1>
      <p className="page-subtitle">
        {loading
          ? "Looking at your project folder…"
          : files.length === 0
            ? "Nothing to send — everything you have matches the server."
            : `${files.length} file${files.length === 1 ? "" : "s"} differ from the server. Pick which ones to submit, discard the ones you don't want to keep, and leave the rest as work-in-progress.`}
      </p>

      {errorMessage && (
        <div
          style={{
            border: "1px solid var(--danger)",
            background: "rgba(196, 108, 90, 0.08)",
            padding: "12px 14px",
            borderRadius: "var(--radius-s)",
            fontSize: 12,
            color: "var(--cream)",
            marginBottom: 14,
          }}
        >
          {errorMessage}
        </div>
      )}

      {/* v0.6.5 — Scan-for-unmanaged-files action row. Sits above the
          change-table so it's visible even when there are zero current
          changes. The neutral tone reflects that it's an opt-in cost
          (takes 30-60s on big workspaces) rather than a default action. */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          marginBottom: 14,
          padding: "10px 12px",
          border: "1px solid var(--warm-gray)",
          borderRadius: "var(--radius-s)",
          background: "rgba(212, 180, 122, 0.04)",
        }}
      >
        <div style={{ flex: 1, fontSize: 12, color: "var(--cream-dim)" }}>
          <strong style={{ color: "var(--cream)" }}>
            Missing a file you added outside Unreal?
          </strong>{" "}
          Click <em>Scan</em> to find files dragged into the project folder
          (e.g. into <code>sharedassets/</code>) that aren't yet tracked.
          Takes about 30-60 seconds on a large project.
        </div>
        <button
          className="row-act"
          onClick={handleScan}
          disabled={scanning || loading || submitting}
          style={{ minWidth: 130 }}
        >
          {scanning ? "Scanning…" : "Scan for files"}
        </button>
      </div>

      {scanMessage && (
        <div
          style={{
            border: "1px solid var(--gold)",
            background: "rgba(212, 180, 122, 0.10)",
            padding: "10px 14px",
            borderRadius: "var(--radius-s)",
            fontSize: 12,
            color: "var(--cream)",
            marginBottom: 14,
          }}
        >
          {scanMessage}
        </div>
      )}

      {files.length > 0 && (
        <>
          <div className="selection-bar">
            <span className="count">
              <strong>{selectedCount}</strong> of {files.length} selected
            </span>
            <span style={{ color: "var(--warm-gray)" }}>·</span>
            <span style={{ color: "var(--cream-dim)" }}>
              {formatBytes(totalBytes)} will be uploaded
            </span>
            <div className="quick-actions">
              <button className="row-act" onClick={() => toggleAll(true)}>
                Select all
              </button>
              <button className="row-act" onClick={() => toggleAll(false)}>
                Select none
              </button>
              <button className="row-act" onClick={() => selectOnly(["M"])}>
                Only modified
              </button>
            </div>
          </div>

          <div className="changes-table">
            <div className="changes-head">
              <span>
                <input
                  type="checkbox"
                  checked={selectedCount === files.length}
                  ref={(el) => {
                    if (el)
                      el.indeterminate =
                        selectedCount > 0 && selectedCount < files.length;
                  }}
                  onChange={(e) => toggleAll(e.target.checked)}
                />
              </span>
              <span></span>
              <span>File</span>
              <span style={{ textAlign: "right" }}>Actions</span>
            </div>

            {files.map((f) => {
              const isSel = !!selected[f.local_path];
              const menuOpen = openMenuFor === f.local_path;
              return (
                <div
                  className={`change-row ${isSel ? "" : "unchecked"}`}
                  key={f.local_path}
                >
                  <input
                    type="checkbox"
                    checked={isSel}
                    onChange={() => toggleOne(f.local_path)}
                  />
                  <span className={`glyph-cell ${f.status}`}>
                    {STATUS_GLYPH[f.status]}
                  </span>
                  <div className="path-cell" title={f.local_path}>
                    {workspaceRelative(f.local_path, project.workspace_root)}
                    <span className="size">
                      {f.status === "D"
                        ? "deleted"
                        : f.size != null
                          ? formatBytes(f.size)
                          : "—"}
                    </span>
                  </div>
                  <div
                    className="action-cell"
                    style={{ position: "relative" }}
                  >
                    <button
                      className="row-act"
                      title="More actions"
                      onClick={() =>
                        setOpenMenuFor(menuOpen ? null : f.local_path)
                      }
                    >
                      ⋮
                    </button>
                    {menuOpen && (
                      <div
                        ref={menuRef}
                        style={{
                          position: "absolute",
                          right: 0,
                          top: "100%",
                          marginTop: 4,
                          background: "var(--ink)",
                          border: "1px solid var(--rule)",
                          borderRadius: "var(--radius-s)",
                          minWidth: 220,
                          zIndex: 20,
                          boxShadow:
                            "0 12px 36px rgba(0,0,0,0.6)",
                          padding: 4,
                        }}
                      >
                        <MenuItem onClick={() => handleReveal(f.local_path)}>
                          Reveal in Explorer
                        </MenuItem>
                        <MenuItem onClick={() => handleRestore(f.local_path)}>
                          Restore from server
                          <span
                            style={{
                              color: "var(--warm-gray)",
                              fontSize: 10,
                              marginLeft: 6,
                            }}
                          >
                            (undo my changes)
                          </span>
                        </MenuItem>
                        <MenuItem disabled title="Available in v0.6">
                          Show history…
                        </MenuItem>
                        <MenuItem onClick={() => copyPath(f.local_path)}>
                          Copy path
                        </MenuItem>
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          <div
            style={{
              marginTop: 6,
              fontSize: 11,
              color: "var(--warm-gray)",
              fontStyle: "italic",
              fontFamily: "var(--font-serif)",
            }}
          >
            <em>Restore from server</em> throws away your local edits to a file
            and pulls back the server's version. Files you uncheck stay on your
            computer as work-in-progress until next time.
          </div>

          <div className="section-label" style={{ marginTop: 22 }}>
            D E S C R I B E &nbsp; W H A T &nbsp; C H A N G E D
          </div>
          <div className="card">
            <textarea
              placeholder="e.g. SHOT_001 — locked the v04 board, removed scratch wav, added director notes for SHOT_002"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              style={{ minHeight: 80 }}
            />
          </div>

          <div className="action-bar">
            <button
              className="btn"
              onClick={handleSubmit}
              disabled={submitting || selectedCount === 0}
            >
              <span className="glyph">⇡</span>{" "}
              {submitting ? "Submitting…" : `Submit ${selectedCount} files`}
            </button>
            <button
              className="btn btn-ghost"
              onClick={onBack}
              disabled={submitting}
            >
              Cancel
            </button>
          </div>

          <div
            style={{
              marginTop: 16,
              fontSize: 11,
              color: "var(--warm-gray)",
              fontFamily: "var(--font-serif)",
              fontStyle: "italic",
            }}
          >
            Most submissions go through Unreal Engine. This screen is the
            fallback for files you edited outside Unreal — storyboards, audio,
            notes.
          </div>
        </>
      )}

      {files.length === 0 && !loading && (
        <div style={{ marginTop: 22 }}>
          <button className="btn btn-ghost" onClick={onBack}>
            ‹ Back to project
          </button>
        </div>
      )}
    </>
  );
}

function MenuItem({
  children,
  onClick,
  disabled,
  title,
}: {
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      title={title}
      style={{
        display: "block",
        width: "100%",
        background: "transparent",
        color: disabled ? "var(--warm-gray)" : "var(--cream)",
        border: "none",
        textAlign: "left",
        padding: "8px 10px",
        fontSize: 12,
        cursor: disabled ? "not-allowed" : "pointer",
        borderRadius: "var(--radius-s)",
        fontFamily: "var(--font-sans)",
      }}
      onMouseEnter={(e) => {
        if (!disabled)
          (e.currentTarget as HTMLButtonElement).style.background =
            "var(--ink-2)";
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLButtonElement).style.background = "transparent";
      }}
    >
      {children}
    </button>
  );
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function workspaceRelative(localPath: string, workspaceRoot: string): string {
  const norm = (s: string) => s.replace(/[/\\]+/g, "\\");
  const lp = norm(localPath);
  const wr = norm(workspaceRoot);
  if (lp.toLowerCase().startsWith(wr.toLowerCase())) {
    return lp.slice(wr.length).replace(/^\\+/, "");
  }
  return lp;
}
