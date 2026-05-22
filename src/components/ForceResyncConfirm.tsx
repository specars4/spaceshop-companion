import { useState } from "react";

interface Props {
  /** Kept in the props for callers' clarity / future reuse; not currently
   *  shown in the modal now that the confirm gate is a plain YES. */
  projectName?: string;
  localChanges: number;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ForceResyncConfirm({
  localChanges,
  onConfirm,
  onCancel,
}: Props) {
  const [typed, setTyped] = useState("");
  const armed = typed.trim().toLowerCase() === "yes";

  return (
    <>
      <div className="fr-backdrop" onClick={onCancel} />
      <div className="fr-modal" role="dialog" aria-modal="true">
        <div className="fr-danger-flag">
          ●&nbsp; T H I S &nbsp; C A N N O T &nbsp; B E &nbsp; U N D O N E
        </div>
        <h2 className="fr-title">
          Force re-download <em>everything?</em>
        </h2>
        <div className="fr-body">
          Companion will overwrite every file in your project folder with the
          server's current copy.{" "}
          <span style={{ color: "var(--danger)", fontStyle: "italic" }}>
            Any local changes you haven't submitted will be permanently lost.
          </span>
        </div>

        {localChanges > 0 && (
          <div
            style={{
              fontSize: 11,
              color: "var(--warm-gray)",
              textTransform: "uppercase",
              letterSpacing: "var(--tracking-caps)",
              marginBottom: 6,
            }}
          >
            {localChanges} local change{localChanges === 1 ? "" : "s"} will be lost
          </div>
        )}

        <div
          style={{
            fontSize: 12,
            color: "var(--cream-dim)",
            fontFamily: "var(--font-serif)",
            fontStyle: "italic",
            marginBottom: 8,
          }}
        >
          If you want to keep any of these, cancel and submit them first.
          Otherwise type <strong style={{ color: "var(--cream)", fontFamily: "var(--font-mono)", fontStyle: "normal" }}>YES</strong> to confirm:
        </div>
        <input
          value={typed}
          onChange={(e) => setTyped(e.target.value)}
          placeholder="Type YES to confirm"
          autoFocus
          style={{ marginBottom: 8 }}
        />

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
            onClick={onConfirm}
            disabled={!armed}
          >
            Yes — overwrite everything
          </button>
          <button className="btn btn-ghost" onClick={onCancel}>
            Cancel
          </button>
        </div>
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
