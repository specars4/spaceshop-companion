interface Props {
  onConfirm: () => void;
  onCancel: () => void;
}

/// Confirm dialog for the Repair workspace action.
///
/// Repair runs `p4 clean //...` which removes ANY file in the workspace
/// folder that isn't tracked by the depot AND isn't currently `p4 opened`.
/// In practice that means: scratch notes the contractor dropped in, screen
/// recordings, render outputs sitting in non-ignored subfolders, etc.
/// Files explicitly opened for add/edit in p4 ARE safe.
///
/// We don't require a typed YES gate here (unlike Force re-download) —
/// repair is less destructive than a full force-resync because pending
/// changelists are preserved. But the user does need to know the orphan
/// rule before clicking. One explicit confirm click on this modal.
export function RepairConfirm({ onConfirm, onCancel }: Props) {
  return (
    <>
      <div className="fr-backdrop" onClick={onCancel} />
      <div className="fr-modal" role="dialog" aria-modal="true">
        <div
          className="fr-danger-flag"
          style={{ color: "var(--gold)" }}
        >
          ●&nbsp; R E A D &nbsp; B E F O R E &nbsp; Y O U &nbsp; C L I C K
        </div>
        <h2 className="fr-title">
          Repair this <em>workspace?</em>
        </h2>
        <div className="fr-body">
          Companion will remove every file in the project folder that
          isn't tracked by the server, then pull anything that's missing
          back down fresh.
        </div>

        <div
          style={{
            fontSize: 13,
            color: "var(--cream)",
            fontFamily: "var(--font-serif)",
            lineHeight: 1.6,
            marginBottom: 16,
          }}
        >
          <div style={{ marginBottom: 10 }}>
            <strong style={{ color: "var(--gold)" }}>Safe:</strong>{" "}
            files you've opened for add or edit in p4 (anything that
            shows up in <em>Review &amp; submit</em>) — those stay.
          </div>
          <div>
            <strong style={{ color: "var(--danger)" }}>Deleted:</strong>{" "}
            anything you dropped into the project folder but never
            added to p4 — notes, screenshots, scratch exports, render
            outputs. If you have stuff like that you want to keep,
            cancel and either submit it or move it somewhere outside
            the project folder first.
          </div>
        </div>

        <div style={{ display: "flex", gap: 12, marginTop: 18 }}>
          <button
            className="btn"
            onClick={onConfirm}
            style={{
              background: "var(--gold)",
              color: "var(--ink)",
              borderColor: "var(--gold)",
            }}
          >
            Yes — repair workspace
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
          border: 1px solid var(--gold-dim);
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
