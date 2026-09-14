/** Petits éléments partagés. */
import { useEffect, useState } from "react";
import type { AgentStatus } from "../ipc";

/**
 * Horloge locale. Le temps écoulé est calculé côté interface à partir
 * d'un horodatage de départ : le moteur n'a pas à émettre un snapshot
 * par seconde juste pour faire avancer un compteur.
 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}

export function elapsed(startedAt: string | null | undefined, now: number): string {
  return startedAt ? formatDuration(now - Date.parse(startedAt)) : "—";
}
import { STATUS_COLOR } from "../world/palette";

const LABEL: Record<AgentStatus, string> = {
  idle: "au repos",
  working: "en cours",
  waiting: "en attente",
  "needs-approval": "validation",
  paused: "en pause",
  error: "erreur",
  completed: "terminé",
};

export function StatusPill({ status }: { status: AgentStatus }) {
  return (
    <span className="status-pill" style={{ color: STATUS_COLOR[status] }}>
      <span className="dot" />
      {LABEL[status]}
    </span>
  );
}

export function Dot({ status }: { status: AgentStatus }) {
  return (
    <span
      style={{
        width: 8, height: 8, borderRadius: "50%",
        background: STATUS_COLOR[status], flex: "none",
      }}
    />
  );
}

/** Durée compacte : « 12 min », « 1 h 04 ». */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")}`;
}

export function formatTime(iso: string): string {
  const d = new Date(iso);
  return d.toLocaleTimeString("fr-FR", { hour12: false });
}
