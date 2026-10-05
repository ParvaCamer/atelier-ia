/**
 * Visionneuse : le rendu de chaque projet, en grand.
 *
 * Deux sources par projet : son aperçu en direct (l'adresse de son site,
 * chargée telle quelle) et les images que ses tâches ont produites
 * (slides, captures de l'app). Rien n'est lu sur le disque ici : les
 * images passent par le moteur.
 */
import { useEffect, useMemo, useState } from "react";
import type { Project } from "../../ipc";
import { groupRenders, renderUrl, useRenders, type RenderGroup } from "../../state/renders";
import { useWorld } from "../../state/store";

function when(iso: string): string {
  return new Date(iso).toLocaleString("fr-FR", { dateStyle: "medium", timeStyle: "short" });
}

export function RenderViewer() {
  const open = useRenders((s) => s.open);
  const renders = useRenders((s) => s.renders);
  const { projectId, tab, taskId, index } = useRenders();
  const { close, select } = useRenders.getState();
  const projects = useWorld((s) => s.projects);

  const groups = useMemo(() => groupRenders(renders), [renders]);
  // Seuls les projets qui ont quelque chose à montrer.
  const shown = useMemo(
    () => projects.filter((p) => p.previewUrl || groups.some((g) => g.projectId === p.id)),
    [projects, groups],
  );
  const project = shown.find((p) => p.id === projectId) ?? shown[0];
  const mine = groups.filter((g) => g.projectId === project?.id);
  const activeTab = project?.previewUrl && (tab === "site" || !mine.length) ? "site" : "renders";
  const group = mine.find((g) => g.taskId === taskId) ?? mine[0];

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (["INPUT", "TEXTAREA", "SELECT"].includes(tag)) return;
      if (e.key === "Escape") close();
      if (activeTab !== "renders" || !group) return;
      const n = group.items.length;
      if (e.key === "ArrowRight") select({ index: (index + 1) % n });
      if (e.key === "ArrowLeft") select({ index: (index - 1 + n) % n });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, close, select, activeTab, group, index]);

  if (!open) return null;

  return (
    <div className="settings renders-viewer" role="dialog" aria-label="Rendus des projets">
      <nav className="settings-nav">
        <div className="settings-title">Rendus</div>
        {shown.length === 0 && <div className="field-hint" style={{ padding: "0 10px" }}>Aucun rendu pour l'instant.</div>}
        {shown.map((p) => {
          const count = groups.filter((g) => g.projectId === p.id).reduce((n, g) => n + g.items.length, 0);
          return (
            <button
              key={p.id} className="settings-nav-item" data-active={p.id === project?.id}
              onClick={() => select({ projectId: p.id, taskId: null, index: 0, tab: p.previewUrl ? "site" : "renders" })}
            >
              <span><span className="dot" style={{ background: p.color, display: "inline-block", marginRight: 7 }} />{p.name}</span>
              <small>{[p.previewUrl && "site en direct", count && `${count} image${count > 1 ? "s" : ""}`].filter(Boolean).join(" · ")}</small>
            </button>
          );
        })}
      </nav>

      <div className="renders-main">
        <div className="renders-head">
          <h1>{project?.name ?? "Rendus"}</h1>
          {project?.previewUrl && mine.length > 0 && (
            <div className="view-switch renders-tabs" role="group" aria-label="Source du rendu">
              <button data-active={activeTab === "site"} onClick={() => select({ tab: "site" })}>Site en direct</button>
              <button data-active={activeTab === "renders"} onClick={() => select({ tab: "renders" })}>Images produites</button>
            </div>
          )}
          <div className="header-spacer" />
          <button className="btn ghost" onClick={close}>Fermer</button>
        </div>
        {!project && (
          <div className="renders-empty">
            Indique l'adresse du site d'un projet dans Réglages › Projets (« Aperçu en direct »), ou lance une
            tâche qui produit des images : slides, maquettes, captures de l'app.
          </div>
        )}
        {project && activeTab === "site" && <LiveSite project={project} />}
        {project && activeTab === "renders" && group && (
          <Carousel group={group} groups={mine} index={Math.min(index, group.items.length - 1)} />
        )}
      </div>
    </div>
  );
}

function LiveSite({ project }: { project: Project }) {
  const [reload, setReload] = useState(0);
  const url = project.previewUrl!;
  return (
    <div className="renders-site">
      <div className="renders-address">
        <span className="badge" style={{ background: "rgba(74, 222, 128, 0.16)", color: "var(--ok)" }}>en direct</span>
        <code title={url}>{url.startsWith("data:") ? "aperçu simulé (harnais)" : url}</code>
        <button className="btn small" onClick={() => setReload((n) => n + 1)}>Recharger</button>
      </div>
      {/* Bac à sable : le site s'exécute, mais n'atteint ni l'application
          ni ses commandes. Certains sites en ligne refusent d'être intégrés
          (en-tête X-Frame-Options) : la zone reste alors vide. */}
      <iframe
        key={reload}
        className="renders-frame"
        src={url}
        title={`Aperçu de ${project.name}`}
        sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
      />
    </div>
  );
}

function Carousel({ group, groups, index }: { group: RenderGroup; groups: RenderGroup[]; index: number }) {
  const { select } = useRenders.getState();
  const item = group.items[index];
  const [src, setSrc] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const n = group.items.length;

  useEffect(() => {
    let alive = true;
    setSrc(null);
    setError(null);
    renderUrl(item.id).then((u) => alive && setSrc(u)).catch((e) => alive && setError(String(e)));
    return () => { alive = false; };
  }, [item.id]);

  return (
    <div className="renders-carousel">
      <div className="renders-bar">
        <select
          className="input compact" value={group.taskId} aria-label="Série de rendus"
          onChange={(e) => select({ taskId: e.target.value, index: 0 })}
        >
          {groups.map((g) => (
            <option key={g.taskId} value={g.taskId}>{g.title} — {when(g.createdAt)} ({g.items.length})</option>
          ))}
        </select>
        <code className="renders-path" title={item.path}>{item.path}</code>
        <span className="renders-count">{index + 1} / {n}</span>
      </div>
      <div className="renders-stage">
        <button className="renders-nav" disabled={n < 2} onClick={() => select({ index: (index - 1 + n) % n })} aria-label="Image précédente">‹</button>
        <div className="renders-image">
          {error ? <div className="feedback" data-kind="error">{error}</div>
            : src ? <img src={src} alt={`${group.title} — ${index + 1} sur ${n}`} />
            : <div className="field-hint">Chargement…</div>}
        </div>
        <button className="renders-nav" disabled={n < 2} onClick={() => select({ index: (index + 1) % n })} aria-label="Image suivante">›</button>
      </div>
      {n > 1 && (
        <div className="renders-thumbs">
          {group.items.map((r, i) => <Thumb key={r.id} id={r.id} active={i === index} label={`${i + 1}`} onClick={() => select({ index: i })} />)}
        </div>
      )}
    </div>
  );
}

function Thumb({ id, active, label, onClick }: { id: string; active: boolean; label: string; onClick: () => void }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    renderUrl(id).then((u) => alive && setSrc(u)).catch(() => {});
    return () => { alive = false; };
  }, [id]);
  return (
    <button className="renders-thumb" data-active={active} onClick={onClick} aria-label={`Image ${label}`}>
      {src ? <img src={src} alt="" /> : <span>{label}</span>}
    </button>
  );
}
