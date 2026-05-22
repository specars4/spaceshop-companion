import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Shell } from "./components/Shell";
import { Welcome } from "./pages/Welcome";
import { Confirm } from "./pages/Confirm";
import { Connecting } from "./pages/Connecting";
import { ProjectView } from "./pages/Project";
import { Changes } from "./pages/Changes";
import type { InviteData, Project } from "./lib/types";

type Screen =
  | { name: "welcome" }
  | { name: "confirm"; code: string; invite: InviteData }
  | {
      name: "connecting";
      code: string;
      invite: InviteData;
      workspaceRoot: string;
    }
  | { name: "project"; project: Project; fresh: boolean }
  | { name: "changes"; project: Project };

export default function App() {
  const [screen, setScreen] = useState<Screen>({ name: "welcome" });

  useEffect(() => {
    const unlisten = listen<string>("nav", (event) => {
      if (event.payload === "/onboard") {
        setScreen({ name: "welcome" });
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  let body;
  switch (screen.name) {
    case "welcome":
      body = (
        <Welcome
          onInvite={(code, invite) =>
            setScreen({ name: "confirm", code, invite })
          }
          onOpenProject={(project) =>
            setScreen({ name: "project", project, fresh: false })
          }
        />
      );
      break;
    case "confirm":
      body = (
        <Confirm
          invite={screen.invite}
          onCancel={() => setScreen({ name: "welcome" })}
          onConnect={(workspaceRoot) =>
            setScreen({
              name: "connecting",
              code: screen.code,
              invite: screen.invite,
              workspaceRoot,
            })
          }
        />
      );
      break;
    case "connecting":
      body = (
        <Connecting
          invite={screen.invite}
          code={screen.code}
          workspaceRoot={screen.workspaceRoot}
          onComplete={(project) =>
            setScreen({ name: "project", project, fresh: true })
          }
          onBack={() =>
            setScreen({
              name: "confirm",
              code: screen.code,
              invite: screen.invite,
            })
          }
        />
      );
      break;
    case "project":
      body = (
        <ProjectView
          project={screen.project}
          fresh={screen.fresh}
          onBack={() => setScreen({ name: "welcome" })}
          onRemoved={() => setScreen({ name: "welcome" })}
          onOpenChanges={() =>
            setScreen({ name: "changes", project: screen.project })
          }
        />
      );
      break;
    case "changes":
      body = (
        <Changes
          project={screen.project}
          onBack={() =>
            setScreen({ name: "project", project: screen.project, fresh: false })
          }
          onSubmitted={() =>
            setScreen({ name: "project", project: screen.project, fresh: false })
          }
        />
      );
      break;
  }

  return <Shell>{body}</Shell>;
}
