import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, mb, type AccountRef, type AppInfo, type Instance } from "./api";
import { isActive, launches, useLaunches } from "./launches";
import { Accounts } from "./screens/Accounts";
import { CreateInstance } from "./screens/CreateInstance";
import { Home } from "./screens/Home";
import { ImportDialog } from "./screens/Import";
import { InstancePage, type Tab } from "./screens/InstancePage";
import { Logs } from "./screens/Logs";
import { Settings } from "./screens/Settings";
import { Avatar } from "./ui/Avatar";
import { DialogProvider } from "./ui/Dialog";

type Screen = "home" | "profile" | "doctor" | "history" | "logs" | "accounts" | "settings";

const NAV: { id: Screen; label: string; ico: string }[] = [
  { id: "home", label: "Главная", ico: "⌂" },
  { id: "profile", label: "Профили", ico: "▦" },
  { id: "doctor", label: "Mod Doctor", ico: "⚙" },
  { id: "history", label: "Снимки и откат", ico: "↺" },
  { id: "logs", label: "Логи и краши", ico: "≡" },
  { id: "accounts", label: "Аккаунты", ico: "☺" },
  { id: "settings", label: "Настройки", ico: "✎" },
];

function applyTheme(theme: string) {
  const dark = theme === "system" ? window.matchMedia("(prefers-color-scheme: dark)").matches : theme !== "light";
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

function Shell() {
  const states = useLaunches();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [instances, setInstances] = useState<Instance[]>([]);
  const [broken, setBroken] = useState<{ dir: string; reason: string }[]>([]);
  const [accounts, setAccounts] = useState<AccountRef[]>([]);
  const [selectedId, setSelectedId] = useState<string | undefined>();
  const [screen, setScreen] = useState<Screen>("home");
  const [profileTab, setProfileTab] = useState<Tab | undefined>();
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [theme, setTheme] = useState("dark");
  const [loadError, setLoadError] = useState("");

  const refresh = useCallback(async () => {
    try {
      const r = await api.listInstances();
      setInstances(r.instances);
      setBroken(r.broken);
      setSelectedId((cur) => (cur && r.instances.some((i) => i.id === cur) ? cur : r.instances[0]?.id));
      setLoadError("");
    } catch (e) {
      setLoadError(String(e));
    }
  }, []);
  const [defaultKey, setDefaultKey] = useState<string | undefined>();
  const refreshAccounts = useCallback(() => {
    api.listAccounts().then(setAccounts).catch(() => setAccounts([]));
    api.globalSettings().then((g) => setDefaultKey(g.default_account)).catch(() => {});
  }, []);

  useEffect(() => {
    launches.start();
    api.appInfo().then(setInfo);
    api.globalSettings().then((g) => { setTheme(g.theme); applyTheme(g.theme); }).catch(() => applyTheme("dark"));
    refresh();
    refreshAccounts();
    const off = listen("launch-finished", () => refresh());
    return () => { off.then((f) => f()); };
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

  if (!info) return <div className="content muted">Загрузка…</div>;
  const open = (id: string, tab?: Tab) => { setSelectedId(id); setProfileTab(tab); setScreen("profile"); };

  return (
    <div className="app">
      <aside className="side">
        <div className="logo"><i /><div>Quarry<small>рабочее название · v{info.version}</small></div></div>
        <nav className="nav" aria-label="Разделы">
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
            <b>{ms?.name ?? "Нет аккаунта"}</b>
            {ms ? <span className={`badge ${ms.owns_java_edition ? "ms" : "err"}`}>Microsoft · {ms.owns_java_edition ? "владение ✓" : "игра не куплена"}</span> : <span className="badge">войти</span>}
          </div>
        </button>
      </aside>

      <div className="main">
        {info.dev_launch && (
          <div className="devbar" role="status">Сборка разработчика (dev-launch): разрешён запуск без входа для отладки на собственной копии игры. В релизе этого режима нет.</div>
        )}
        <header className="top">
          {downloading ? (
            <div className="dl" aria-live="polite">
              ↓ {downloading.name}: {downloading.s.stageLabel} · {downloading.s.progress!.files_done} из {downloading.s.progress!.files_total} файлов · {mb(downloading.s.progress!.bytes_done)} МБ
              <div className="bar"><i style={{ width: `${downloading.s.progress!.bytes_total ? (downloading.s.progress!.bytes_done / downloading.s.progress!.bytes_total) * 100 : 0}%` }} /></div>
            </div>
          ) : runningCount > 0 ? (
            <span className="dl">Запущено профилей: {runningCount}</span>
          ) : <span className="dl" />}
          <div className="sp" />
          <button className="iconbtn" aria-label="Переключить тему" title="Тема" onClick={() => changeTheme(document.documentElement.dataset.theme === "dark" ? "light" : "dark")}>◐</button>
        </header>

        <main className="content">
          {loadError && <div className="alert err"><div className="h">Не удалось прочитать профили</div><span>{loadError}</span></div>}
          {screen === "home" && (
            <Home info={info} instances={instances} broken={broken} accounts={accounts} states={states} selected={selected}
              onSelect={setSelectedId} onOpen={(id, tab) => open(id, tab)} onCreate={() => setCreating(true)} onImport={() => setImporting(true)}
              onAccounts={() => setScreen("accounts")} onLogs={(id) => { setSelectedId(id); setScreen("logs"); }} />
          )}
          {screen === "profile" && (selected ? (
            <>
              {instances.length > 1 && (
                <div className="row" style={{ marginBottom: 12 }}>
                  <select className="field" style={{ maxWidth: 280 }} value={selected.id} onChange={(e) => setSelectedId(e.target.value)} aria-label="Профиль">
                    {instances.map((i) => <option key={i.id} value={i.id}>{i.name}</option>)}
                  </select>
                  <button className="btn" onClick={() => setCreating(true)}>+ Новый профиль</button>
                </div>
              )}
              <InstancePage key={selected.id} inst={selected} state={states.get(selected.id)} accounts={accounts} initialTab={profileTab}
                onChanged={(i) => setInstances((list) => list.map((x) => (x.id === i.id ? i : x)))}
                onTrashed={() => { refresh(); setScreen("home"); }} />
            </>
          ) : (
            <div className="card empty"><p>Профилей нет.</p><button className="btn pri" onClick={() => setCreating(true)}>+ Новый профиль</button></div>
          ))}
          {screen === "logs" && <Logs instances={instances} states={states} id={selectedId} onPick={setSelectedId} />}
          {screen === "accounts" && <Accounts info={info} accounts={accounts} defaultKey={defaultKey} onChanged={refreshAccounts} />}
          {screen === "settings" && <Settings info={info} theme={theme} onTheme={changeTheme} />}
        </main>

        <footer className="foot">
          <span>Данные: {info.data_dir}</span>
          <span>Quarry {info.version}{info.dev_launch ? " · dev-launch" : ""}</span>
        </footer>
      </div>

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
