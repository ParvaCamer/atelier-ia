import { useEffect, useState } from "react";
import { useWorld } from "../state/store";
import { useUi } from "../state/ui";
import { worldHandle } from "../world/handle";
import { AgentPanel } from "../ui/AgentPanel";
import { CommandPalette } from "../ui/CommandPalette";
import { Header } from "../ui/Header";
import { Channel } from "../ui/channel/Channel";
import { WorldCanvas } from "../ui/WorldCanvas";
import { Settings } from "../ui/settings/Settings";
import { History } from "../ui/history/History";
import { RenderViewer } from "../ui/renders/RenderViewer";

export function App() {
  const ready = useWorld((s) => s.ready);
  const init = useWorld((s) => s.init);
  const [error, setError] = useState<string | null>(null);
  const collapsed = useUi((s) => s.collapsed);
  const tall = useUi((s) => s.tall);

  useEffect(() => {
    init().catch((e) => setError(String(e)));
  }, [init]);

  // ⌘K / Ctrl+K : la palette, depuis n'importe où — y compris un champ.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        const ui = useUi.getState();
        if (ui.paletteOpen) ui.closePalette(); else ui.openPalette();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (error) {
    return <div className="loading">Le moteur n'a pas démarré : {error}</div>;
  }
  if (!ready) {
    return <div className="loading">Démarrage du moteur…</div>;
  }

  return (
    <div className="app">
      <Header
        onFocusProject={(id) => worldHandle.get()?.focusProject(id)}
        onFrameAll={() => worldHandle.get()?.frameAll()}
      />
      <WorldCanvas />
      {/* Replier masque les panneaux sans les démonter : la discussion n'est
          pas reconstruite au dépliage, et le shell garde sa session. */}
      <div className={collapsed ? "dock collapsed" : tall ? "dock tall" : "dock"}>
        {collapsed && (
          <div className="pane dock-bar">
            <div className="pane-head">
              Agent, discussion, journal
              <button className="dock-toggle" onClick={() => useUi.getState().toggleCollapsed()}>déplier</button>
            </div>
          </div>
        )}
        <AgentPanel />
        <Channel />
      </div>
      <Settings />
      <History />
      <RenderViewer />
      <CommandPalette />
    </div>
  );
}
