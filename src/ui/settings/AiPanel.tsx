import { useState } from "react";
import type { HealthState, ModelRoute, ProviderConfig } from "../../ipc";
import { api } from "../../ipc";
import { useConfig } from "../../state/config";
import { useWorld } from "../../state/store";
import { DangerButton, Feedback, Select, Text, Toggle, useJob } from "./fields";

/** Alias appelés directement par le moteur (miroir de config.rs, pour l'affichage seulement). */
const RESERVED: Record<string, string> = {
  "reasoning.high": "planification de l'orchestrateur",
  "reasoning.default": "escalade de l'aiguillage",
  "classify.fast": "aiguillage des demandes",
};

const STATE_LABEL: Record<HealthState, string> = { ok: "opérationnel", degraded: "à vérifier", unavailable: "indisponible" };

export function AiPanel() {
  const providers = useConfig((s) => s.providers);
  const routes = useConfig((s) => s.routes);
  const health = useConfig((s) => s.health);
  const healthLoading = useConfig((s) => s.healthLoading);
  const settings = useConfig((s) => s.settings);
  const refreshHealth = useConfig((s) => s.refreshHealth);
  const afterSave = useConfig((s) => s.afterSave);
  const [adding, setAdding] = useState(false);
  const job = useJob();

  const ollamaModels = health.find((h) => h.providerId === "ollama")?.models ?? [];
  const openaiModels = health.find((h) => providers.find((p) => p.id === h.providerId)?.kind === "openai")?.models ?? [];

  return (
    <div className="ai">
      <div className="ai-head">
        <h2>Fournisseurs</h2>
        <button className="btn ghost small" disabled={healthLoading} onClick={() => void refreshHealth()}>
          {healthLoading ? "vérification…" : "Actualiser l'état"}
        </button>
      </div>

      <div className="providers">
        {providers.map((p) => <ProviderCard key={p.id} provider={p} />)}
      </div>

      <Toggle
        checked={settings.startOllamaWithApp}
        label="Démarrer Ollama avec Atelier s'il est éteint"
        onChange={async (startOllamaWithApp) => {
          const saved = await job.run(() => api.saveSettings({ ...settings, startOllamaWithApp }));
          if (saved) await afterSave();
        }}
      />
      <p className="note">
        Ollama n'est pas lancé à l'ouverture de ta session macOS. Sans lui, l'aiguillage se replie
        sans le dire sur Claude Code, donc sur ton quota.
      </p>
      <Feedback error={job.error} ok={null} />

      <div className="ai-head">
        <h2>Modèles</h2>
        <button className="btn ghost small" onClick={() => setAdding(true)} disabled={adding}>+ Nouvel alias</button>
      </div>
      <p className="note">
        Les agents ne désignent jamais un modèle : ils utilisent un alias. Changer le fournisseur d'un alias
        change le modèle de tous les agents qui l'utilisent, sans toucher à leur définition.
      </p>

      <datalist id="models-ollama">{ollamaModels.map((m) => <option key={m} value={m.replace(/:latest$/, "")} />)}</datalist>
      <datalist id="models-openai">{openaiModels.map((m) => <option key={m} value={m} />)}</datalist>
      <datalist id="models-claude">{["sonnet", "opus", "haiku"].map((m) => <option key={m} value={m} />)}</datalist>

      <div className="routes">
        <div className="route route-labels">
          <span>Alias</span><span>Fournisseur</span><span>Modèle</span><span>Si indisponible</span><span>Tokens max</span><span />
        </div>
        {routes.map((r) => <RouteRow key={r.modelRef} route={r} />)}
        {adding && (
          <RouteRow
            route={{ modelRef: "", providerId: "ollama", model: "llama3.2", maxTokens: 2048, temperature: 0.2, fallbackRef: null }}
            isNew
            onDone={() => setAdding(false)}
          />
        )}
      </div>
    </div>
  );
}

