import { useMemo, useState } from "react";
import { duration, loaderName, relTime, sizeText, statusOf, type AccountRef, type AppInfo, type Instance } from "../api";
import { isActive, type LaunchState } from "../launches";
import { LaunchProgress, PlayButton, ProfileIcon, StatusBadge } from "../ui/common";
import { CrashCard } from "../ui/CrashCard";
import type { Tab } from "./InstancePage";
import { nick, useStreamer } from "../streamer";
import { t } from "../i18n";

type Sort = "recent" | "name" | "version" | "created";

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
    if (ms.length === 0 && !info.dev_launch) out.push({ cls: "err", text: t("Нет аккаунта Microsoft"), hint: t("Войдите на экране «Аккаунты»") });
    else if (ms.length > 0 && !ms.some((a) => a.owns_java_edition)) out.push({ cls: "err", text: t("На аккаунте не куплена Java Edition") });
    if (ms.length === 0 && info.dev_launch) out.push({ cls: "warn", text: t("Режим разработчика: запуск без входа"), hint: t("только для отладки на своей копии игры") });
  } else {
    out.push({ cls: "warn", text: t("Профиль входит через Ely.by"), hint: t("только совместимые серверы; официальные недоступны") });
  }
  const free = info.free_bytes;
  if (free !== undefined && free < 5 * 1024 ** 3) {
    out.push({ cls: free < 2 * 1024 ** 3 ? "err" : "warn", text: t("Мало места на диске: свободно {0}", sizeText(free)), hint: t("версии игры с модами нужно 1–3 ГБ") });
  }
  const l = inst.last_launch;
  if (l && (l.outcome === "crashed" || l.outcome === "failed_to_start")) {
    out.push({ cls: "err", text: l.outcome === "crashed" ? t("Последний запуск упал ({0})", relTime(l.at)) : t("Последний запуск не начался"), hint: l.message });
  }
  return out;
}

