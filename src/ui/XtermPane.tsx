/**
 * Terminal interactif réel : xterm.js branché sur un PTY natif.
 *
 * Reste monté quand on change d'onglet (simplement masqué) : la session
 * shell survit, comme dans n'importe quel émulateur de terminal.
 */
import { useEffect, useRef } from "react";
import { Terminal as Xterm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";
import { api } from "../ipc";
import { isTauri } from "../ipc/devMock";
import { useWorld } from "../state/store";

export function XtermPane({ visible }: { visible: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (!isTauri() || !host.current) return;
    const el = host.current;

    const term = new Xterm({
      fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
      fontSize: 12,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback: 5000,
      theme: {
        background: "#0f151e",
        foreground: "#dbe6f5",
        cursor: "#5eead4",
        selectionBackground: "#2b3a4f",
      },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    // Rendu WebGL si disponible ; sinon xterm retombe sur le rendu DOM.
    try {
      const gl = new WebglAddon();
      gl.onContextLoss(() => gl.dispose());
      term.loadAddon(gl);
    } catch {
      /* rendu DOM */
    }
    fit.fit();
    fitRef.current = fit;

    let id: string | null = null;
    let disposed = false;
    const s = useWorld.getState();
    const projectId = s.selectedAgent ? s.agentsById.get(s.selectedAgent)?.projectId ?? null : null;

    api
      .ptyOpen(term.cols, term.rows, projectId, (ev) => {
        if (ev.kind === "output") term.write(ev.data);
        else term.write("\r\n\x1b[2m[session terminée]\x1b[0m\r\n");
      })
      .then((opened) => {
        // Démonté entre-temps (double montage du mode strict) : on referme.
        if (disposed) void api.ptyClose(opened);
        else id = opened;
      })
      .catch((e) => term.write(`\r\n\x1b[31mouverture impossible : ${e}\x1b[0m\r\n`));

    const input = term.onData((data) => { if (id) void api.ptyWrite(id, data); });

    const observer = new ResizeObserver(() => {
      // Masqué (display: none) : dimensions nulles, on n'envoie rien.
      if (!el.offsetParent) return;
      fit.fit();
      if (id) void api.ptyResize(id, term.cols, term.rows);
    });
    observer.observe(el);

    return () => {
      disposed = true;
      observer.disconnect();
      input.dispose();
      if (id) void api.ptyClose(id);
      term.dispose();
    };
  }, []);

  useEffect(() => {
    if (visible) requestAnimationFrame(() => fitRef.current?.fit());
  }, [visible]);

  if (!isTauri()) {
    return (
      <div className="empty" hidden={!visible}>
        Le terminal interactif n'existe que dans l'application desktop.
      </div>
    );
  }
  return <div className="xterm-host" ref={host} hidden={!visible} />;
}
