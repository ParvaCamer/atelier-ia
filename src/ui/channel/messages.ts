/**
 * Du flux de journal brut à une discussion lisible.
 *
 * Le moteur n'émet que des lignes (`LogLine`) ; c'est ici qu'elles
 * deviennent des messages : une demande, des notes de l'orchestrateur, et
 * pour chaque agent un bloc qui réunit ses réflexions, ses commandes et
 * leurs sorties. Fonction pure : aucune donnée inventée, seulement regroupée.
 */
import type { AgentId, LogLine, ProjectId, TaskId } from "../../ipc";

export type EntryKind = "thought" | "command" | "output" | "stderr" | "note" | "done" | "failed";

export interface Entry {
  id: string;
  kind: EntryKind;
  text: string;
}

export type ChannelItem =
  | { kind: "request"; id: string; ts: string; text: string; projectId: ProjectId | null }
  | { kind: "orchestrator"; id: string; ts: string; projectId: ProjectId | null; notes: Entry[] }
  | { kind: "agent"; id: string; ts: string; agentId: AgentId; taskId: TaskId | null; entries: Entry[] };

/** Au-delà de ce silence, le même interlocuteur ouvre un nouveau message. */
const SAME_MESSAGE_MS = 2 * 60_000;
const REQUEST = /^◆ demande : « (.*) »$/s;

function agentEntry(line: LogLine): Entry {
  const { id, text } = line;
  if (line.stream === "command") return { id, kind: "command", text: text.replace(/^\$ /, "") };
  if (line.stream === "stdout") return { id, kind: "output", text };
  if (line.stream === "stderr") return { id, kind: "stderr", text };
  if (text.startsWith("💭 ")) return { id, kind: "thought", text: text.slice(3) };
  if (text.startsWith("✓ ")) return { id, kind: "done", text: text.slice(2) };
  if (text.startsWith("✗ ")) return { id, kind: "failed", text: text.slice(2) };
  return { id, kind: "note", text };
}

export function toChannel(lines: readonly LogLine[]): ChannelItem[] {
  const out: ChannelItem[] = [];
  for (const line of lines) {
    const last = out[out.length - 1];
    const close = !!last && Date.parse(line.ts) - Date.parse(last.ts) < SAME_MESSAGE_MS;

    if (!line.agentId) {
      const request = REQUEST.exec(line.text);
      if (request) {
        out.push({ kind: "request", id: line.id, ts: line.ts, text: request[1], projectId: line.projectId });
        continue;
      }
      const note: Entry = { id: line.id, kind: "note", text: line.text.replace(/^◆ /, "") };
      if (close && last.kind === "orchestrator") {
        last.notes.push(note);
        last.ts = line.ts;
      } else {
        out.push({ kind: "orchestrator", id: line.id, ts: line.ts, projectId: line.projectId, notes: [note] });
      }
      continue;
    }

    const entry = agentEntry(line);
    if (close && last.kind === "agent" && last.agentId === line.agentId && last.taskId === line.taskId) {
      const prev = last.entries[last.entries.length - 1];
      // Les lignes de sortie consécutives forment un seul bloc.
      if (prev && (entry.kind === "output" || entry.kind === "stderr") && prev.kind === entry.kind) {
        prev.text += `\n${entry.text}`;
      } else {
        last.entries.push(entry);
      }
      last.ts = line.ts;
    } else {
      out.push({ kind: "agent", id: line.id, ts: line.ts, agentId: line.agentId, taskId: line.taskId, entries: [entry] });
    }
  }
  return out;
}
