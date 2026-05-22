import { useEffect, useState } from "react";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { applyInvite } from "../lib/invoke";
import { ErrorBox } from "../components/ErrorBox";
import { StepList, StepRecord } from "../components/StepList";
import type {
  FriendlyError,
  InviteData,
  OnboardingEvent,
  OnboardingStepId,
  Project,
} from "../lib/types";

const ALL_STEPS: OnboardingStepId[] = [
  "parse",
  "service",
  "connect",
  "server",
  "ticket",
  "workspace",
  "sync",
];

interface Props {
  invite: InviteData;
  code: string;
  workspaceRoot: string;
  onComplete: (project: Project) => void;
  onBack: () => void;
}

export function Connecting({
  invite,
  code,
  workspaceRoot,
  onComplete,
  onBack,
}: Props) {
  const [steps, setSteps] = useState<StepRecord[]>(
    ALL_STEPS.map((id) => ({ id, status: "pending" as const })),
  );
  const [lastSyncLine, setLastSyncLine] = useState<string>("");
  const [syncCount, setSyncCount] = useState<number>(0);
  const [error, setError] = useState<FriendlyError | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | null = null;

    listen<OnboardingEvent>("onboarding-event", (event) => {
      const ev = event.payload;
      if (ev.kind === "step") {
        setSteps((prev) =>
          prev.map((s) =>
            s.id === ev.step
              ? { ...s, status: ev.status, detail: ev.detail ?? undefined }
              : s,
          ),
        );
      } else if (ev.kind === "sync_progress") {
        setLastSyncLine(ev.line);
        setSyncCount((n) => n + 1);
      } else if (ev.kind === "complete") {
        if (!cancelled) onComplete(ev.project);
      } else if (ev.kind === "failed") {
        setSteps((prev) =>
          prev.map((s) =>
            s.id === ev.step ? { ...s, status: "failed" } : s,
          ),
        );
        setError(ev.error);
      }
    }).then((fn) => {
      unlisten = fn;
    });

    applyInvite(code, workspaceRoot).catch((err) => {
      // The error event handler above will populate the UI; this catch
      // just stops the rejection from being unhandled in the console.
      const friendly = err as FriendlyError;
      if (friendly && friendly.title) {
        setError(friendly);
      }
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <>
      <h1 className="page-title">
        Setting up <em>{invite.project_name}</em>
      </h1>
      <p className="page-subtitle">
        Sit tight — this takes 2–5 minutes on a healthy connection.
      </p>

      <div className="card">
        <StepList steps={steps} />
        {syncCount > 0 && (
          <div
            style={{
              marginTop: 14,
              fontFamily: "var(--font-mono)",
              fontSize: 11,
              color: "var(--cream-dim)",
              borderTop: "1px solid var(--rule)",
              paddingTop: 10,
            }}
          >
            {syncCount} files · {truncate(lastSyncLine, 90)}
          </div>
        )}
      </div>

      {error && (
        <>
          <ErrorBox error={error} />
          <div style={{ marginTop: 16 }}>
            <button className="btn btn-ghost" onClick={onBack}>
              Try again
            </button>
          </div>
        </>
      )}
    </>
  );
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return "…" + s.slice(s.length - max);
}
