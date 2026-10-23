import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, mb, statusOf, type AccountRef, type AppInfo, type GlobalSettings, type Instance } from "./api";
import { isActive, launches, useLaunches } from "./launches";
import { Accounts } from "./screens/Accounts";
import { CreateInstance } from "./screens/CreateInstance";
import { ImportDialog } from "./screens/Import";
import { InstancePage, typing, type Tab } from "./screens/InstancePage";
import { Logs } from "./screens/Logs";
import { Settings } from "./screens/Settings";
import { FirstRun } from "./screens/FirstRun";
import { Avatar } from "./ui/Avatar";
import { ProfileIcon } from "./ui/common";
import { DialogProvider } from "./ui/Dialog";
import { nick, path, setStreamer, useStreamer } from "./streamer";
import { markRead, pushNote, useNotes } from "./notify";
import { src, syncLang, t } from "./i18n";

/** Профиль с вкладками или один из общих экранов, открытых с рельсы. */
type Screen = "profile" | "logs" | "accounts" | "settings" | "new";

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
  const [screen, setScreen] = useState<Screen>("profile");
  const [profileTab, setProfileTab] = useState<Tab | undefined>();
  const [theme, setTheme] = useState("dark");
  const [loadError, setLoadError] = useState("");
  const [firstRun, setFirstRun] = useState(false);
  const [release, setRelease] = useState<{ version: string; url: string } | null>(null);
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
      pushNote({ kind: "info", title: src("Вышла новая версия Lime {0}", r.version) });
    }).catch(() => {});
    const off = listen("launch-finished", () => refresh());
    // Esc — назад к профилю. Enter (играть) — ниже, ему нужен выбранный профиль.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !typing(e)) setScreen("profile");
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

  // Enter — главная кнопка в шапке профиля («Играть» или «Починить и играть»), только когда фокус не на кнопке, поле или в диалоге.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const free = document.activeElement === document.body || document.activeElement?.id === "main";
      if (e.key !== "Enter" || typing(e) || !free || screen !== "profile" || !selectedId || isActive(states.get(selectedId))) return;
      e.preventDefault();
      document.querySelector<HTMLButtonElement>(".phead .play .go")?.click();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [screen, selectedId, states]);

  if (!info) return <div className="content muted">{t("Загрузка…")}</div>;
  const open = (id: string, tab?: Tab) => { setSelectedId(id); setProfileTab(tab); setScreen("profile"); };
  // Рельса: сначала избранные, потом по последнему запуску; архивные — в самом конце, приглушённые.
  const rail = [...instances].sort((a, b) =>
    Number(!!a.archived) - Number(!!b.archived) || Number(!!b.favorite) - Number(!!a.favorite) || (b.last_launch?.at ?? 0) - (a.last_launch?.at ?? 0));
  const status = (i: Instance) => {
    const st = states.get(i.id);
    const s = statusOf(i, isActive(st) ? st!.phase : undefined);
    return { ...s, cls: i.last_launch || isActive(st) ? s.cls || "idle" : "idle" };
  };
  const selStatus = selected ? status(selected) : null;

  return (
    <div className="app">
      <a href="#main" className="skip">{t("Перейти к содержимому")}</a>
      <nav className="rail" aria-label={t("Профили и разделы")}>
        <div className="rail-logo" aria-hidden title={`Lime ${info.version}`}><i /></div>
        <div className="rail-list">
          {rail.map((i, n) => {
            const s = status(i);
            const prev = rail[n - 1];
            const sep = n > 0 && ((prev.favorite && !i.favorite) || (!prev.archived && i.archived));
            return (
              <div key={i.id} className={sep ? "rail-sep" : undefined}>
                <button className={`tile ${screen === "profile" && i.id === selectedId ? "on" : ""} ${i.archived ? "archived" : ""}`}
                  aria-label={`${i.name}, ${i.archived ? `${t("в архиве")}, ` : ""}${s.text}`} title={`${i.name} · ${i.archived ? `${t("в архиве")} · ` : ""}${s.text}`}
                  aria-current={screen === "profile" && i.id === selectedId ? "page" : undefined} onClick={() => open(i.id)}>
                  <ProfileIcon id={i.id} color={i.color} size={34} />
                  <span className={`sdot ${s.cls}`} aria-hidden />
                </button>
              </div>
            );
          })}
          <button className={`tile add ${screen === "new" ? "on" : ""}`} aria-label={t("Новый профиль")} title={t("Новый профиль")} onClick={() => setScreen("new")}>+</button>
        </div>
        <div className="rail-tools">
          <button className={`rail-btn ${screen === "logs" ? "on" : ""}`} aria-label={t("Логи и краши{0}", notes.unread ? t(": новых {0}", notes.unread) : "")} title={t("Логи и краши")}
            onClick={() => { markRead(); setScreen("logs"); }}>
            🔔{notes.unread > 0 && <span className="adot" aria-hidden />}
          </button>
          <button className={`rail-btn ${screen === "settings" ? "on" : ""}`} aria-label={t("Настройки")} title={t("Настройки")} onClick={() => setScreen("settings")}>⚙</button>
          <button className={`rail-btn avatar ${screen === "accounts" ? "on" : ""}`} aria-label={t("Аккаунты и скины: {0}", ms ? nick(ms.name, streamer) : t("нет аккаунта"))} title={t("Аккаунты и скины")} onClick={() => setScreen("accounts")}>
            <Avatar account={ms} />
          </button>
        </div>
      </nav>

      <div className="main">
        {info.dev_launch && (
          <div className="devbar" role="status">{t("Сборка разработчика (dev-launch): разрешён запуск без входа для отладки на собственной копии игры. В релизе этого режима нет.")}</div>
        )}
        {release && (
          <div className="devbar" role="status">
            {t("Вышла новая версия Lime {0}.", release.version)}{" "}
            <button className="btn link small" onClick={() => api.openLink(release.url).catch(() => {})}>{t("Открыть страницу загрузки")}</button>
            <button className="btn link small" onClick={() => setRelease(null)}>{t("Позже")}</button>
          </div>
        )}

        <main className="content" id="main" tabIndex={-1}>
          {loadError && <div className="alert err"><div className="h">{t("Не удалось прочитать профили")}</div><span>{loadError}</span></div>}
          {broken.length > 0 && (
            <div className="alert warn"><div className="h">{t("Не удалось прочитать профили:")} {broken.length}</div>
              {broken.map((b) => <span key={b.dir} className="small mono">{path(b.dir, streamer)}</span>)}
            </div>
          )}
          {screen === "profile" && (selected ? (
            <InstancePage key={selected.id} inst={selected} mods={modsCount[selected.id]} instances={instances} state={states.get(selected.id)} accounts={accounts} initialTab={profileTab}
              onCloned={async (i) => { await refresh(); open(i.id, "profile"); }}
              onChanged={(i) => setInstances((list) => list.map((x) => (x.id === i.id ? i : x)))}
              onTrashed={() => { refresh(); setProfileTab(undefined); }} onLogs={() => setScreen("logs")} />
          ) : (
            <div className="card empty">
              <p>{t("Профилей нет.")}</p>
              <div className="row" style={{ justifyContent: "center" }}>
                <button className="btn pri" onClick={() => setScreen("new")}>{t("+ Новый профиль")}</button>
              </div>
            </div>
          ))}
          {screen === "logs" && <Logs instances={instances} states={states} id={selectedId} onPick={setSelectedId} />}
          {screen === "accounts" && <Accounts info={info} accounts={accounts} defaultKey={defaultKey} onChanged={refreshAccounts} />}
          {screen === "new" && (
            <>
              <h2>{t("Новый профиль")}</h2>
              <p className="sub">{t("Создать с нуля или импортировать готовую сборку.")}</p>
              <div className="split2">
                <CreateInstance embedded onClose={() => setScreen("profile")} onCreated={(i) => refresh().then(() => open(i.id))} />
                <ImportDialog embedded onClose={() => setScreen("profile")} onDone={(i) => refresh().then(() => open(i.id))} />
              </div>
            </>
          )}
          {screen === "settings" && <Settings info={info} theme={theme} onTheme={changeTheme} onAppearance={applyAppearance} />}
        </main>

        <footer className="bottombar">
          <span className="keys">
            {screen === "profile" && selected && <><kbd>1</kbd><kbd>2</kbd><kbd>3</kbd><kbd>4</kbd> {t("вкладки")}</>}
            {screen !== "profile" && <><kbd>Esc</kbd> {t("назад")}</>}
            {selected && <><kbd>Enter</kbd> {t("играть")}</>}
          </span>
          <span className="status" aria-live="polite">
            {downloading ? (
              <>↓ {downloading.name}: {t(downloading.s.stageLabel)} · {downloading.s.progress!.files_done}/{downloading.s.progress!.files_total} · {mb(downloading.s.progress!.bytes_done)} {t("МБ")}</>
            ) : runningCount > 0 ? t("Запущено профилей: {0}", runningCount)
              : selected && selStatus ? t("Профиль «{0}»: {1}", selected.name, selStatus.text) : ""}
          </span>
        </footer>
      </div>

      {firstRun && (
        <FirstRun onDone={(next) => {
          setFirstRun(false);
          if (next === "accounts") setScreen("accounts");
          else if (next === "create" || next === "import") setScreen("new");
        }} />
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
