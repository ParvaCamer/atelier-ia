import { useEffect, useState } from "react";
import { useWorld } from "../state/store";
import { worldHandle } from "../world/handle";
import { AgentPanel } from "../ui/AgentPanel";
import { Header } from "../ui/Header";
import { Terminal } from "../ui/Terminal";
import { WorldCanvas } from "../ui/WorldCanvas";
import { Settings } from "../ui/settings/Settings";
import { History } from "../ui/history/History";
import { RenderViewer } from "../ui/renders/RenderViewer";

export function App() {
  const ready = useWorld((s) => s.ready);
  const init = useWorld((s) => s.init);
  const [error, setError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(false);

  useEffect(() => {
    init().catch((e) => setError(String(e)));
  }, [init]);

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
      <div className={collapsed ? "dock collapsed" : "dock"}>
        {collapsed ? (
          <div className="pane" style={{ gridColumn: "1 / -1" }}>
            <div className="pane-head">
              Panneaux
              <button className="dock-toggle" onClick={() => setCollapsed(false)}>
                déplier
              </button>
            </div>
          </div>
        ) : (
          <>
            <AgentPanel />
            <div className="pane" style={{ minWidth: 0 }}>
              <Terminal />
            </div>
          </>
        )}
      </div>
      <Settings />
      <History />
      <RenderViewer />
      {!collapsed && (
        <button
          className="dock-toggle"
          style={{ position: "absolute", right: 12, bottom: 276, zIndex: 4 }}
          onClick={() => setCollapsed(true)}
        >
          replier
        </button>
      )}
    </div>
  );
}
