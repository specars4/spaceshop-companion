import { ReactNode } from "react";

export type BannerSeverity = "info" | "warn" | "danger";

interface BannerProps {
  severity: BannerSeverity;
  /** Small-caps prefix, e.g. "Update available" or "Fatal error". */
  label?: string;
  /** Main message, rendered in serif at a larger size. */
  title: ReactNode;
  /** Optional secondary line, rendered in cream-dim. */
  body?: ReactNode;
  /** Optional technical detail line, rendered in mono with ellipsis. */
  details?: string;
  /** Right-side slot for action buttons / links. */
  children?: ReactNode;
}

interface SeverityStyle {
  accent: string;          // accent color (label, bullet, bottom border)
  background: string;      // tinted backdrop
  borderBottom: string;    // bottom rule
  labelWeight: number;
}

const SEVERITY_STYLES: Record<BannerSeverity, SeverityStyle> = {
  info: {
    accent: "var(--gold)",
    background: "rgba(212, 180, 122, 0.10)",
    borderBottom: "1px solid var(--gold-dim)",
    labelWeight: 400,
  },
  warn: {
    accent: "var(--gold-dim)",
    background: "rgba(142, 122, 79, 0.14)",
    borderBottom: "1px solid var(--gold-dim)",
    labelWeight: 500,
  },
  danger: {
    accent: "var(--danger)",
    background: "rgba(196, 108, 90, 0.18)",
    borderBottom: "2px solid var(--danger)",
    labelWeight: 600,
  },
};

export function Banner({
  severity,
  label,
  title,
  body,
  details,
  children,
}: BannerProps) {
  const s = SEVERITY_STYLES[severity];
  const isDanger = severity === "danger";

  // Severity drives density: danger banners are "loud" (taller, larger text),
  // info/warn banners are compact. This preserves the pre-refactor look of
  // the two extant call sites (UpdateBanner / StartupErrorBanner) while
  // still routing through one component.
  const padding = isDanger ? "16px 48px" : "10px 48px";
  const baseFontSize = isDanger ? 14 : 12;
  const labelFontSize = isDanger ? 11 : 10;
  const titleFontSize = isDanger ? 15 : 12;

  return (
    <div
      role={isDanger ? "alert" : undefined}
      style={{
        background: s.background,
        borderBottom: s.borderBottom,
        padding,
        display: "flex",
        alignItems: "center",
        gap: isDanger ? 20 : 16,
        fontSize: baseFontSize,
        color: "var(--cream)",
      }}
    >
      {label && (
        <span
          style={{
            color: s.accent,
            textTransform: "uppercase",
            letterSpacing: "var(--tracking-caps)",
            fontSize: labelFontSize,
            fontWeight: s.labelWeight,
            whiteSpace: "nowrap",
          }}
        >
          {"●"} {label}
        </span>
      )}
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            fontFamily: "var(--font-serif)",
            fontSize: titleFontSize,
            marginBottom: body || details ? 2 : 0,
          }}
        >
          {title}
        </div>
        {body && (
          <div
            style={{
              fontSize: 12,
              color: "var(--cream-dim)",
              marginBottom: details ? 4 : 0,
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
      {children && (
        <div
          style={{
            marginLeft: "auto",
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}
