import { useEffect, useRef, useState } from "react";
import { api, errText, mb, statusOf, type Instance, type QuickPlay } from "../api";
import { isActive, launches, type LaunchState } from "../launches";
import { Modal, useDialog } from "./Dialog";

const ICONS = [
  "linear-gradient(#5d9e3a 0 30%,#7a5537 30% 100%)",
  "linear-gradient(#3a78c9 0 30%,#a78bfa 30% 100%)",
  "linear-gradient(#8bc34a 0 30%,#8d6e63 30% 100%)",
  "linear-gradient(#d0543c 0 30%,#5d4037 30% 100%)",
  "linear-gradient(#555 0 30%,#222 30% 100%)",
  "linear-gradient(#e8b04a 0 30%,#a0660a 30% 100%)",
];

export function iconFor(id: string): string {
  let h = 0;
  for (const c of id) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return ICONS[h % ICONS.length];
}

export function ProfileIcon({ id, size = 36, className = "pi" }: { id: string; size?: number; className?: string }) {
  return <div className={className} style={{ width: size, height: size, background: iconFor(id) }} aria-hidden />;
}

export function StatusBadge({ inst, state }: { inst: Instance; state?: LaunchState }) {
  const s = statusOf(inst, isActive(state) ? state!.phase : undefined);
  return <span className={`badge ${s.cls}`}>{s.text}</span>;
}

/** Ход подготовки: этап, файлы, мегабайты. */
export function LaunchProgress({ state }: { state: LaunchState }) {
  const p = state.progress;
  const pct = p && p.bytes_total > 0 ? Math.min(100, (p.bytes_done / p.bytes_total) * 100) : p && p.files_total > 0 ? (p.files_done / p.files_total) * 100 : 0;
  return (
    <div className="prep" aria-live="polite">
      <div className="row small">
        {state.phase === "preparing" && <span className="spinner" />}
        <b>{state.stageLabel}</b>
        {p && p.files_total > 0 && (
          <span className="muted">
            {p.files_done} из {p.files_total} файлов · {mb(p.bytes_done)} / {mb(p.bytes_total)} МБ{p.retries ? ` · повторов: ${p.retries}` : ""}
          </span>
        )}
      </div>
      {p && p.files_total > 0 && <div className="bar wide"><i style={{ width: `${pct}%` }} /></div>}
      {state.javaReason && <span className="muted small">Java: {state.javaReason}</span>}
      {state.warnings.map((w, i) => <span key={i} className="small" style={{ color: "var(--warn)" }}>⚠ {w}</span>)}
    </div>
  );
}

function QuickPlayDialog({ kind, onClose, onGo }: { kind: "singleplayer" | "multiplayer"; onClose: () => void; onGo: (q: QuickPlay) => void }) {
  const [target, setTarget] = useState("");
  return (
    <Modal title={kind === "singleplayer" ? "Сразу в мир" : "Сразу на сервер"} onClose={onClose}>
      <form className="form" onSubmit={(e) => { e.preventDefault(); if (target.trim()) onGo({ kind, target: target.trim() }); }}>
        <label>
          <span>{kind === "singleplayer" ? "Имя папки мира (в saves/)" : "Адрес сервера, например play.example.net:25565"}</span>
          <input className="field" value={target} onChange={(e) => setTarget(e.target.value)} data-autofocus />
        </label>
        <span className="muted small">Quick Play работает в версиях, которые его поддерживают; для старых версий вход на сервер передаётся через --server/--port.</span>
        <div className="acts">
          <button type="button" className="btn" onClick={onClose}>Отмена</button>
          <button className="btn pri" disabled={!target.trim()}>Играть</button>
        </div>
      </form>
    </Modal>
  );
}

export function PlayButton({ inst, state, compact }: { inst: Instance; state?: LaunchState; compact?: boolean }) {
  const dialog = useDialog();
  const [menu, setMenu] = useState(false);
  const [quick, setQuick] = useState<null | "singleplayer" | "multiplayer">(null);
  const ref = useRef<HTMLDivElement>(null);
  const active = isActive(state);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node)) setMenu(false); };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [menu]);

  const go = async (quickPlay?: QuickPlay, fullVerify = false) => {
    setMenu(false);
    setQuick(null);
    launches.begin(inst.id);
    try {
      await api.launch(inst.id, quickPlay, fullVerify);
    } catch (e) {
      await dialog.alert("Не удалось начать запуск", errText(e));
    }
  };

  const stop = async () => {
    if (state?.phase === "running") {
      const ok = await dialog.confirm({
        title: "Закрыть игру?",
        body: "Игра будет закрыта без сохранения последних секунд. Мир сохраняется игрой периодически; надёжнее выйти из мира в самой игре.",
        confirm: "Закрыть игру",
        danger: true,
      });
      if (!ok) return;
    }
    await api.stop(inst.id);
  };

  if (active) {
    return (
      <div className="play">
        <button className="go solo stop" onClick={stop} style={compact ? { padding: "8px 16px", minWidth: 0, fontSize: 14 } : undefined}>
          {state?.phase === "running" ? "■ Закрыть игру" : "Отменить"}
        </button>
      </div>
    );
  }
  return (
    <div className="play" ref={ref}>
      <button className="go" onClick={() => go()} style={compact ? { padding: "8px 18px", minWidth: 0, fontSize: 14 } : undefined}>▶ Играть</button>
      <button className="more" aria-label="Другие варианты запуска" aria-expanded={menu} onClick={() => setMenu((m) => !m)}>▾</button>
      {menu && (
        <div className="menu" role="menu">
          <button role="menuitem" onClick={() => setQuick("singleplayer")}>Сразу в мир…<small>Quick Play: запуск сразу в выбранный мир</small></button>
          <button role="menuitem" onClick={() => setQuick("multiplayer")}>Сразу на сервер…<small>Quick Play или --server для старых версий</small></button>
          <button role="menuitem" onClick={() => go(undefined, true)}>Проверить файлы и играть<small>Пересчитать хэши всех файлов и докачать повреждённые</small></button>
          <button role="menuitem" onClick={() => { setMenu(false); api.openFolder(inst.id).catch((e) => dialog.alert("Не удалось открыть папку", errText(e))); }}>Открыть папку профиля</button>
        </div>
      )}
      {quick && <QuickPlayDialog kind={quick} onClose={() => setQuick(null)} onGo={(q) => go(q)} />}
    </div>
  );
}
