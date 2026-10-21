import { useEffect, useRef, useState } from "react";
import { api, errText, mb, statusOf, type Instance, type QuickPlay } from "../api";
import { isActive, launches, type LaunchState } from "../launches";
import { Modal, useDialog } from "./Dialog";
import { t } from "../i18n";

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

export function ProfileIcon({ id, color, size = 36, className = "pi" }: { id: string; color?: string; size?: number; className?: string }) {
  return <div className={className} style={{ width: size, height: size, background: color ?? iconFor(id) }} aria-hidden />;
}

export function StatusBadge({ inst, state }: { inst: Instance; state?: LaunchState }) {
  const s = statusOf(inst, isActive(state) ? state!.phase : undefined);
  return <span className={`badge ${s.cls}`}>{s.text}</span>;
}

/** Ход подготовки: этап, файлы, мегабайты. */
/** Скорость по последним ~3 секундам и оставшееся время. */
function useSpeed(done: number, total: number): { speed: number; eta: number | null } {
  const samples = useRef<[number, number][]>([]);
  const now = Date.now();
  const s = samples.current;
  if (!s.length || s[s.length - 1][1] !== done) s.push([now, done]);
  while (s.length > 2 && now - s[0][0] > 3000) s.shift();
  const [t0, b0] = s[0];
  const speed = now - t0 > 300 ? ((done - b0) * 1000) / (now - t0) : 0;
  return { speed, eta: speed > 0 && total > done ? (total - done) / speed : null };
}

const fmtEta = (sec: number) => (sec < 60 ? t("{0} с", Math.ceil(sec)) : t("{0} мин {1} с", Math.floor(sec / 60), Math.ceil(sec % 60)));

let pausedNow = false;

export function LaunchProgress({ state }: { state: LaunchState }) {
  const p = state.progress;
  const pct = p && p.bytes_total > 0 ? Math.min(100, (p.bytes_done / p.bytes_total) * 100) : p && p.files_total > 0 ? (p.files_done / p.files_total) * 100 : 0;
  const { speed, eta } = useSpeed(p?.bytes_done ?? 0, p?.bytes_total ?? 0);
  const [paused, setPaused] = useState(pausedNow);
  const togglePause = () => { pausedNow = !paused; setPaused(pausedNow); api.pauseDownloads(pausedNow).catch(() => {}); };
  const downloading = state.phase === "preparing" && !!p && p.files_total > 0 && p.files_done < p.files_total;
  // Загрузка закончилась или отменена — пауза не должна остаться на следующие загрузки.
  useEffect(() => {
    if (!downloading && pausedNow) { pausedNow = false; setPaused(false); api.pauseDownloads(false).catch(() => {}); }
  }, [downloading]);
  return (
    <div className="prep" aria-live="polite">
      <div className="row small">
        {state.phase === "preparing" && <span className="spinner" />}
        <b>{t(state.stageLabel)}</b>
        {p && p.files_total > 0 && (
          <span className="muted">
            {p.files_done} {t("из")} {p.files_total} {t("файлов ·")} {mb(p.bytes_done)} / {mb(p.bytes_total)} {t("МБ")}{p.retries ? t(" · повторов: {0}", p.retries) : ""}
            {downloading && (paused ? t(" · на паузе") : speed > 0 ? t(" · {0} МБ/с{1}", (speed / 1048576).toFixed(1), eta !== null ? t(" · осталось ≈ {0}", fmtEta(eta)) : "") : "")}
          </span>
        )}
        {downloading && <button className="btn link small" onClick={togglePause}>{paused ? t("Продолжить") : t("Пауза")}</button>}
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
    <Modal title={kind === "singleplayer" ? t("Сразу в мир") : t("Сразу на сервер")} onClose={onClose}>
      <form className="form" onSubmit={(e) => { e.preventDefault(); if (target.trim()) onGo({ kind, target: target.trim() }); }}>
        <label>
          <span>{kind === "singleplayer" ? t("Имя папки мира (в saves/)") : t("Адрес сервера, например play.example.net:25565")}</span>
          <input className="field" value={target} onChange={(e) => setTarget(e.target.value)} data-autofocus />
        </label>
        <span className="muted small">{t("Quick Play работает в версиях, которые его поддерживают; для старых версий вход на сервер передаётся через --server/--port.")}</span>
        <div className="acts">
          <button type="button" className="btn" onClick={onClose}>{t("Отмена")}</button>
          <button className="btn pri" disabled={!target.trim()}>{t("Играть")}</button>
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

  const go = async (quickPlay?: QuickPlay, fullVerify = false, safeMode = false) => {
    setMenu(false);
    setQuick(null);
    launches.begin(inst.id);
    try {
      await api.launch(inst.id, quickPlay, fullVerify, safeMode);
    } catch (e) {
      await dialog.alert(t("Не удалось начать запуск"), errText(e));
    }
  };

  const stop = async () => {
    if (state?.phase === "running") {
      const ok = await dialog.confirm({
        title: t("Закрыть игру?"),
        body: t("Игра будет закрыта без сохранения последних секунд. Мир сохраняется игрой периодически; надёжнее выйти из мира в самой игре."),
        confirm: t("Закрыть игру"),
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
          {state?.phase === "running" ? t("■ Закрыть игру") : t("Отменить")}
        </button>
      </div>
    );
  }
  return (
    <div className="play" ref={ref}>
      <button className="go" onClick={() => go()} style={compact ? { padding: "8px 18px", minWidth: 0, fontSize: 14 } : undefined}>{t("▶ Играть")}</button>
      <button className="more" aria-label={t("Другие варианты запуска")} aria-expanded={menu} onClick={() => setMenu((m) => !m)}>▾</button>
      {menu && (
        <div className="menu" role="menu">
          <button role="menuitem" onClick={() => setQuick("singleplayer")}>{t("Сразу в мир…")}<small>{t("Quick Play: запуск сразу в выбранный мир")}</small></button>
          <button role="menuitem" onClick={() => setQuick("multiplayer")}>{t("Сразу на сервер…")}<small>{t("Quick Play или --server для старых версий")}</small></button>
          <button role="menuitem" onClick={() => go(undefined, true)}>{t("Проверить файлы и играть")}<small>{t("Пересчитать хэши всех файлов и докачать повреждённые")}</small></button>
          {inst.loader !== "vanilla" && <button role="menuitem" onClick={() => go(undefined, false, true)}>{t("Безопасный запуск")}<small>{t("Без модов; моды вернутся после выхода из игры")}</small></button>}
          <button role="menuitem" onClick={() => { setMenu(false); api.openFolder(inst.id).catch((e) => dialog.alert(t("Не удалось открыть папку"), errText(e))); }}>{t("Открыть папку профиля")}</button>
        </div>
      )}
      {quick && <QuickPlayDialog kind={quick} onClose={() => setQuick(null)} onGo={(q) => go(q)} />}
    </div>
  );
}
