// Палитра команд (Ctrl+K / ⌘K): быстрый переход к профилям, вкладкам, экранам и действиям.
import { useEffect, useMemo, useRef, useState } from "react";

export interface Command { id: string; label: string; hint?: string; run: () => void }

export function Palette({ commands, onClose }: { commands: Command[]; onClose: () => void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const prev = useRef<HTMLElement | null>(document.activeElement as HTMLElement | null);

  const shown = useMemo(() => {
    const words = q.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return commands.filter((c) => words.every((w) => `${c.label} ${c.hint ?? ""}`.toLowerCase().includes(w))).slice(0, 12);
  }, [commands, q]);

  useEffect(() => {
    input.current?.focus();
    const back = prev.current;
    return () => back?.focus();
  }, []);
  useEffect(() => setSel(0), [q]);

  const run = (c?: Command) => { if (!c) return; onClose(); c.run(); };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") { e.preventDefault(); onClose(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(s + 1, shown.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(s - 1, 0)); }
    else if (e.key === "Enter") { e.preventDefault(); run(shown[sel]); }
    else if (e.key === "Tab") e.preventDefault(); // фокус остаётся в палитре
  };

  return (
    <div className="overlay" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="palette" role="dialog" aria-modal="true" aria-label="Палитра команд" onKeyDown={onKey}>
        <input ref={input} className="field" placeholder="Профиль, вкладка или действие…" value={q} onChange={(e) => setQ(e.target.value)}
          role="combobox" aria-expanded="true" aria-controls="palette-list" aria-activedescendant={shown[sel] ? `pc-${shown[sel].id}` : undefined} />
        <ul id="palette-list" role="listbox" aria-label="Команды">
          {shown.length === 0 && <li className="muted small" style={{ padding: 10 }}>Ничего не найдено</li>}
          {shown.map((c, i) => (
            <li key={c.id} id={`pc-${c.id}`} role="option" aria-selected={i === sel} className={i === sel ? "on" : ""}
              onMouseEnter={() => setSel(i)} onMouseDown={(e) => { e.preventDefault(); run(c); }}>
              <span>{c.label}</span>{c.hint && <span className="muted small">{c.hint}</span>}
            </li>
          ))}
        </ul>
        <p className="muted small" style={{ margin: "8px 4px 0" }}>↑↓ — выбрать · Enter — выполнить · Esc — закрыть</p>
      </div>
    </div>
  );
}
