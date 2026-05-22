import type { FriendlyError } from "../lib/types";

interface Props {
  error: FriendlyError;
}

export function ErrorBox({ error }: Props) {
  return (
    <div className="error-box">
      <div className="err-title">{error.title}</div>
      <div className="err-body">{error.body}</div>
      <div className="err-action">{error.suggested_action}</div>
      {error.details && (
        <details>
          <summary>Show technical details</summary>
          <pre>{error.details}</pre>
        </details>
      )}
    </div>
  );
}
