import { useEffect, useState } from "react";
import {
  cleanUninstall,
  detectTailscaleOrigin,
  type TailscaleOrigin,
} from "../lib/invoke";

interface Props {
  onCancel: () => void;
}

/// Clean Uninstall modal. Modeled on RepairConfirm but danger-toned and
/// gated by a typed YES — this is catastrophic and irreversible (the
/// MSI uninstaller fires at the end and Companion exits).
///
/// On mount we ask the backend whether Tailscale is on the machine and
/// whether *we* installed it. That determines the default state of the
/// "Also uninstall Tailscale" checkbox:
///
///   InstalledByUs → checked, label "we installed it for you"
///   PreExisting   → unchecked, label "you had it before — leaving
///                   it is the safe choice"
///   NotInstalled  → checkbox hidden entirely
///   Unknown       → unchecked, label "we can't tell — leaving it
///                   is the safe choice"
///
/// See `detect_tailscale_origin` in src-tauri/src/commands/uninstall.rs
/// for the heuristic details (install-flag check, then registry
/// InstallDate same-day match).
export function UninstallConfirm({ onCancel }: Props) {
  const [origin, setOrigin] = useState<TailscaleOrigin | null>(null);
  const [removeTailscale, setRemoveTailscale] = useState(false);
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Final-status text shown in the brief window between the IPC reply
  // and Companion's auto-exit. Surfaces the "we kicked off the MSI
  // uninstaller in the background" vs. "you'll need to finish in
  // Windows Settings" branch.
  const [done, setDone] = useState<string | null>(null);

  const armed = typed.trim().toLowerCase() === "yes" && !running;

  useEffect(() => {
    let cancelled = false;
    detectTailscaleOrigin()
      .then((o) => {
        if (cancelled) return;
        setOrigin(o);
        // Default-ON only when we know we installed it. Every other
        // state (PreExisting / Unknown) defaults OFF — leaving
        // Tailscale alone is the conservative choice.
        setRemoveTailscale(o === "installedByUs");
      })
      .catch(() => {
        if (cancelled) return;
        setOrigin("unknown");
        setRemoveTailscale(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function handleConfirm() {
    if (!armed) return;
    setRunning(true);
    setError(null);
    try {
      const result = await cleanUninstall(removeTailscale);
      // Companion will exit ~750ms after this resolves. Show a friendly
      // "what happened" message so the last beat of UI isn't blank.
      if (result.fellBackToSettings) {
        setDone(
          "Spaceshop data cleared. Companion couldn't find its own installer entry — Windows Settings has been opened so you can click Uninstall there.",
        );
      } else if (result.companionUninstallFired) {
        setDone(
          "Spaceshop data cleared. Companion's uninstaller is running in the background — this window will close in a moment.",
        );
      } else {
        setDone(
          "Spaceshop data cleared. You can remove Companion from Settings → Apps & Features when ready.",
        );
      }
    } catch (err) {
      const e = err as { title?: string; body?: string; details?: string };
      setError(e?.body ?? e?.title ?? "Uninstall failed.");
      setRunning(false);
    }
  }

  const tailscaleLabel = (() => {
    switch (origin) {
      case "installedByUs":
        return "Also uninstall Tailscale (we installed it for you)";
      case "preExisting":
        return "Also uninstall Tailscale (you had it before Companion — leaving it is the safe choice)";
      case "unknown":
        return "Also uninstall Tailscale (we can't tell if we installed it — leaving it is the safe choice)";
      default:
        return "";
    }
  })();

  return (
    <>
      <div className="fr-backdrop" onClick={running ? undefined : onCancel} />
      <div className="fr-modal" role="dialog" aria-modal="true">
        <div className="fr-danger-flag">
          ●&nbsp; T H I S &nbsp; C A N N O T &nbsp; B E &nbsp; U N D O N E
        </div>
        <h2 className="fr-title">
          Uninstall <em>Spaceshop Companion?</em>
        </h2>
        <div className="fr-body">
          Companion will remove every trace of itself from this computer,
          then start the Windows uninstaller. Your project files on disk
          and your access to the server are not touched.
        </div>

        <div
          style={{
            fontSize: 12.5,
            color: "var(--cream-dim)",
            fontFamily: "var(--font-serif)",
            lineHeight: 1.65,
            marginBottom: 14,
          }}
        >
          <div
            style={{
              fontSize: 10,
              color: "var(--warm-gray)",
              textTransform: "uppercase",
              letterSpacing: "var(--tracking-caps)",
              marginBottom: 8,
            }}
          >
            What gets removed
          </div>
          <ul style={{ paddingLeft: 18, margin: 0 }}>
            <li>
              Your saved projects and settings
              <span style={{ color: "var(--warm-gray)" }}>
                {" "}
                (%LOCALAPPDATA%\Spaceshop\, %APPDATA%\Spaceshop\)
              </span>
            </li>
            <li>
              Companion's saved server pass
              <span style={{ color: "var(--warm-gray)" }}>
                {" "}
                (just our line in p4tickets.txt — other apps' tickets stay)
              </span>
            </li>
            <li>
              The bundled <code>p4.exe</code> copy Companion uses internally
            </li>
            <li>The Companion app itself (via the Windows uninstaller)</li>
          </ul>
        </div>

        {origin === null ? (
          <div
            style={{
              fontSize: 11,
              color: "var(--warm-gray)",
              fontStyle: "italic",
              fontFamily: "var(--font-serif)",
              marginBottom: 14,
            }}
          >
            Checking Tailscale install…
          </div>
        ) : origin !== "notInstalled" ? (
          <label
            style={{
              display: "flex",
              gap: 10,
              alignItems: "flex-start",
              padding: "10px 12px",
              border: "1px solid var(--rule)",
              borderRadius: "var(--radius-s)",
              background: "var(--ink-2)",
              marginBottom: 14,
              cursor: "pointer",
              fontSize: 12.5,
              fontFamily: "var(--font-serif)",
              color: "var(--cream)",
              lineHeight: 1.5,
            }}
          >
            <input
              type="checkbox"
              checked={removeTailscale}
              onChange={(e) => setRemoveTailscale(e.target.checked)}
              disabled={running}
              style={{ marginTop: 3 }}
            />
            <span>{tailscaleLabel}</span>
          </label>
        ) : null}

        {done ? (
          <div
            style={{
              padding: "12px 14px",
              background: "rgba(127, 168, 115, 0.10)",
              border: "1px solid var(--ok)",
              borderRadius: "var(--radius-s)",
              fontSize: 12.5,
              color: "var(--cream)",
              fontFamily: "var(--font-serif)",
              fontStyle: "italic",
              marginBottom: 4,
            }}
          >
            {done}
          </div>
        ) : (
          <>
            <div
              style={{
                fontSize: 12,
                color: "var(--cream-dim)",
                fontFamily: "var(--font-serif)",
                fontStyle: "italic",
                marginBottom: 8,
              }}
            >
              Type{" "}
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
            </div>
            <input
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder="Type YES to confirm"
              autoFocus
              disabled={running}
              style={{ marginBottom: 8 }}
            />
            {error && (
              <div
                style={{
                  fontSize: 11.5,
                  color: "var(--danger)",
                  fontFamily: "var(--font-serif)",
                  marginBottom: 8,
                }}
              >
                {error}
              </div>
            )}
            <div style={{ display: "flex", gap: 12, marginTop: 18 }}>
              <button
                className="btn btn-danger"
                style={
                  armed
                    ? {
                        background: "var(--danger)",
                        color: "var(--ink)",
                        borderColor: "var(--danger)",
                      }
                    : undefined
                }
                onClick={handleConfirm}
                disabled={!armed}
              >
                {running ? "Uninstalling…" : "Yes — uninstall Companion"}
              </button>
              <button
                className="btn btn-ghost"
                onClick={onCancel}
                disabled={running}
              >
                Cancel
              </button>
            </div>
          </>
        )}
      </div>
      <style>{`
        .fr-backdrop {
          position: fixed; inset: 0;
          background: rgba(15, 14, 13, 0.78);
          backdrop-filter: blur(2px);
          z-index: 50;
        }
        .fr-modal {
          position: fixed;
          top: 50%; left: 50%;
          transform: translate(-50%, -50%);
          width: 560px; max-width: calc(100vw - 48px);
          background: var(--ink);
          border: 1px solid var(--danger);
          border-radius: var(--radius-m);
          padding: 32px 36px 28px 36px;
          z-index: 60;
          box-shadow: 0 30px 80px rgba(0,0,0,0.6);
        }
        .fr-danger-flag {
          font-size: 10px; color: var(--danger);
          text-transform: uppercase; letter-spacing: var(--tracking-caps-wide);
          margin-bottom: 14px;
        }
        .fr-title {
          font-family: var(--font-serif); font-weight: 400; font-size: 24px;
          margin: 0 0 12px 0;
        }
        .fr-title em { font-style: italic; color: var(--cream-dim); }
        .fr-body {
          font-family: var(--font-serif); font-size: 14px;
          color: var(--cream); line-height: 1.7;
          margin-bottom: 16px;
        }
      `}</style>
    </>
  );
}
