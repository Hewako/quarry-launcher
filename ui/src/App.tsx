import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, mb, relTime, type AccountRef, type AppInfo, type GlobalSettings, type Instance } from "./api";
import { isActive, launches, useLaunches } from "./launches";
import { Accounts } from "./screens/Accounts";
import { CreateInstance } from "./screens/CreateInstance";
import { Home } from "./screens/Home";
import { ImportDialog } from "./screens/Import";
import { InstancePage, type Tab } from "./screens/InstancePage";
import { Logs } from "./screens/Logs";
import { Settings } from "./screens/Settings";
import { FirstRun } from "./screens/FirstRun";
import { Avatar } from "./ui/Avatar";
import { DialogProvider } from "./ui/Dialog";
import { nick, path, setStreamer, useStreamer } from "./streamer";
import { clearNotes, markRead, pushNote, useNotes } from "./notify";
import { Palette, type Command } from "./ui/Palette";
import { src, syncLang, t, tc } from "./i18n";

type Screen = "home" | "profile" | "doctor" | "history" | "logs" | "accounts" | "settings";

const NAV: { id: Screen; label: string; ico: string }[] = [
  { id: "home", label: t("Главная"), ico: "⌂" },
  { id: "profile", label: t("Профили"), ico: "▦" },
  { id: "doctor", label: "Mod Doctor", ico: "⚙" },
  { id: "history", label: t("Снимки и откат"), ico: "↺" },
  { id: "logs", label: t("Логи и краши"), ico: "≡" },
  { id: "accounts", label: t("Аккаунты и скины"), ico: "☺" },
  { id: "settings", label: t("Настройки"), ico: "✎" },
];

