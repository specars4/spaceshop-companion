import { ReactNode } from "react";
import { UpdateBanner } from "./UpdateBanner";

interface Props {
  subtitle?: string;
  children: ReactNode;
}

export function Shell({ subtitle = "C O M P A N I O N", children }: Props) {
  return (
    <div className="app">
      <UpdateBanner />
      <header className="header">
        <span className="bullet">●</span>
        <span className="wordmark">
          Spaceshop <em>Studios</em>
        </span>
        <span className="subtitle">{subtitle}</span>
      </header>
      <main className="app-main">{children}</main>
    </div>
  );
}
