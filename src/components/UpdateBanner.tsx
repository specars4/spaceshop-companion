import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface UpdateInfo {
  current_version: string;
  new_version: string;
  notes: string | null;
}

export function UpdateBanner() {
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<UpdateInfo>("update-available", (event) => {
      setInfo(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  async function handleInstall() {
    setInstalling(true);
    setError(null);
    try {
      await invoke("install_update");
      // App will restart; this code likely doesn't run.
    } catch (err) {
      const e = err as { body?: string; title?: string } | string;
      const msg =
        typeof e === "string"
          ? e
          : (e?.body ?? e?.title ?? "Update install failed.");
      setError(msg);
      setInstalling(false);
    }
  }

  if (!info || dismissed) return null;

  return (
    <div
      style={{
        background: "rgba(212, 180, 122, 0.10)",
        borderBottom: "1px solid var(--gold-dim)",
        padding: "10px 48px",
        display: "flex",
        alignItems: "center",
        gap: 16,
        fontSize: 12,
      }}
    >
      <span
        style={{
          color: "var(--gold)",
          textTransform: "uppercase",
          letterSpacing: "var(--tracking-caps)",
          fontSize: 10,
        }}
      >
        ● Update available
      </span>
      <span style={{ color: "var(--cream-dim)" }}>
        v{info.current_version} → <strong style={{ color: "var(--cream)" }}>v{info.new_version}</strong>
        {info.notes && (
          <span style={{ marginLeft: 10, fontStyle: "italic", fontFamily: "var(--font-serif)" }}>
            — {info.notes.split("\n")[0].slice(0, 80)}
          </span>
        )}
      </span>
      <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
        {error && (
          <span style={{ color: "var(--danger)", fontSize: 11 }}>{error}</span>
        )}
        <button
          className="btn btn-small"
          onClick={handleInstall}
          disabled={installing}
        >
          {installing ? "Installing…" : "Install now"}
        </button>
        <button
          className="btn btn-small btn-ghost"
          onClick={() => setDismissed(true)}
          disabled={installing}
        >
          Later
        </button>
      </div>
    </div>
  );
}
