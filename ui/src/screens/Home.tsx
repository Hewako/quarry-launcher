import { useState } from "react";
import { duration, loaderName, relTime, statusOf, type AccountRef, type AppInfo, type Instance } from "../api";
import { isActive, type LaunchState } from "../launches";
import { LaunchProgress, PlayButton, ProfileIcon, StatusBadge } from "../ui/common";
import { CrashCard } from "../ui/CrashCard";
import type { Tab } from "./InstancePage";

interface Props {
  info: AppInfo;
  instances: Instance[];
  broken: { dir: string; reason: string }[];
  accounts: AccountRef[];
  states: Map<string, LaunchState>;
  selected?: Instance;
  onSelect: (id: string) => void;
  onOpen: (id: string, tab?: Tab) => void;
  onCreate: () => void;
  onImport: () => void;
  onAccounts: () => void;
  onLogs: (id: string) => void;
}

/** Только замечания: зелёные галочки на каждый запуск — шум (разбор игрока, п. 7). */
function remarks(inst: Instance, accounts: AccountRef[], info: AppInfo) {
  const out: { cls: "warn" | "err"; text: string; hint?: string }[] = [];
  const ms = accounts.filter((a) => a.provider === "microsoft");
  if (inst.auth_mode === "microsoft") {
    if (ms.length === 0 && !info.dev_launch) out.push({ cls: "err", text: "Нет аккаунта Microsoft", hint: "Войдите на экране «Аккаунты»" });
    else if (ms.length > 0 && !ms.some((a) => a.owns_java_edition)) out.push({ cls: "err", text: "На аккаунте не куплена Java Edition" });
    if (ms.length === 0 && info.dev_launch) out.push({ cls: "warn", text: "Режим разработчика: запуск без входа", hint: "только для отладки на своей копии игры" });
  } else {
    out.push({ cls: "warn", text: "Профиль входит через Ely.by", hint: "только совместимые серверы; официальные недоступны" });
  }
  const l = inst.last_launch;
  if (l && (l.outcome === "crashed" || l.outcome === "failed_to_start")) {
    out.push({ cls: "err", text: l.outcome === "crashed" ? `Последний запуск упал (${relTime(l.at)})` : "Последний запуск не начался", hint: l.message });
  }
  return out;
}

