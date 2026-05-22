import { useState } from "react";

interface Props {
  label: string;
  value: string;
  onCopy?: () => void;
}

export function CopyField({ label, value, onCopy }: Props) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      // Tauri webview supports the clipboard API on Windows; if it's not
      // available we fall back to selection-copy (rare).
      const el = document.createElement("textarea");
      el.value = value;
      document.body.appendChild(el);
      el.select();
      try {
        document.execCommand("copy");
      } finally {
        document.body.removeChild(el);
      }
    }
    setCopied(true);
    onCopy?.();
    window.setTimeout(() => setCopied(false), 1400);
  }

  return (
    <div className="copy-field">
      <span className="copy-field-label">{label}</span>
      <span className="copy-field-value" title={value}>
        {value}
      </span>
      <button
        className="btn btn-small btn-ghost"
        onClick={handleCopy}
        type="button"
      >
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
  );
}
