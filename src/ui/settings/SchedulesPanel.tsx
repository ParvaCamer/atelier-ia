/**
 * Planifications. Le moteur valide l'expression et calcule les échéances ;
 * l'éditeur ne fait que proposer des formes simples au lieu de cron brut.
 */
import { useEffect, useState } from "react";
import type { FileWatch, ProjectId, Schedule, ScheduleId, ScheduleTarget, WatchId, WorkflowId } from "../../ipc";
import { api } from "../../ipc";
import { useConfig } from "../../state/config";
import { useHistory } from "../../state/history";
import { useWorld } from "../../state/store";
import { same, useUnsavedFlag } from "../../state/unsaved";
import { Area, DangerButton, Feedback, Field, Select, Text, Toggle, useJob } from "./fields";

type Freq =
  | { mode: "daily"; hour: number; minute: number }
  | { mode: "weekly"; days: number[]; hour: number; minute: number }
  | { mode: "hourly"; every: number; minute: number }
  | { mode: "cron"; expr: string };

const DAYS: [number, string][] = [[1, "lun"], [2, "mar"], [3, "mer"], [4, "jeu"], [5, "ven"], [6, "sam"], [0, "dim"]];

export function toCron(f: Freq): string {
  switch (f.mode) {
    case "daily": return `${f.minute} ${f.hour} * * *`;
    case "weekly": return `${f.minute} ${f.hour} * * ${[...f.days].sort().join(",") || "1"}`;
    case "hourly": return f.every <= 1 ? `${f.minute} * * * *` : `${f.minute} */${f.every} * * *`;
    case "cron": return f.expr;
  }
}

export function fromCron(expr: string): Freq {
  const e = expr.trim().replace(/\s+/g, " ");
  let m = e.match(/^(\d{1,2}) (\d{1,2}) \* \* \*$/);
  if (m) return { mode: "daily", minute: +m[1], hour: +m[2] };
  m = e.match(/^(\d{1,2}) (\d{1,2}) \* \* ([0-6](?:,[0-6])*)$/);
  if (m) return { mode: "weekly", minute: +m[1], hour: +m[2], days: m[3].split(",").map(Number) };
  m = e.match(/^(\d{1,2}) \* \* \* \*$/);
  if (m) return { mode: "hourly", every: 1, minute: +m[1] };
  m = e.match(/^(\d{1,2}) \*\/(\d{1,2}) \* \* \*$/);
  if (m) return { mode: "hourly", every: +m[2], minute: +m[1] };
  return { mode: "cron", expr: e };
}

const blank = (target: ScheduleTarget): Schedule => ({
  id: "" as ScheduleId, name: "", target, cron: "0 9 * * 1", enabled: true, runMissed: true,
  lastRunAt: null, lastRunId: null, lastOutcome: null, lastError: null, nextRunAt: null, createdAt: new Date().toISOString(),
});

const blankWatch = (workflowId: WorkflowId): FileWatch => ({
  id: "" as WatchId, name: "", workflowId, patterns: ["src/**/*"], debounceSecs: 5, enabled: true,
  lastRunAt: null, lastRunId: null, lastOutcome: null, lastError: null, lastTrigger: null, createdAt: new Date().toISOString(),
});

const when = (iso: string) => new Date(iso).toLocaleString("fr-FR", { weekday: "long", day: "numeric", month: "long", hour: "2-digit", minute: "2-digit" });

