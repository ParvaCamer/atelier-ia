/**
 * Validations en attente.
 *
 * Posées au-dessus du monde, pas cachées dans un panneau : une tâche est
 * suspendue tant que je n'ai pas répondu, ça doit se voir immédiatement.
 */
import { useWorld } from "../state/store";

export function Approvals() {
  const approvals = useWorld((s) => s.approvals);
  const agentsById = useWorld((s) => s.agentsById);
  const resolve = useWorld((s) => s.resolveApproval);
  const select = useWorld((s) => s.select);

  if (!approvals.length) return null;

  return (
    <div className="approvals">
      {approvals.map((a) => (
        <div className="approval" key={a.id}>
          <div className="approval-head">
            <span className="badge">validation</span>
            <button className="link" onClick={() => select(a.agentId)}>
              {agentsById.get(a.agentId)?.name ?? "agent"}
            </button>
          </div>
          <div className="approval-summary">{a.summary}</div>
          <div className="approval-reason">{a.reason}</div>
          {a.details && a.details !== "{}" && <pre className="approval-details">{a.details}</pre>}
          <div className="approval-actions">
            <button onClick={() => resolve(a.id, false)}>Refuser</button>
            <button className="primary" onClick={() => resolve(a.id, true)}>Autoriser</button>
          </div>
        </div>
      ))}
    </div>
  );
}

export function NoticeToast() {
  const notice = useWorld((s) => s.notice);
  const setNotice = useWorld((s) => s.setNotice);
  if (!notice) return null;
  return (
    <button className="notice" data-kind={notice.kind} onClick={() => setNotice(null)}>
      {notice.text}
    </button>
  );
}
