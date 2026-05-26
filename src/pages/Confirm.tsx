import { useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { InviteData } from "../lib/types";

interface Props {
  invite: InviteData;
  onCancel: () => void;
  onConnect: (workspaceRoot: string) => void;
}

const USERPROFILE_PLACEHOLDER = "%USERPROFILE%";

function expandLocalRoot(raw: string): string {
  // Best-effort expansion in the renderer — falls back to literal if we
  // can't resolve. Real expansion happens on the Rust side.
  if (raw.includes(USERPROFILE_PLACEHOLDER)) {
    // We can't read env in webview; fake a reasonable hint.
    return raw.replace(/%[A-Z_]+%/g, "<home>");
  }
  return raw;
}

export function Confirm({ invite, onCancel, onConnect }: Props) {
  const [folder, setFolder] = useState<string>("");
  const [hint] = useState<string>(expandLocalRoot(invite.perforce.local_root_default));

  useEffect(() => {
    // Pre-populate the folder field with the invite's default; the user
    // can either accept it (we let Rust expand %USERPROFILE%) or pick a
    // different folder via the OS dialog.
    setFolder(invite.perforce.local_root_default);
  }, [invite]);

  // SELF-ONBOARD heuristic: an invite whose `tailscale.auth_key` is
  // empty was minted by Workshop's SELF-ONBOARD INVITE flow, which
  // means the workspace + folder already exist server-side and
  // changing the folder here will cause Companion to either fight
  // the existing workspace's Root: or no-op-sync into the wrong
  // place. Warn (don't block) so the operator can override
  // intentionally for non-standard layouts.
  const isSelfOnboardShape = invite.tailscale.auth_key.trim() === "";
  const folderDiffersFromInvite =
    folder.trim() !== "" &&
    folder.trim() !== invite.perforce.local_root_default.trim();
  const showSelfOnboardDriftWarning =
    isSelfOnboardShape && folderDiffersFromInvite;

  async function pickFolder() {
    const picked = await openDialog({
      directory: true,
      multiple: false,
      title: "Where should this project live?",
    });
    if (typeof picked === "string") setFolder(picked);
  }

  return (
    <>
      <h1 className="page-title">
        Set up <em>{invite.project_name}</em>
      </h1>
      <p className="page-subtitle">
        Invite from {invite.issued_by}. Pick a folder and Companion will set
        up the connection and download your project.
      </p>

      <div className="section-label">P R O J E C T &nbsp; F O L D E R</div>
      <div className="card">
        <div style={{ marginBottom: 8, fontSize: 12, color: "var(--cream-dim)" }}>
          Your project files will be downloaded into this folder.
        </div>
        <input
          value={folder}
          onChange={(e) => setFolder(e.target.value)}
          placeholder={hint}
          spellCheck={false}
        />
        <div style={{ marginTop: 12, display: "flex", gap: 12 }}>
          <button className="btn btn-ghost btn-small" onClick={pickFolder}>
            Choose folder…
          </button>
        </div>
        {showSelfOnboardDriftWarning && (
          <div
            style={{
              marginTop: 12,
              padding: "10px 12px",
              border: "1px solid var(--gold)",
              borderRadius: 4,
              background: "rgba(212, 180, 122, 0.08)",
              fontSize: 12,
              color: "var(--cream)",
              lineHeight: 1.5,
            }}
          >
            <strong style={{ color: "var(--gold)" }}>
              Folder differs from invite default.
            </strong>{" "}
            This looks like a SELF-ONBOARD invite (no Tailscale auth
            key — your machine is already on the tailnet). The
            workspace + folder above already exist on the Perforce
            server. Pointing Companion at a different folder will
            either rewrite the existing workspace's Root: or sync into
            the wrong place. If your workspace was set up by Workshop's
            MIGRATE TO PERFORCE on the suggested folder, leave the
            field as-is. Override only if your layout differs from
            what Workshop expected.
          </div>
        )}
      </div>

      <div className="section-label">W H A T &nbsp; H A P P E N S &nbsp; N E X T</div>
      <div className="card">
        <ol
          style={{
            paddingLeft: 22,
            margin: 0,
            color: "var(--cream-dim)",
            fontSize: 13,
            lineHeight: 1.8,
          }}
        >
          <li>Windows will ask for permission once. Click <strong>Yes</strong>.</li>
          <li>Companion sets up the connection — about 2–5 minutes.</li>
          <li>Your project files download into the folder above.</li>
          <li>
            Companion shows you what to paste into <em>Unreal</em> so editing
            assets just works.
          </li>
        </ol>
      </div>

      <div style={{ marginTop: 28, display: "flex", gap: 12 }}>
        <button className="btn" onClick={() => onConnect(folder)}>
          Connect
        </button>
        <button className="btn btn-ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </>
  );
}