export function SchedulesPanel() {
  const focusId = useConfig((s) => s.focusId);
  const workflows = useWorld((s) => s.workflows);
  const projects = useWorld((s) => s.projects);
  const [items, setItems] = useState<Schedule[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<Schedule | null>(null);
  const [watches, setWatches] = useState<FileWatch[]>([]);
  const [watchDraft, setWatchDraft] = useState<FileWatch | null>(null);
  const [loaded, setLoaded] = useState(false);
  const job = useJob();

  const load = async () => {
    const [list, ws] = await Promise.all([job.run(() => api.listSchedules()), job.run(() => api.listWatches())]);
    if (list) setItems(list);
    if (ws) setWatches(ws);
    setLoaded(true);
    return list ?? [];
  };

  useEffect(() => {
    void (async () => {
      const list = await load();
      // Arrivée depuis un workflow : « new:workflow:<id> ».
      if (focusId?.startsWith("new:workflow:")) {
        const workflowId = focusId.slice("new:workflow:".length) as WorkflowId;
        const wf = workflows.find((w) => w.id === workflowId);
        setSelected("new");
        setDraft({ ...blank({ kind: "workflow", workflowId }), name: wf ? `${wf.name} — planifié` : "" });
      } else if (list[0]) {
        setSelected(list[0].id);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (selected === "new" || selected === "watch:new") return;
    const found = items.find((s) => s.id === selected);
    setDraft(found ? { ...found } : null);
    const fw = watches.find((w) => `watch:${w.id}` === selected);
    setWatchDraft(fw ? { ...fw } : null);
  }, [selected, items, watches]);

  const describe = (s: Schedule) =>
    s.target.kind === "workflow"
      ? workflows.find((w) => w.id === (s.target as { workflowId: string }).workflowId)?.name ?? "workflow supprimé"
      : `« ${(s.target as { text: string }).text} »`;

  return (
    <div className="split">
      <aside className="split-list">
        <div className="warning">Les planifications ne tournent que lorsqu'Atelier est ouvert.</div>
        <button
          className="list-new"
          onClick={() => {
            setSelected("new");
            setDraft(blank(workflows[0] ? { kind: "workflow", workflowId: workflows[0].id } : { kind: "request", text: "", projectId: null }));
            job.setError(null); job.setOk(null);
          }}
        >
          + Nouvelle planification
        </button>
        {items.map((s) => (
          <button key={s.id} className="list-item column" data-active={selected === s.id} data-muted={!s.enabled} onClick={() => setSelected(s.id)}>
            <span className="list-item-main">{s.name}</span>
            <small>{s.enabled && s.nextRunAt ? `prochaine : ${when(s.nextRunAt)}` : "désactivée"}</small>
            <small className="list-item-sub">{describe(s)}</small>
          </button>
        ))}

        <div className="list-group">Surveillances de fichiers</div>
        <button
          className="list-new" disabled={!workflows.length}
          onClick={() => {
            setSelected("watch:new");
            setDraft(null);
            setWatchDraft(blankWatch(workflows[0].id));
            job.setError(null); job.setOk(null);
          }}
        >
          + Nouvelle surveillance
        </button>
        {watches.map((w) => (
          <button key={w.id} className="list-item column" data-active={selected === `watch:${w.id}`} data-muted={!w.enabled} onClick={() => setSelected(`watch:${w.id}`)}>
            <span className="list-item-main">{w.name}</span>
            <small>{w.enabled ? w.patterns.join(", ") : "désactivée"}</small>
            <small className="list-item-sub">{workflows.find((x) => x.id === w.workflowId)?.name ?? "workflow supprimé"}</small>
          </button>
        ))}
      </aside>

      {watchDraft && selected?.startsWith("watch:") ? (
        <WatchEditor
          key={watchDraft.id || "new"}
          draft={watchDraft}
          original={watches.find((w) => w.id === watchDraft.id)}
          onChange={setWatchDraft}
          onSaved={async (saved) => { await load(); setSelected(`watch:${saved.id}`); }}
          onDeleted={async () => { await load(); setSelected(null); setWatchDraft(null); }}
        />
      ) : draft ? (
        <ScheduleEditor
          key={draft.id || "new"}
          draft={draft}
          original={items.find((x) => x.id === draft.id)}
          onChange={setDraft}
          projects={projects}
          onSaved={async (saved) => { await load(); setSelected(saved.id); }}
          onDeleted={async () => { await load(); setSelected(null); }}
        />
      ) : (
        <section className="split-detail empty">
          {!loaded ? "Chargement…" : items.length || watches.length
            ? "Sélectionne une planification ou une surveillance."
            : "Aucune planification. Exemple : « tous les lundis à 9 h, lancer Release Spotly »."}
          <Feedback error={job.error} ok={null} />
        </section>
      )}
    </div>
  );
}

function ScheduleEditor({ draft, original, onChange, projects, onSaved, onDeleted }: {
  draft: Schedule;
  /** Version enregistrée, pour savoir s'il reste des modifications. */
  original?: Schedule;
  onChange: (s: Schedule) => void;
  projects: { id: ProjectId; name: string }[];
  onSaved: (s: Schedule) => void;
  onDeleted: () => void;
}) {
  const workflows = useWorld((s) => s.workflows);
  const allProjects = useWorld((s) => s.projects);
  const [freq, setFreq] = useState<Freq>(() => fromCron(draft.cron));
  useUnsavedFlag("schedule", original ? !same(draft, original) : !!draft.name.trim());
  const [preview, setPreview] = useState<string[]>([]);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const job = useJob();

  const patch = (p: Partial<Schedule>) => onChange({ ...draft, ...p });
  const cron = toCron(freq);

  useEffect(() => {
    if (cron !== draft.cron) patch({ cron });
    const h = setTimeout(async () => {
      try {
        setPreview(await api.previewSchedule(cron));
        setPreviewError(null);
      } catch (e) {
        setPreview([]);
        setPreviewError(String(e));
      }
    }, 250);
    return () => clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cron]);

  const isRequest = draft.target.kind === "request";
  const target = draft.target as ScheduleTarget & { workflowId?: WorkflowId; text?: string; projectId?: ProjectId | null };
  const pad = (n: number) => String(n).padStart(2, "0");
  const time = "hour" in freq ? `${pad(freq.hour)}:${pad(freq.minute)}` : "09:00";
  const setTime = (v: string) => {
    const [h, m] = v.split(":").map(Number);
    if (freq.mode === "daily" || freq.mode === "weekly") setFreq({ ...freq, hour: h || 0, minute: m || 0 });
  };

  return (
    <section className="split-detail">
      <h2>{draft.id ? draft.name : "Nouvelle planification"}</h2>
      <div className="form">
        <Field label="Nom" wide><Text value={draft.name} onChange={(name) => patch({ name })} placeholder="Audit SEO hebdomadaire" /></Field>

        <Field label="Ce qui est lancé" wide>
          <div className="segmented">
            <button data-active={!isRequest} disabled={!workflows.length} onClick={() => patch({ target: { kind: "workflow", workflowId: workflows[0]?.id } as ScheduleTarget })}>Un workflow</button>
            <button data-active={isRequest} onClick={() => patch({ target: { kind: "request", text: "", projectId: null } })}>Une demande</button>
          </div>
        </Field>

        {isRequest ? (
          <>
            <Field label="Demande" wide hint="Planifiée à nouveau par l'orchestrateur à chaque échéance : consomme du quota à chaque exécution. Un workflow enregistré, lui, ne coûte rien à lancer.">
              <Area rows={2} value={target.text ?? ""} onChange={(text) => patch({ target: { kind: "request", text, projectId: target.projectId ?? null } })} placeholder="Analyse les performances SEO du site et résume les évolutions" />
            </Field>
            <Field label="Projet">
              <Select
                value={(target.projectId as string) ?? ""}
                options={[{ value: "", label: "L'orchestrateur choisit" }, ...projects.map((p) => ({ value: p.id as string, label: p.name }))]}
                onChange={(v) => patch({ target: { kind: "request", text: target.text ?? "", projectId: (v || null) as ProjectId | null } })}
              />
            </Field>
          </>
        ) : (
          <Field label="Workflow" wide>
            <Select
              value={(target.workflowId as string) ?? ""}
              options={workflows.map((w) => ({ value: w.id as string, label: `${allProjects.find((p) => p.id === w.projectId)?.name ?? "?"} › ${w.name}` }))}
              onChange={(v) => patch({ target: { kind: "workflow", workflowId: v as WorkflowId } })}
            />
          </Field>
        )}

        <Field label="Fréquence" wide>
          <div className="freq">
            <Select
              value={freq.mode}
              options={[
                { value: "daily", label: "Chaque jour" },
                { value: "weekly", label: "Certains jours" },
                { value: "hourly", label: "Toutes les N heures" },
                { value: "cron", label: "Expression cron" },
              ]}
              onChange={(mode) => {
                if (mode === "daily") setFreq({ mode, hour: 9, minute: 0 });
                if (mode === "weekly") setFreq({ mode, days: [1], hour: 9, minute: 0 });
                if (mode === "hourly") setFreq({ mode, every: 2, minute: 0 });
                if (mode === "cron") setFreq({ mode, expr: cron });
              }}
            />
            {(freq.mode === "daily" || freq.mode === "weekly") && (
              <input className="input time" type="time" value={time} onChange={(e) => setTime(e.target.value)} />
            )}
            {freq.mode === "hourly" && (
              <>
                <span>toutes les</span>
                <input className="input num" type="number" min={1} max={23} value={freq.every} onChange={(e) => setFreq({ ...freq, every: Math.max(1, +e.target.value) })} />
                <span>h, à la minute</span>
                <input className="input num" type="number" min={0} max={59} value={freq.minute} onChange={(e) => setFreq({ ...freq, minute: Math.min(59, Math.max(0, +e.target.value)) })} />
              </>
            )}
            {freq.mode === "cron" && (
              <Text mono value={freq.expr} onChange={(expr) => setFreq({ mode: "cron", expr })} placeholder="0 9 * * 1" />
            )}
          </div>
          {freq.mode === "weekly" && (
            <div className="days">
              {DAYS.map(([d, label]) => (
                <button key={d} type="button" data-active={freq.days.includes(d)}
                  onClick={() => setFreq({ ...freq, days: freq.days.includes(d) ? freq.days.filter((x) => x !== d) : [...freq.days, d] })}>
                  {label}
                </button>
              ))}
            </div>
          )}
          <div className="preview">
            <code>{cron}</code>
            {previewError ? <span className="feedback" data-kind="error">{previewError}</span>
              : preview.length > 0 && <span>prochaines : {preview.map(when).join(" · ")}</span>}
          </div>
        </Field>

        <Field label="État">
          <Toggle checked={draft.enabled} onChange={(enabled) => patch({ enabled })} label={draft.enabled ? "Active" : "Désactivée"} />
        </Field>
        <Field label="Si Atelier était fermé à l'échéance" hint="Rattrapée une seule fois, jamais autant de fois qu'elle a été manquée.">
          <Toggle checked={draft.runMissed} onChange={(runMissed) => patch({ runMissed })} label={draft.runMissed ? "L'exécuter au prochain lancement" : "L'ignorer"} />
        </Field>
      </div>

      {draft.lastRunAt && (
        <div className="last-run" data-outcome={draft.lastOutcome ?? ""}>
          Dernière échéance : {when(draft.lastRunAt)} —{" "}
          {draft.lastOutcome === "launched" ? "lancée" : draft.lastOutcome === "skipped" ? "ignorée" : "erreur"}
          {draft.lastError && ` : ${draft.lastError}`}
          {draft.lastRunId && (
            <button className="link small inline" onClick={() => useHistory.getState().show(draft.lastRunId)}>voir l'exécution</button>
          )}
        </div>
      )}

      <div className="actions">
        <button className="btn primary" disabled={job.busy || !!previewError} onClick={async () => {
          const saved = await job.run(() => api.saveSchedule({ ...draft, cron }), draft.id ? "Planification enregistrée." : "Planification créée.");
          if (saved) { await useConfig.getState().afterSave(); onSaved(saved); }
        }}>{draft.id ? "Enregistrer" : "Créer"}</button>
        {draft.id && (
          <button className="btn" disabled={job.busy} onClick={async () => {
            const run = await job.run(() => api.runScheduleNow(draft.id), "Lancée — l'échéance suivante reste inchangée.");
            if (run) onSaved(draft);
          }}>Exécuter maintenant</button>
        )}
        {draft.id && (
          <DangerButton label="Supprimer" confirmLabel="Confirmer" onConfirm={async () => {
            const done = await job.run(() => api.deleteSchedule(draft.id));
            if (done !== undefined) onDeleted();
          }} />
        )}
        <Feedback error={job.error} ok={job.ok} />
      </div>
    </section>
  );
}

const OUTCOME: Record<string, string> = { launched: "lancée", skipped: "ignorée", error: "en erreur" };

function WatchEditor({ draft, original, onChange, onSaved, onDeleted }: {
  draft: FileWatch;
  original?: FileWatch;
  onChange: (w: FileWatch) => void;
  onSaved: (w: FileWatch) => void;
  onDeleted: () => void;
}) {
  const workflows = useWorld((s) => s.workflows);
  const projects = useWorld((s) => s.projects);
  // Une ligne par motif : un motif peut contenir une virgule (`{a,b}` n'est pas
  // pris en charge, mais un nom de fichier peut en avoir une).
  const [patterns, setPatterns] = useState(draft.patterns.join("\n"));
  const job = useJob();
  useUnsavedFlag("watch", (original ? !same(draft, original) : !!draft.name.trim()) || patterns !== (original?.patterns ?? draft.patterns).join("\n"));
  const patch = (p: Partial<FileWatch>) => onChange({ ...draft, ...p });
  const wf = workflows.find((w) => w.id === draft.workflowId);
  const project = projects.find((p) => p.id === wf?.projectId);

  const save = async () => {
    const watch = { ...draft, patterns: patterns.split("\n") };
    const saved = await job.run(() => api.saveWatch(watch), draft.id ? "Surveillance enregistrée." : "Surveillance créée : le premier passage relève l'état actuel, sans rien lancer.");
    if (saved) { setPatterns(saved.patterns.join("\n")); onSaved(saved); }
  };

  const remove = async () => {
    const done = await job.run(() => api.deleteWatch(draft.id));
    if (done !== undefined) onDeleted();
  };

  return (
    <section className="split-detail">
      <h2>{draft.id ? draft.name : "Nouvelle surveillance"}</h2>
      <p className="note">
        Lance un workflow quand des fichiers du dossier du projet changent. Une rafale d'écritures ne produit qu'un
        lancement ; si l'exécution précédente tourne encore, le passage est ignoré. Ne tourne que lorsqu'Atelier est ouvert.
      </p>
      <div className="form">
        <Field label="Nom"><Text value={draft.name} onChange={(name) => patch({ name })} placeholder="Tests à chaque modification" /></Field>
        <Field label="Workflow" hint={project ? (project.rootPath ? `Dossier surveillé : ${project.rootPath}` : `Le projet ${project.name} n'a pas de dossier : la surveillance sera refusée.`) : undefined}>
          <Select value={draft.workflowId} options={workflows.map((w) => ({ value: w.id, label: w.name }))} onChange={(workflowId) => patch({ workflowId })} />
        </Field>
        <Field label="Motifs de fichiers" wide hint="Un par ligne, relatifs au dossier du projet. « * » dans un dossier, « ** » à toute profondeur ; sans « / », vaut pour un nom de fichier partout. node_modules, build, .git… sont ignorés.">
          <Area mono rows={3} value={patterns} onChange={setPatterns} placeholder={"src/**/*.kt\n*.md"} />
        </Field>
        <Field label="Anti-rebond (secondes)" hint="Délai sans nouvelle écriture avant de lancer.">
          <input className="input" type="number" min={1} max={3600} value={draft.debounceSecs} onChange={(e) => patch({ debounceSecs: Number(e.target.value) })} />
        </Field>
        <Field label="État">
          <Toggle checked={draft.enabled} onChange={(enabled) => patch({ enabled })} label={draft.enabled ? "Active" : "Désactivée"} />
        </Field>
      </div>

      {draft.lastRunAt && (
        <div className="note">
          Dernier passage : {when(draft.lastRunAt)} — {OUTCOME[draft.lastOutcome ?? ""] ?? "—"}
          {draft.lastTrigger && <> · déclenché par <code>{draft.lastTrigger}</code></>}
          {draft.lastError && <> · {draft.lastError}</>}
          {draft.lastRunId && <> · <button className="link small inline" onClick={() => useHistory.getState().show(draft.lastRunId)}>voir l'exécution</button></>}
        </div>
      )}

      <div className="actions">
        <button className="btn primary" disabled={job.busy} onClick={save}>{draft.id ? "Enregistrer" : "Créer la surveillance"}</button>
        {draft.id && <DangerButton label="Supprimer" confirmLabel="Confirmer" onConfirm={remove} disabled={job.busy} />}
        <Feedback error={job.error} ok={job.ok} />
      </div>
    </section>
  );
}
