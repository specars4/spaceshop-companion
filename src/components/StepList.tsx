import { OnboardingStepId, StepStatus, STEP_LABELS } from "../lib/types";

export interface StepRecord {
  id: OnboardingStepId;
  status: StepStatus | "pending";
  detail?: string | null;
}

interface Props {
  steps: StepRecord[];
}

export function StepList({ steps }: Props) {
  return (
    <ol className="steps">
      {steps.map((step) => {
        const isPending = step.status === "pending";
        const isDone = step.status === "done";
        const isFailed = step.status === "failed";
        const isRunning = step.status === "running" || step.status === "installing";
        const dotClass = isDone
          ? "done"
          : isFailed
            ? "failed"
            : isRunning
              ? "running"
              : "";
        const labelClass = isPending ? "pending" : "";
        const glyph = isDone ? "✓" : isFailed ? "!" : "";
        return (
          <li key={step.id}>
            <span className={`step-dot ${dotClass}`}>{glyph}</span>
            <span className={`step-label ${labelClass}`}>
              {STEP_LABELS[step.id]}
            </span>
            <span className="step-detail">{step.detail ?? ""}</span>
          </li>
        );
      })}
    </ol>
  );
}
