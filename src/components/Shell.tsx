import { ReactNode, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { UpdateBanner } from "./UpdateBanner";

interface Props {
  subtitle?: string;
  children: ReactNode;
}

// Render "0.5.9" as "v 0 . 5 . 9" — every glyph spaced like the rest of
// the small-caps subtitle so it sits flush with "C O M P A N I O N".
function spaceVersion(v: string): string {
  return "v " + v.split("").join(" ");
}

/// Matches the Rust-side `serde_json::json!({...})` emitted from
/// lib.rs::setup() when ensure_binaries fails. Treat all fields as
/// optional defensively in case the backend ever evolves the shape.
interface StartupError {
  kind?: string;
  title?: string;
  body?: string;
  details?: string;
}

export function Shell({ subtitle, children }: Props) {
  const [version, setVersion] = useState<string | null>(null);
  const [startupError, setStartupError] = useState<StartupError | null>(null);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  useEffect(() => {
    const unlisten = listen<StartupError>("startup-error", (event) => {
      setStartupError(event.payload ?? { title: "Companion can't find its tools." });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const computedSubtitle =
    subtitle ??
    (version
      ? `C O M P A N I O N   ·   ${spaceVersion(version)}`
      : "C O M P A N I O N");

  return (
    <div className="app">
      {startupError && <StartupErrorBanner error={startupError} />}
      <UpdateBanner />
      <header className="header">
        <span className="bullet">●</span>
        <span className="wordmark">
          Spaceshop <em>Studios</em>
        </span>
        <span className="subtitle">{computedSubtitle}</span>
      </header>
      <main className="app-main">{children}</main>
    </div>
  );
}

function StartupErrorBanner({ error }: { error: StartupError }) {
  function handleReinstall(e: React.MouseEvent) {
    e.preventDefault();
    openUrl("https://specars4.github.io/onboard/").catch(() => {
      // Fall back to a plain window.open if the opener plugin fails.
      window.open("https://specars4.github.io/onboard/", "_blank");
    });
  }

  const title = error.title ?? "Companion can't find its tools. Please reinstall.";
  const body = error.body ?? "";
  const details = error.details ?? "";

  return (
    <div
      role="alert"
      style={{
        background: "rgba(196, 108, 90, 0.18)",
        borderBottom: "2px solid var(--danger)",
        padding: "16px 48px",
        display: "flex",
        alignItems: "center",
        gap: 20,
        fontSize: 14,
        color: "var(--cream)",
      }}
    >
      <span
        style={{
          color: "var(--danger)",
          textTransform: "uppercase",
          letterSpacing: "var(--tracking-caps)",
          fontSize: 11,
          fontWeight: 600,
          whiteSpace: "nowrap",
        }}
      >
        ● Fatal error
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontFamily: "var(--font-serif)",
            fontSize: 15,
            marginBottom: 2,
          }}
        >
          {title}
        </div>
        {body && (
          <div
            style={{
              fontSize: 12,
              color: "var(--cream-dim)",
              marginBottom: 4,
            }}
          >
            {body}
          </div>
        )}
        {details && (
          <div
            style={{
              fontSize: 11,
              color: "var(--cream-dim)",
              fontFamily: "var(--font-mono)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
            title={details}
          >
            {details}
          </div>
        )}
      </div>
      <a
        href="https://specars4.github.io/onboard/"
        onClick={handleReinstall}
        style={{
          background: "var(--danger)",
          color: "var(--ink)",
          padding: "10px 18px",
          borderRadius: "var(--radius-s)",
          textDecoration: "none",
          fontSize: 12,
          textTransform: "uppercase",
          letterSpacing: "var(--tracking-caps)",
          fontWeight: 600,
          whiteSpace: "nowrap",
        }}
      >
        Reinstall
      </a>
    </div>
  );
}
