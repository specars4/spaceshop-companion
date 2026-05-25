import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Banner } from "./Banner";

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

  const title = (
    <>
      v{info.current_version} →{" "}
      <strong style={{ color: "var(--cream)" }}>v{info.new_version}</strong>
      {info.notes && (
        <span
          style={{
            marginLeft: 10,
            fontStyle: "italic",
            color: "var(--cream-dim)",
          }}
        >
          — {info.notes.split("\n")[0].slice(0, 80)}
        </span>
      )}
    </>
  );

  return (
    <Banner severity="info" label="Update available" title={title}>
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
    </Banner>
  );
}