function ProviderCard({ provider }: { provider: ProviderConfig }) {
  const h = useConfig((s) => s.health.find((x) => x.providerId === provider.id));
  const refreshHealth = useConfig((s) => s.refreshHealth);
  const afterSave = useConfig((s) => s.afterSave);
  const [url, setUrl] = useState(provider.baseUrl ?? "");
  const [key, setKey] = useState("");
  const job = useJob();
  const isOllama = provider.kind === "ollama";
  const isOpenAi = provider.kind === "openai";

  const saveKey = async (value: string | null) => {
    const saved = await job.run(() => api.saveProviderKey(provider.id, value), value ? "Clé enregistrée." : "Clé effacée.");
    if (saved) { setKey(""); await afterSave(); }
  };

  const save = async (p: ProviderConfig) => {
    const saved = await job.run(() => api.saveProvider(p), "Enregistré.");
    if (saved) { await afterSave(); }
  };

  return (
    <div className="provider" data-state={h?.state ?? "unknown"}>
      <div className="provider-head">
        <span className="provider-dot" />
        <strong>{provider.label}</strong>
        <span className="provider-state">{h ? STATE_LABEL[h.state] : "…"}</span>
      </div>
      <div className="provider-detail">{h?.detail ?? "vérification en cours"}</div>
      {provider.kind === "claude-code" && (
        <div className="provider-note">Usage personnel uniquement. Aucun outil : Claude Code décide, Atelier exécute.</div>
      )}
      {isOpenAi && (
        <div className="provider-note">
          Facturé à l'usage par OpenAI, en plus de tout abonnement. La clé reste sur cette machine et n'est jamais réaffichée.
        </div>
      )}
      {isOllama && h && h.models.length > 0 && (
        <div className="chips">{h.models.map((m) => <code key={m}>{m}</code>)}</div>
      )}
      <div className="provider-actions">
        <Toggle checked={provider.enabled} onChange={(enabled) => save({ ...provider, enabled })} label="Activé" />
        {(isOllama || isOpenAi) && (
          <>
            <Text mono value={url} onChange={setUrl} placeholder={isOllama ? "http://127.0.0.1:11434" : "https://api.openai.com/v1"} />
            {url !== (provider.baseUrl ?? "") && <button className="btn small" onClick={() => save({ ...provider, baseUrl: url })}>Enregistrer</button>}
            {isOllama && h?.state === "unavailable" && provider.enabled && (
              <button
                className="btn small"
                disabled={job.busy}
                onClick={async () => { await job.run(() => api.startOllama(), "Ollama démarré."); await refreshHealth(); }}
              >
                {job.busy ? "démarrage…" : "Démarrer Ollama"}
              </button>
            )}
          </>
        )}
      </div>
      {isOpenAi && (
        <div className="provider-actions">
          <input
            className="input" data-mono type="password" autoComplete="off" value={key}
            aria-label="Clé d'API OpenAI"
            placeholder={provider.hasKey ? "clé enregistrée — saisir pour la remplacer" : "sk-…"}
            onChange={(e) => setKey(e.target.value)}
          />
          <button className="btn small" disabled={!key.trim() || job.busy} onClick={() => void saveKey(key)}>Enregistrer la clé</button>
          {provider.hasKey && <DangerButton label="Effacer la clé" confirmLabel="Effacer" onConfirm={() => void saveKey(null)} disabled={job.busy} />}
        </div>
      )}
      <FeedbackLine error={job.error} ok={job.ok} />
    </div>
  );
}

function RouteRow({ route, isNew, onDone }: { route: ModelRoute; isNew?: boolean; onDone?: () => void }) {
  const providers = useConfig((s) => s.providers);
  const routes = useConfig((s) => s.routes);
  const afterSave = useConfig((s) => s.afterSave);
  const agents = useWorld((s) => s.agents);
  const [draft, setDraft] = useState<ModelRoute>(route);
  const job = useJob();

  const provider = providers.find((p) => p.id === draft.providerId);
  const dirty = isNew || JSON.stringify(draft) !== JSON.stringify(route);
  const users = agents.filter((a) => a.modelRef === route.modelRef).length;
  const usage = [RESERVED[route.modelRef], users ? `${users} agent${users > 1 ? "s" : ""}` : null].filter(Boolean).join(" · ");
  const patch = (p: Partial<ModelRoute>) => setDraft((d) => ({ ...d, ...p }));

  return (
    <div className="route-wrap">
      <div className="route">
        <div className="route-alias">
          {isNew ? <Text mono value={draft.modelRef} onChange={(modelRef) => patch({ modelRef })} placeholder="summary.long" /> : <code>{route.modelRef}</code>}
          {usage && <small>{usage}</small>}
        </div>
        <Select value={draft.providerId} options={providers.map((p) => ({ value: p.id, label: p.label }))} onChange={(providerId) => patch({ providerId })} />
        <input
          className="input" data-mono list={`models-${provider?.kind === "claude-code" ? "claude" : provider?.kind}`}
          value={draft.model} placeholder={provider?.kind === "ollama" ? "llama3.2" : provider?.kind === "openai" ? "gpt-4o-mini" : "défaut du compte"}
          onChange={(e) => patch({ model: e.target.value })}
        />
        <Select
          value={draft.fallbackRef ?? ""}
          options={[{ value: "", label: "— échouer —" }, ...routes.filter((r) => r.modelRef !== draft.modelRef).map((r) => ({ value: r.modelRef, label: r.modelRef }))]}
          onChange={(v) => patch({ fallbackRef: v || null })}
        />
        <input className="input" type="number" min={1} max={128000} value={draft.maxTokens} onChange={(e) => patch({ maxTokens: Number(e.target.value) })} />
        <div className="route-actions">
          {dirty && (
            <button className="btn primary small" disabled={job.busy} onClick={async () => {
              const saved = await job.run(() => api.saveRoute(draft), "Enregistré.");
              if (saved) { await afterSave(); onDone?.(); }
            }}>Enregistrer</button>
          )}
          {isNew && <button className="btn ghost small" onClick={onDone}>Annuler</button>}
          {!isNew && !dirty && (
            <button
              className="btn ghost small" disabled={job.busy}
              title={provider?.kind === "claude-code" ? "Consomme un peu de quota" : "Gratuit"}
              onClick={async () => {
                const r = await job.run(() => api.testRoute(route.modelRef));
                if (r) job.setOk(`OK · ${r.servedBy} · ${(r.latencyMs / 1000).toFixed(1)} s`);
              }}
            >
              {job.busy ? "test…" : "Tester"}
            </button>
          )}
          {!isNew && !RESERVED[route.modelRef] && (
            <DangerButton label="✕" confirmLabel="Supprimer" onConfirm={async () => {
              const done = await job.run(() => api.deleteRoute(route.modelRef));
              if (done !== undefined) await afterSave();
            }} />
          )}
        </div>
      </div>
      <FeedbackLine error={job.error} ok={job.ok} />
    </div>
  );
}

function FeedbackLine({ error, ok }: { error: string | null; ok: string | null }) {
  if (!error && !ok) return null;
  return <div className="feedback inline" data-kind={error ? "error" : "ok"}>{error ?? ok}</div>;
}