export function Home(p: Props) {
  const [view, setView] = useState<"cards" | "list">(() => (localStorage.getItem("profilesView") as "cards" | "list") ?? "cards");
  const setViewSaved = (v: "cards" | "list") => { setView(v); try { localStorage.setItem("profilesView", v); } catch { /* недоступно — не страшно */ } };
  const streamer = useStreamer();
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>(() => (localStorage.getItem("profilesSort") as Sort) ?? "recent");
  const [filter, setFilter] = useState("all");
  const setSortSaved = (v: Sort) => { setSort(v); try { localStorage.setItem("profilesSort", v); } catch { /* не страшно */ } };
  const groups = useMemo(() => [...new Set(p.instances.map((i) => i.group).filter((g): g is string => !!g))].sort(), [p.instances]);
  const archived = p.instances.filter((i) => i.archived).length;
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = p.instances.filter((i) => {
      if (filter === "archive" ? !i.archived : i.archived) return false;
      if (filter === "fav" && !i.favorite) return false;
      if (filter.startsWith("g:") && i.group !== filter.slice(2)) return false;
      return !q || [i.name, i.game_version, loaderName[i.loader], i.group ?? "", i.description ?? ""].some((x) => x.toLowerCase().includes(q));
    });
    const key: Record<Sort, (i: Instance) => string | number> = {
      recent: (i) => -(i.last_launch?.at ?? 0),
      name: (i) => i.name.toLowerCase(),
      version: (i) => i.game_version,
      created: (i) => -i.created_at,
    };
    return list.sort((a, b) => {
      if (!!a.favorite !== !!b.favorite) return a.favorite ? -1 : 1;
      const ka = key[sort](a), kb = key[sort](b);
      if (sort === "version") return String(kb).localeCompare(String(ka), undefined, { numeric: true });
      return ka < kb ? -1 : ka > kb ? 1 : 0;
    });
  }, [p.instances, query, sort, filter]);
  const inst = p.selected;
  const state = inst ? p.states.get(inst.id) : undefined;

  if (p.instances.length === 0) {
    return (
      <div className="card empty">
        <h2>{t("Профилей пока нет")}</h2>
        <p>{t("Профиль — отдельная папка игры со своей версией, загрузчиком, модами и мирами. Профили не мешают друг другу.")}</p>
        <div className="row" style={{ justifyContent: "center" }}>
          <button className="btn pri" onClick={p.onCreate}>{t("+ Новый профиль")}</button>
          <button className="btn" onClick={p.onImport}>{t("Импорт из Prism, .mrpack или CurseForge")}</button>
        </div>
        {p.broken.length > 0 && <p className="small" style={{ color: "var(--warn)" }}>{t("Не удалось прочитать профилей:")} {p.broken.length}</p>}
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
              <ProfileIcon id={inst.id} color={inst.color} size={64} className="ficon" />
              <h3>{inst.name}</h3>
              <div className="chips">
                <span className="chip">Minecraft {inst.game_version}</span>
                <span className="chip">{loaderName[inst.loader]}{inst.loader_version ? ` ${inst.loader_version}` : ""}</span>
                {inst.last_launch && <span className="chip">{t("последний запуск")} {relTime(inst.last_launch.at)}{inst.last_launch.duration_secs ? `, ${duration(inst.last_launch.duration_secs)}` : ""}</span>}
              </div>
              <div className="play-wrap">
                <PlayButton inst={inst} state={state} />
                <StatusBadge inst={inst} state={state} />
                <span className="muted small">
                  {inst.auth_mode === "ely_by" ? t("Вход: Ely.by (совместимые серверы)") : ms ? t("Аккаунт: {0} (Microsoft)", nick(ms.name, streamer)) : p.info.dev_launch ? t("Без входа (dev-launch)") : t("Нет аккаунта")}
                </span>
              </div>
              {state && isActive(state) && <LaunchProgress state={state} />}
              {state?.phase === "failed" && state.error && (
                <div className="alert err" style={{ marginTop: 12 }}>
                  <div className="h">{t("Запуск не удался")}</div>
                  <span style={{ userSelect: "text" }}>{state.error}</span>
                </div>
              )}
              {state?.phase === "done" && state.exit?.crashed && (
                <div style={{ marginTop: 12 }}>
                  {state.exit.analysis ? (
                    <CrashCard inst={inst} a={state.exit.analysis} code={state.exit.code} onOpenTab={(t) => p.onOpen(inst.id, t)} />
                  ) : (
                    <div className="alert err"><div className="h">{t("Игра упала")}{state.exit.code != null ? t(" (код {0})", state.exit.code) : ""}</div></div>
                  )}
                  <button className="btn link small" style={{ marginTop: 6 }} onClick={() => p.onLogs(inst.id)}>{t("Открыть полный лог")}</button>
                </div>
              )}
              <div className="row" style={{ marginTop: 14 }}>
                <span className="muted small">{t("Быстро:")}</span>
                {inst.loader !== "vanilla" && <button className="btn" onClick={() => p.onOpen(inst.id, "mods")}>{t("Моды")}</button>}
                <button className="btn" onClick={() => p.onOpen(inst.id, "launch")}>{t("Настройки профиля")}</button>
                <button className="btn" onClick={() => p.onOpen(inst.id, "snapshots")}>{t("Снимки")}</button>
                <button className="btn" onClick={() => p.onLogs(inst.id)}>{t("Логи")}</button>
              </div>
            </div>
          </div>
          <div className="card">
            <h4 style={{ margin: "0 0 10px" }}>{t("Проверка перед запуском")}</h4>
            {(() => {
              const r = remarks(inst, p.accounts, p.info);
              if (r.length === 0) return <p className="muted small" style={{ margin: 0 }}><span className="s ok" style={{ display: "inline-grid", width: 18, height: 18, borderRadius: "50%", placeItems: "center", marginRight: 6 }}>✓</span>{t("Замечаний нет. Файлы и Java проверяются при каждом запуске.")}</p>;
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
            {!ms && !p.info.dev_launch && <button className="btn pri" style={{ marginTop: 12 }} onClick={p.onAccounts}>{t("Войти через Microsoft")}</button>}
          </div>
        </div>
      )}

      <div className="sect">
        <h4>{t("Профили ·")} {p.instances.length - archived}</h4>
        <div className="row">
          <input className="field" type="search" placeholder={t("Поиск профиля…")} aria-label={t("Поиск профиля")} value={query} onChange={(e) => setQuery(e.target.value)} style={{ width: 170 }} />
          <select className="field auto" aria-label={t("Показывать")} value={filter} onChange={(e) => setFilter(e.target.value)}>
            <option value="all">{t("Все")}</option>
            <option value="fav">{t("Избранные")}</option>
            {groups.map((g) => <option key={g} value={`g:${g}`}>{t("Группа:")} {g}</option>)}
            {archived > 0 && <option value="archive">{t("Архив (")}{archived})</option>}
          </select>
          <select className="field auto" aria-label={t("Сортировка")} value={sort} onChange={(e) => setSortSaved(e.target.value as Sort)}>
            <option value="recent">{t("Недавние")}</option>
            <option value="name">{t("По имени")}</option>
            <option value="version">{t("По версии")}</option>
            <option value="created">{t("Новые")}</option>
          </select>
          <div className="seg" role="group" aria-label={t("Вид списка")}>
            <button className={view === "cards" ? "on" : ""} onClick={() => setViewSaved("cards")}>{t("Карточки")}</button>
            <button className={view === "list" ? "on" : ""} onClick={() => setViewSaved("list")}>{t("Список")}</button>
          </div>
          <button className="btn" onClick={p.onCreate}>{t("+ Новый профиль")}</button>
          <button className="btn" onClick={p.onImport}>{t("Импорт .mrpack / zip / Prism")}</button>
        </div>
      </div>

      {view === "cards" ? (
        <div className="cards">
          {shown.map((i) => {
            const ls = p.states.get(i.id);
            const st = statusOf(i, isActive(ls) ? ls!.phase : undefined);
            return (
              <button key={i.id} className={`pcard ${i.id === inst?.id ? "sel" : ""}`} onClick={() => p.onSelect(i.id)} onDoubleClick={() => p.onOpen(i.id)}>
                <div className="t">
                  <ProfileIcon id={i.id} color={i.color} />
                  <div style={{ minWidth: 0 }}>
                    <b>{i.favorite && <span className="star" aria-label={t("в избранном")}>★ </span>}{i.name}</b>
                    <span className="muted small">{i.game_version} · {loaderName[i.loader]}{i.group ? ` · ${i.group}` : ""}</span>
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
            <thead><tr><th>{t("Профиль")}</th><th>{t("Версия")}</th><th>{t("Загрузчик")}</th><th>{t("Последний запуск")}</th></tr></thead>
            <tbody>
              {shown.map((i) => (
                <tr key={i.id} className={i.id === inst?.id ? "sel" : ""} tabIndex={0} aria-selected={i.id === inst?.id} onClick={() => p.onSelect(i.id)} onDoubleClick={() => p.onOpen(i.id)}
                  onKeyDown={(e) => { if (e.key === "Enter") p.onOpen(i.id); else if (e.key === " ") { e.preventDefault(); p.onSelect(i.id); } }}>
                  <td><div className="row"><ProfileIcon id={i.id} color={i.color} size={22} className="" /> {i.favorite && <span className="star">★</span>}{i.name}{i.group && <span className="muted small">· {i.group}</span>}</div></td>
                  <td>{i.game_version}</td>
                  <td>{loaderName[i.loader]} {i.loader_version ?? ""}</td>
                  <td><StatusBadge inst={i} state={p.states.get(i.id)} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {shown.length === 0 && <p className="muted">{t("Ничего не найдено")}{query ? t(" по запросу «{0}»", query) : ""}.</p>}
      {p.broken.length > 0 && (
        <div className="alert warn" style={{ marginTop: 14 }}>
          <div className="h">{t("Не удалось прочитать профили:")} {p.broken.length}</div>
          {p.broken.map((b) => <span key={b.dir} className="small mono">{b.dir}: {b.reason}</span>)}
          <span className="small muted">{t("Файлы не изменены. Исправьте instance.toml или уберите папку из instances/.")}</span>
        </div>
      )}
    </>
  );
}