export function Home(p: Props) {
  const [view, setView] = useState<"cards" | "list">(() => (localStorage.getItem("profilesView") as "cards" | "list") ?? "cards");
  const setViewSaved = (v: "cards" | "list") => { setView(v); try { localStorage.setItem("profilesView", v); } catch { /* недоступно — не страшно */ } };
  const inst = p.selected;
  const state = inst ? p.states.get(inst.id) : undefined;

  if (p.instances.length === 0) {
    return (
      <div className="card empty">
        <h2>Профилей пока нет</h2>
        <p>Профиль — отдельная папка игры со своей версией, загрузчиком, модами и мирами. Профили не мешают друг другу.</p>
        <div className="row" style={{ justifyContent: "center" }}>
          <button className="btn pri" onClick={p.onCreate}>+ Новый профиль</button>
          <button className="btn" onClick={p.onImport}>Импорт из Prism, .mrpack или CurseForge</button>
        </div>
        {p.broken.length > 0 && <p className="small" style={{ color: "var(--warn)" }}>Не удалось прочитать профилей: {p.broken.length}</p>}
      </div>
    );
  }

  const ms = p.accounts.find((a) => a.provider === "microsoft" && a.owns_java_edition);
  // Какой Microsoft-аккаунт будет использован, решает ядро (аккаунт профиля → по умолчанию → первый).
  return (
    <>
      {inst && (
        <div className="hero">
          <div className="card feature">
            <div className="art" />
            <div className="body">
              <ProfileIcon id={inst.id} size={64} className="ficon" />
              <h3>{inst.name}</h3>
              <div className="chips">
                <span className="chip">Minecraft {inst.game_version}</span>
                <span className="chip">{loaderName[inst.loader]}{inst.loader_version ? ` ${inst.loader_version}` : ""}</span>
                {inst.last_launch && <span className="chip">последний запуск {relTime(inst.last_launch.at)}{inst.last_launch.duration_secs ? `, ${duration(inst.last_launch.duration_secs)}` : ""}</span>}
              </div>
              <div className="play-wrap">
                <PlayButton inst={inst} state={state} />
                <StatusBadge inst={inst} state={state} />
                <span className="muted small">
                  {inst.auth_mode === "ely_by" ? "Вход: Ely.by (совместимые серверы)" : ms ? `Аккаунт: ${ms.name} (Microsoft)` : p.info.dev_launch ? "Без входа (dev-launch)" : "Нет аккаунта"}
                </span>
              </div>
              {state && isActive(state) && <LaunchProgress state={state} />}
              {state?.phase === "failed" && state.error && (
                <div className="alert err" style={{ marginTop: 12 }}>
                  <div className="h">Запуск не удался</div>
                  <span style={{ userSelect: "text" }}>{state.error}</span>
                </div>
              )}
              {state?.phase === "done" && state.exit?.crashed && (
                <div style={{ marginTop: 12 }}>
                  {state.exit.analysis ? (
                    <CrashCard inst={inst} a={state.exit.analysis} code={state.exit.code} onOpenTab={(t) => p.onOpen(inst.id, t)} />
                  ) : (
                    <div className="alert err"><div className="h">Игра упала{state.exit.code != null ? ` (код ${state.exit.code})` : ""}</div></div>
                  )}
                  <button className="btn link small" style={{ marginTop: 6 }} onClick={() => p.onLogs(inst.id)}>Открыть полный лог</button>
                </div>
              )}
              <div className="row" style={{ marginTop: 14 }}>
                <span className="muted small">Быстро:</span>
                {inst.loader !== "vanilla" && <button className="btn" onClick={() => p.onOpen(inst.id, "mods")}>Моды</button>}
                <button className="btn" onClick={() => p.onOpen(inst.id, "launch")}>Настройки профиля</button>
                <button className="btn" onClick={() => p.onOpen(inst.id, "snapshots")}>Снимки</button>
                <button className="btn" onClick={() => p.onLogs(inst.id)}>Логи</button>
              </div>
            </div>
          </div>
          <div className="card">
            <h4 style={{ margin: "0 0 10px" }}>Проверка перед запуском</h4>
            {(() => {
              const r = remarks(inst, p.accounts, p.info);
              if (r.length === 0) return <p className="muted small" style={{ margin: 0 }}><span className="s ok" style={{ display: "inline-grid", width: 18, height: 18, borderRadius: "50%", placeItems: "center", marginRight: 6 }}>✓</span>Замечаний нет. Файлы и Java проверяются при каждом запуске.</p>;
              return (
                <ul className="pre">
                  {r.map((x, i) => (
                    <li key={i}>
                      <span className={`s ${x.cls}`}>{x.cls === "err" ? "!" : "·"}</span>
                      <div>{x.text}{x.hint && <em>{x.hint}</em>}</div>
                    </li>
                  ))}
                </ul>
              );
            })()}
            {!ms && !p.info.dev_launch && <button className="btn pri" style={{ marginTop: 12 }} onClick={p.onAccounts}>Войти через Microsoft</button>}
          </div>
        </div>
      )}

      <div className="sect">
        <h4>Профили · {p.instances.length}</h4>
        <div className="row">
          <div className="seg" role="group" aria-label="Вид списка">
            <button className={view === "cards" ? "on" : ""} onClick={() => setViewSaved("cards")}>Карточки</button>
            <button className={view === "list" ? "on" : ""} onClick={() => setViewSaved("list")}>Список</button>
          </div>
          <button className="btn" onClick={p.onCreate}>+ Новый профиль</button>
          <button className="btn" onClick={p.onImport}>Импорт .mrpack / zip / Prism</button>
        </div>
      </div>

      {view === "cards" ? (
        <div className="cards">
          {p.instances.map((i) => {
            const ls = p.states.get(i.id);
            const st = statusOf(i, isActive(ls) ? ls!.phase : undefined);
            return (
              <button key={i.id} className={`pcard ${i.id === inst?.id ? "sel" : ""}`} onClick={() => p.onSelect(i.id)} onDoubleClick={() => p.onOpen(i.id)}>
                <div className="t">
                  <ProfileIcon id={i.id} />
                  <div style={{ minWidth: 0 }}>
                    <b>{i.name}</b>
                    <span className="muted small">{i.game_version} · {loaderName[i.loader]}</span>
                  </div>
                </div>
                <span><span className={`badge ${st.cls}`}>{st.text}</span></span>
              </button>
            );
          })}
        </div>
      ) : (
        <div className="card" style={{ padding: 0 }}>
          <table className="plist">
            <thead><tr><th>Профиль</th><th>Версия</th><th>Загрузчик</th><th>Последний запуск</th></tr></thead>
            <tbody>
              {p.instances.map((i) => (
                <tr key={i.id} className={i.id === inst?.id ? "sel" : ""} onClick={() => p.onSelect(i.id)} onDoubleClick={() => p.onOpen(i.id)}>
                  <td><div className="row"><ProfileIcon id={i.id} size={22} className="" /> {i.name}</div></td>
                  <td>{i.game_version}</td>
                  <td>{loaderName[i.loader]} {i.loader_version ?? ""}</td>
                  <td><StatusBadge inst={i} state={p.states.get(i.id)} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {p.broken.length > 0 && (
        <div className="alert warn" style={{ marginTop: 14 }}>
          <div className="h">Не удалось прочитать профили: {p.broken.length}</div>
          {p.broken.map((b) => <span key={b.dir} className="small mono">{b.dir}: {b.reason}</span>)}
          <span className="small muted">Файлы не изменены. Исправьте instance.toml или уберите папку из instances/.</span>
        </div>
      )}
    </>
  );
}
