/** Primitives de formulaire des réglages. Aucune validation : c'est le moteur qui tranche. */
import { useState, type ReactNode } from "react";

export function Field({ label, hint, children, wide }: { label: string; hint?: ReactNode; children: ReactNode; wide?: boolean }) {
  return (
    <label className="field" data-wide={wide}>
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  );
}

export function Text({ value, onChange, placeholder, mono, disabled }: {
  value: string; onChange: (v: string) => void; placeholder?: string; mono?: boolean; disabled?: boolean;
}) {
  return (
    <input
      className="input" data-mono={mono} value={value} placeholder={placeholder} disabled={disabled}
      spellCheck={false} autoCorrect="off" autoCapitalize="off"
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

export function Area({ value, onChange, placeholder, rows = 4, mono }: {
  value: string; onChange: (v: string) => void; placeholder?: string; rows?: number; mono?: boolean;
}) {
  return (
    <textarea
      className="input" data-mono={mono} rows={rows} value={value} placeholder={placeholder}
      spellCheck={false} onChange={(e) => onChange(e.target.value)}
    />
  );
}

export function Select<T extends string>({ value, options, onChange, disabled }: {
  value: T; options: { value: T; label: string }[]; onChange: (v: T) => void; disabled?: boolean;
}) {
  return (
    <select className="input" value={value} disabled={disabled} onChange={(e) => onChange(e.target.value as T)}>
      {options.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
    </select>
  );
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button type="button" className="toggle" data-on={checked} onClick={() => onChange(!checked)}>
      <span className="toggle-track"><span className="toggle-thumb" /></span>
      {label}
    </button>
  );
}

/** Liste de mots saisie comme texte séparé par des virgules. */
export function Tags({ values, onChange, placeholder }: { values: string[]; onChange: (v: string[]) => void; placeholder?: string }) {
  const [text, setText] = useState(values.join(", "));
  return (
    <input
      className="input" value={text} placeholder={placeholder}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => onChange(text.split(",").map((t) => t.trim()).filter(Boolean))}
    />
  );
}

/** Suppression en deux temps : pas de boîte de dialogue, pas de clic malheureux. */
export function DangerButton({ label, confirmLabel, onConfirm, disabled, ariaLabel }: {
  label: string; confirmLabel: string; onConfirm: () => void; disabled?: boolean;
  /** Obligatoire quand `label` est une icône (✕) : c'est ce que lit un lecteur d'écran. */
  ariaLabel?: string;
}) {
  const [armed, setArmed] = useState(false);
  return armed ? (
    // Échap annule la confirmation sans fermer l'écran derrière.
    <span className="danger-confirm" onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); setArmed(false); } }}>
      <button type="button" className="btn danger" autoFocus onClick={() => { setArmed(false); onConfirm(); }}>{confirmLabel}</button>
      <button type="button" className="btn ghost" onClick={() => setArmed(false)}>annuler</button>
    </span>
  ) : (
    <button type="button" className="btn ghost" disabled={disabled} aria-label={ariaLabel} title={ariaLabel} onClick={() => setArmed(true)}>{label}</button>
  );
}

export function useJob() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  async function run<T>(fn: () => Promise<T>, success?: string): Promise<T | undefined> {
    setBusy(true);
    setError(null);
    setOk(null);
    try {
      const result = await fn();
      if (success) setOk(success);
      return result;
    } catch (e) {
      setError(String(e));
      return undefined;
    } finally {
      setBusy(false);
    }
  }
  return { busy, error, ok, run, setError, setOk };
}

export function Feedback({ error, ok }: { error: string | null; ok: string | null }) {
  if (error) return <div className="feedback" data-kind="error">{error}</div>;
  if (ok) return <div className="feedback" data-kind="ok">{ok}</div>;
  return null;
}