function applyTheme(theme: string) {
  const dark = theme === "system" ? window.matchMedia("(prefers-color-scheme: dark)").matches : theme !== "light";
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

/** Цвет текста на акцентном фоне: белый или тёмный — что контрастнее (WCAG). */
function inkFor(hex: string): string {
  const [r, g, b] = (hex.match(/\w\w/g) ?? ["0", "0", "0"]).map((x) => parseInt(x, 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  const l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  return 1.05 / (l + 0.05) >= (l + 0.05) / 0.05 ? "#ffffff" : "#06210f";
}

/** Акцентный цвет, масштаб и плотность интерфейса из настроек. */
const FONTS: Record<string, string> = {
  readable: 'Verdana, "DejaVu Sans", Tahoma, sans-serif',
  serif: 'Georgia, "Times New Roman", serif',
};

export function applyAppearance(g: Pick<GlobalSettings, "accent" | "ui_scale" | "compact" | "font">) {
  const root = document.documentElement;
  if (FONTS[g.font]) root.style.setProperty("--font", FONTS[g.font]); else root.style.removeProperty("--font");
  if (g.accent) {
    root.style.setProperty("--accent", g.accent);
    root.style.setProperty("--accent-ink", inkFor(g.accent));
  } else {
    root.style.removeProperty("--accent");
    root.style.removeProperty("--accent-ink");
  }
  root.style.setProperty("zoom", `${(g.ui_scale || 100) / 100}`);
  root.dataset.density = g.compact ? "compact" : "normal";
}

const TAB_LABELS: [Tab, string][] = [["mods", t("Моды")], ["deps", t("Зависимости")], ["resources", t("Ресурсы")], ["configs", t("Конфиги")], ["launch", t("Запуск и память")], ["options", t("Настройки игры")], ["snapshots", t("Снимки и миры")], ["servers", t("Серверы")], ["logs", t("Логи")], ["profile", t("Профиль")]];

/** Подпись строки настроек (div.lbl) называет поле этой строки для экранных читалок. */
function useSettingLabels() {
  useEffect(() => {
    let n = 0;
    const link = () => document.querySelectorAll<HTMLElement>(".setting").forEach((row) => {
      const lbl = row.querySelector<HTMLElement>(".lbl");
      const field = row.querySelector<HTMLElement>("input:not([type=checkbox]):not([type=radio]), select, textarea");
      if (!lbl || !field || field.hasAttribute("aria-label") || field.hasAttribute("aria-labelledby")) return;
      lbl.id ||= `setting-lbl-${++n}`;
      field.setAttribute("aria-labelledby", lbl.id);
    });
    const obs = new MutationObserver(link);
    obs.observe(document.body, { childList: true, subtree: true });
    link();
    return () => obs.disconnect();
  }, []);
}

function Shell() {
  useSettingLabels();
  const streamer = useStreamer();
  const states = useLaunches();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [broken, setBroken] = useState<{ dir: string; reason: string }[]>([]);
  const [modsCount, setModsCount] = useState<Record<string, number>>({});
  const [accounts, setAccounts] = useState<AccountRef[]>([]);
  const [selectedId, setSelectedId] = useState<string | undefined>();
  const [screen, setScreen] = useState<Screen>("home");
  const [profileTab, setProfileTab] = useState<Tab | undefined>();
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [theme, setTheme] = useState("dark");
  const [loadError, setLoadError] = useState("");
  const [palette, setPalette] = useState(false);
  const [firstRun, setFirstRun] = useState(false);
  const [release, setRelease] = useState<{ version: string; url: string } | null>(null);
  const [bell, setBell] = useState(false);
  const notes = useNotes();

  const refresh = useCallback(async () => {
    try {
      const r = await api.listInstances();
      setInstances(r.instances);
      setBroken(r.broken);
      setModsCount(r.mods ?? {});
      setSelectedId((cur) => (cur && r.instances.some((i) => i.id === cur) ? cur : r.instances[0]?.id));
      setLoadError("");
    } catch (e) {
      setLoadError(String(e));
    }
  }, []);
  const [defaultKey, setDefaultKey] = useState<string | undefined>();
  const refreshAccounts = useCallback(() => {
    api.listAccounts().then(setAccounts).catch(() => setAccounts([]));
    api.globalSettings().then((g) => { setDefaultKey(g.default_account); setStreamer(g.streamer_mode); }).catch(() => {});
  }, []);

  useEffect(() => {
    launches.start();
    api.appInfo().then(setInfo);
    api.globalSettings().then((g) => { setTheme(g.theme); applyTheme(g.theme); applyAppearance(g); setFirstRun(!g.first_run_done); syncLang(g.language); }).catch(() => applyTheme("dark"));
    refresh();
    refreshAccounts();
    api.launcherUpdate().then((r) => {
      if (!r) return;
      setRelease(r);
      pushNote({ kind: "info", title: src("Вышла новая версия Quarry {0}", r.version) });
    }).catch(() => {});
    const off = listen("launch-finished", () => refresh());
    // Ctrl+K / ⌘K — палитра команд.
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); setPalette((p) => !p); }
    };
    window.addEventListener("keydown", onKey);
    return () => { off.then((f) => f()); window.removeEventListener("keydown", onKey); };
  }, [refresh, refreshAccounts]);

  const changeTheme = (t: string) => {
    setTheme(t);
    applyTheme(t);
    api.globalSettings().then((g) => api.saveGlobalSettings({ ...g, theme: t })).catch(() => {});
  };

  const selected = instances.find((i) => i.id === selectedId);
  // states — одна и та же Map, которая меняется на месте: считаем на каждой отрисовке, без useMemo.
  let downloading: { name: string; s: NonNullable<ReturnType<typeof states.get>> } | null = null;
  for (const [id, s] of states) {
    if (s.phase === "preparing" && s.progress && s.progress.files_total > 0) {
      downloading = { name: instances.find((i) => i.id === id)?.name ?? id, s };
      break;
    }
  }
  const runningCount = [...states.values()].filter(isActive).length;
  const msAll = accounts.filter((a) => a.provider === "microsoft");
  const ms = msAll.find((a) => `${a.provider}:${a.uuid}` === defaultKey) ?? msAll[0];

  if (!info) return <div className="content muted">{t("Загрузка…")}</div>;
  const open = (id: string, tab?: Tab) => { setSelectedId(id); setProfileTab(tab); setScreen("profile"); };
  const commands: Command[] = [
    ...NAV.filter((n) => n.id !== "doctor" && n.id !== "history").map((n) => ({ id: `s-${n.id}`, label: n.label, hint: t("экран"), run: () => { setProfileTab(undefined); setScreen(n.id); } })),
    ...instances.flatMap((i) => [
      { id: `p-${i.id}`, label: i.name, hint: t("профиль · {0}", i.game_version), run: () => open(i.id) },
      { id: `play-${i.id}`, label: t("Играть: {0}", i.name), hint: t("запуск"), run: () => { launches.begin(i.id); api.launch(i.id).catch(() => {}); } },
    ]),
    ...(selected ? TAB_LABELS.map(([tab, l]) => ({ id: `t-${tab}`, label: `${selected.name}: ${t(l)}`, hint: t("вкладка"), run: () => open(selected.id, tab) })) : []),
    { id: "a-new", label: t("Новый профиль"), hint: t("действие"), run: () => setCreating(true) },
    { id: "a-import", label: t("Импорт сборки"), hint: ".mrpack / zip / Prism", run: () => setImporting(true) },
    { id: "a-theme", label: t("Сменить тему"), hint: t("светлая / тёмная"), run: () => changeTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark") },
  ];

  return (
    <div className="app">
      <a href="#main" className="skip">{t("Перейти к содержимому")}</a>
      <aside className="side">
        <div className="logo"><i /><div>Quarry<small>{t("рабочее название · v")}{info.version}</small></div></div>
        <nav className="nav" aria-label={t("Разделы")}>
          {NAV.map((n) => (
            <button key={n.id} className={(n.id === "doctor" ? screen === "profile" && profileTab === "mods" : n.id === "history" ? screen === "profile" && profileTab === "snapshots" : n.id === "profile" ? screen === "profile" && profileTab !== "mods" && profileTab !== "snapshots" : screen === n.id) ? "on" : ""} aria-current={screen === n.id ? "page" : undefined} onClick={() => {
              if (n.id === "doctor" || n.id === "history") { setProfileTab(n.id === "doctor" ? "mods" : "snapshots"); setScreen("profile"); return; }
              setProfileTab(undefined); setScreen(n.id);
            }}>
              <span className="ico" aria-hidden>{n.ico}</span>{n.label}
              {n.id === "profile" && instances.length > 0 && <span className="count">{instances.length}</span>}
            </button>
          ))}
        </nav>
        <div className="grow" />
        <button className="acct" onClick={() => setScreen("accounts")}>
          <Avatar account={ms} />
          <div style={{ minWidth: 0 }}>
            <b>{ms ? nick(ms.name, streamer) : t("Нет аккаунта")}</b>
            {ms ? <span className={`badge ${ms.owns_java_edition ? "ms" : "err"}`}>Microsoft · {ms.owns_java_edition ? t("владение ✓") : t("игра не куплена")}</span> : <span className="badge">{t("войти")}</span>}
          </div>
        </button>
      </aside>

      <div className="main">
        {info.dev_launch && (
          <div className="devbar" role="status">{t("Сборка разработчика (dev-launch): разрешён запуск без входа для отладки на собственной копии игры. В релизе этого режима нет.")}</div>
        )}
        {release && (
          <div className="devbar" role="status">
            {t("Вышла новая версия Quarry {0}.", release.version)}{" "}
            <button className="btn link small" onClick={() => api.openLink(release.url).catch(() => {})}>{t("Открыть страницу загрузки")}</button>
            <button className="btn link small" onClick={() => setRelease(null)}>{t("Позже")}</button>
          </div>
        )}
        <header className="top">
          <button className="searchbox" onClick={() => setPalette(true)} aria-label={t("Палитра команд (Ctrl+K)")}>
            <span>{t("Профиль, вкладка или действие…")}</span><kbd>{navigator.platform.includes("Mac") ? "⌘K" : "Ctrl+K"}</kbd>
          </button>
          <div className="sp" />
          {downloading ? (
            <div className="dl" aria-live="polite">
              ↓ {downloading.name}: {t(downloading.s.stageLabel)} · {downloading.s.progress!.files_done} {t("из")} {downloading.s.progress!.files_total} {t("файлов ·")} {mb(downloading.s.progress!.bytes_done)} {t("МБ")}
              <div className="bar"><i style={{ width: `${downloading.s.progress!.bytes_total ? (downloading.s.progress!.bytes_done / downloading.s.progress!.bytes_total) * 100 : 0}%` }} /></div>
            </div>
          ) : runningCount > 0 ? (
            <span className="dl">{t("Запущено профилей:")} {runningCount}</span>
          ) : <span className="dl" />}
          <div className="bellwrap">
            <button className="iconbtn" aria-label={t("Уведомления{0}", notes.unread ? t(": новых {0}", notes.unread) : "")} aria-expanded={bell} onClick={() => { setBell(!bell); markRead(); }}>
              🔔{notes.unread > 0 && <span className="dot">{notes.unread}</span>}
            </button>
            {bell && (
              <div className="bellpanel" role="region" aria-label={t("Уведомления")}>
                <div className="row" style={{ justifyContent: "space-between" }}><b>{t("Уведомления")}</b>{notes.notes.length > 0 && <button className="btn link small" onClick={clearNotes}>{t("Очистить")}</button>}</div>
                {notes.notes.length === 0 ? <p className="muted small">{t("Пока ничего нового.")}</p> : notes.notes.map((n) => (
                  <div key={n.id} className={`note ${n.kind}`}>
                    <b>{tc(n.title)}</b>{n.body && <span className="small">{tc(n.body)}</span>}
                    <span className="muted small">{n.profile ? `${instances.find((i) => i.id === n.profile)?.name ?? n.profile} · ` : ""}{relTime(Math.floor(n.at / 1000))}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
          <button className="iconbtn" aria-label={t("Переключить тему")} title={t("Тема")} onClick={() => changeTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark")}>◐</button>
        </header>

        <main className="content" id="main" tabIndex={-1}>
          {loadError && <div className="alert err"><div className="h">{t("Не удалось прочитать профили")}</div><span>{loadError}</span></div>}
          {screen === "home" && (
            <Home info={info} instances={instances} modsCount={modsCount} broken={broken} accounts={accounts} states={states} selected={selected}
              onSelect={setSelectedId} onOpen={(id, tab) => open(id, tab)} onCreate={() => setCreating(true)} onImport={() => setImporting(true)}
              onAccounts={() => setScreen("accounts")} onLogs={(id) => { setSelectedId(id); setScreen("logs"); }} />
          )}
          {screen === "profile" && (selected ? (
            <>
              {instances.length > 1 && (
                <div className="row" style={{ marginBottom: 12 }}>
                  <select className="field" style={{ maxWidth: 280 }} value={selected.id} onChange={(e) => setSelectedId(e.target.value)} aria-label={t("Профиль")}>
                    {instances.map((i) => <option key={i.id} value={i.id}>{i.name}</option>)}
                  </select>
                  <button className="btn" onClick={() => setCreating(true)}>{t("+ Новый профиль")}</button>
                </div>
              )}
              <InstancePage key={selected.id} inst={selected} instances={instances} state={states.get(selected.id)} accounts={accounts} initialTab={profileTab}
                onCloned={async (i) => { await refresh(); open(i.id, "profile"); }}
                onChanged={(i) => setInstances((list) => list.map((x) => (x.id === i.id ? i : x)))}
                onTrashed={() => { refresh(); setScreen("home"); }} />
            </>
          ) : (
            <div className="card empty"><p>{t("Профилей нет.")}</p><button className="btn pri" onClick={() => setCreating(true)}>{t("+ Новый профиль")}</button></div>
          ))}
          {screen === "logs" && <Logs instances={instances} states={states} id={selectedId} onPick={setSelectedId} />}
          {screen === "accounts" && <Accounts info={info} accounts={accounts} defaultKey={defaultKey} onChanged={refreshAccounts} />}
          {screen === "settings" && <Settings info={info} theme={theme} onTheme={changeTheme} onAppearance={applyAppearance} />}
        </main>

        <footer className="foot">
          <span>{t("Данные:")} {path(info.data_dir, streamer)}</span>
          <span>Quarry {info.version}{info.dev_launch ? " · dev-launch" : ""}</span>
        </footer>
      </div>

      {firstRun && (
        <FirstRun onDone={(next) => {
          setFirstRun(false);
          if (next === "accounts") setScreen("accounts");
          else if (next === "create") setCreating(true);
          else if (next === "import") setImporting(true);
        }} />
      )}
      {palette && <Palette commands={commands} onClose={() => setPalette(false)} />}
      {importing && (
        <ImportDialog onClose={() => setImporting(false)} onDone={(i) => { setImporting(false); refresh().then(() => open(i.id)); }} />
      )}
      {creating && (
        <CreateInstance onClose={() => setCreating(false)} onCreated={(i) => { setCreating(false); refresh().then(() => { setSelectedId(i.id); setScreen("home"); }); }} />
      )}
    </div>
  );
}

export default function App() {
  return (
    <DialogProvider>
      <Shell />
    </DialogProvider>
  );
}
