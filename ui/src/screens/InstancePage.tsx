import { useCallback, useEffect, useState } from "react";
import { api, errText, loaderName, mods as modsApi, modsLabel, sizeText, snapshots, type DoctorReport, type Hooks, type JournalEntry, sourceName, type AccountRef, type Instance, type LaunchOverrides, type Resolved, type Source } from "../api";
import { isActive, type LaunchState } from "../launches";
import { useDialog } from "../ui/Dialog";
import { LaunchProgress, PlayButton, ProfileIcon, StatusBadge } from "../ui/common";
import { CrashCard } from "../ui/CrashCard";
import { ModsTab } from "./ModsTab";
import { SnapshotsTab } from "./SnapshotsTab";
import { ProfileTab } from "./ProfileTab";
import { ServersTab } from "./ServersTab";
import { ResourcesTab } from "./ResourcesTab";
import { ConfigsTab } from "./ConfigsTab";
import { OptionsTab } from "./OptionsTab";
import { DepsTab } from "./DepsTab";
import { Overview } from "./Overview";
import { nick, useStreamer } from "../streamer";
import { t, tc } from "../i18n";

/** Адрес места в профиле: так на него ссылаются разбор краша и уведомления. */
export type Tab = "overview" | "mods" | "deps" | "resources" | "configs" | "options" | "launch" | "snapshots" | "servers" | "profile";
type Main = "overview" | "mods" | "rollback" | "game";
type Sub = "mods" | "resources" | "deps" | "launch" | "options" | "servers" | "profile";

/** Вариант F: 4 вкладки вместо 10, остальное — подвкладки (docs/INTERFACE_F.md). */
const MAIN: [Main, string][] = [["overview", t("Обзор")], ["mods", t("Моды")], ["rollback", t("Откат")], ["game", t("Игра")]];
const SUBS: Record<"mods" | "game", [Sub, string][]> = {
  mods: [["mods", t("Моды")], ["resources", t("Ресурсы и конфиги")], ["deps", t("Зависимости")]],
  game: [["launch", t("Запуск и память")], ["options", t("Настройки игры")], ["servers", t("Серверы")], ["profile", t("О профиле")]],
};
function place(tab: Tab): [Main, Sub | undefined] {
  switch (tab) {
    case "overview": return ["overview", undefined];
    case "mods": case "deps": return ["mods", tab];
    case "resources": case "configs": return ["mods", "resources"];
    case "snapshots": return ["rollback", undefined];
    default: return ["game", tab];
  }
}

/** Переключатель с ролью tablist: стрелки, Home и End, как у вкладок. */
function TabStrip<K extends string>({ items, value, onPick, className, label }: { items: [K, React.ReactNode][]; value: K; onPick: (k: K) => void; className: string; label: string }) {
  return (
    <div className={className} role="tablist" aria-label={label}>
      {items.map(([id, text], i) => (
        <button key={id} role="tab" aria-selected={value === id} tabIndex={value === id ? 0 : -1} className={value === id ? "on" : ""} onClick={() => onPick(id)}
          onKeyDown={(e) => {
            const next = e.key === "ArrowRight" ? (i + 1) % items.length : e.key === "ArrowLeft" ? (i - 1 + items.length) % items.length : e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : -1;
            if (next < 0) return;
            e.preventDefault();
            onPick(items[next][0]);
            (e.currentTarget.parentElement?.children[next] as HTMLElement | undefined)?.focus();
          }}>{text}</button>
      ))}
    </div>
  );
}

/** Клавиша не должна срабатывать, пока фокус в поле, списке или диалоге. */
export const typing = (e: KeyboardEvent) => {
  const el = e.target as HTMLElement | null;
  return !!el && (/^(INPUT|SELECT|TEXTAREA)$/.test(el.tagName) || el.isContentEditable || !!document.querySelector("[role=dialog]"));
};

function SourceTag({ source, onReset }: { source: Source; onReset?: () => void }) {
  return (
    <div className="srcbox">
      <span className={`badge src ${source === "profile" ? "ok" : ""}`}>{sourceName[source]}</span>
      {source === "profile" && onReset && <button className="reset" onClick={onReset}>{t("сбросить")}</button>}
    </div>
  );
}

function LaunchTab({ inst, accounts, onChanged }: { inst: Instance; accounts: AccountRef[]; onChanged: (i: Instance) => void }) {
  const dialog = useDialog();
  const streamer = useStreamer();
  const [data, setData] = useState<Awaited<ReturnType<typeof api.launchSettings>> | null>(null);
  const [draft, setDraft] = useState<LaunchOverrides>(inst.settings);
  const [args, setArgs] = useState((inst.settings.jvm_args ?? []).join(" "));
  const [gameArgs, setGameArgs] = useState((inst.settings.game_args ?? []).join(" "));
  const [envText, setEnvText] = useState(Object.entries(inst.settings.env ?? {}).map(([k, v]) => `${k}=${v}`).join("\n"));
  const [advice, setAdvice] = useState<string[]>([]);
  const [hooks, setHooks] = useState<Hooks>(inst.hooks ?? {});
  const saveHooks = async () => {
    const clean: Hooks = Object.fromEntries(Object.entries(hooks).filter(([k, v]) => k !== "approved" && typeof v === "string" && v.trim()).map(([k, v]) => [k, (v as string).trim()]));
    const lines = Object.entries(clean).map(([k, v]) => `${{ pre: t("до запуска"), post_exit: t("после выхода"), post_crash: t("после краша"), wrapper: t("обёртка") }[k]}: ${v}`);
    if (lines.length && !(await dialog.confirm({
      title: t("Разрешить эти команды?"),
      body: <><p style={{ marginTop: 0 }}>{t("Они будут выполняться на этом компьютере от вашего имени при каждом запуске профиля. Добавляйте только то, что понимаете.")}</p><pre className="cfgdiff" style={{ userSelect: "text" }}>{lines.join("\n")}</pre></>,
      confirm: t("Разрешить"),
      danger: true,
    }))) return;
    try { onChanged(await api.updateInstance(inst.id, { hooks: clean })); } catch (e) { await dialog.alert(t("Не сохранено"), errText(e)); }
  };
  const [saving, setSaving] = useState(false);
  const [preview, setPreview] = useState("");

  const load = useCallback(() => {
    api.launchSettings(inst.id).then((d) => {
      setData(d); setDraft(d.profile); setArgs((d.profile.jvm_args ?? []).join(" "));
      setGameArgs((d.profile.game_args ?? []).join(" "));
      setEnvText(Object.entries(d.profile.env ?? {}).map(([k, v]) => `${k}=${v}`).join("\n"));
    }).catch(() => setData(null));
  }, [inst.id]);
  useEffect(load, [load]);
  const maxForAdvice = draft.max_memory_mb ?? inst.pack_settings.max_memory_mb ?? data?.global.max_memory_mb ?? 4096;
  useEffect(() => {
    const timer = setTimeout(() => api.memoryAdvice(maxForAdvice, inst.pack_settings.max_memory_mb).then(setAdvice).catch(() => setAdvice([])), 250);
    return () => clearTimeout(timer);
  }, [maxForAdvice, inst.pack_settings.max_memory_mb]);

  if (!data) return <p className="muted"><span className="spinner" /> {t("Загружаем настройки…")}</p>;

  // Итог с учётом несохранённых правок: профиль → сборка → глобальные → умолчание.
  const pick = <K extends keyof LaunchOverrides>(key: K, fallback: NonNullable<LaunchOverrides[K]>): Resolved<NonNullable<LaunchOverrides[K]>> => {
    const levels: [LaunchOverrides[K], Source][] = [[draft[key], "profile"], [inst.pack_settings[key], "pack"], [data.global[key], "global"]];
    const hit = levels.find(([v]) => v !== undefined && v !== null);
    return hit ? { value: hit[0] as NonNullable<LaunchOverrides[K]>, source: hit[1] } : { value: fallback, source: "default" };
  };
  const max = pick("max_memory_mb", 4096);
  const total = data.total_memory_mb;
  const ceiling = Math.max(1024, Math.min(total ? total - 1024 : 16384, 32768));

  const save = async () => {
    setSaving(true);
    try {
      const parsed = args.trim() ? args.trim().split(/\s+/) : undefined;
      const env: Record<string, string> = {};
      for (const line of envText.split("\n").map((l) => l.trim()).filter(Boolean)) {
        const i = line.indexOf("=");
        if (i < 1) throw new Error(t("Строка «{0}»: нужно ИМЯ=значение", line));
        env[line.slice(0, i).trim()] = line.slice(i + 1);
      }
      const updated = await api.updateInstance(inst.id, { settings: {
        ...draft, jvm_args: parsed,
        game_args: gameArgs.trim() ? gameArgs.trim().split(/\s+/) : undefined,
        env: Object.keys(env).length ? env : undefined,
      } });
      onChanged(updated);
      load();
    } catch (e) {
      await dialog.alert(t("Настройки не сохранены"), errText(e));
    } finally {
      setSaving(false);
    }
  };

  const ms = accounts.filter((a) => a.provider === "microsoft");
  const ely = accounts.filter((a) => a.provider === "elyby");
  const javaEff = pick("java_path", "");
  return (
    <>
      <div className="alert info">
        <div className="h">{t("Откуда берутся значения")}</div>
        <span className="small">{t("Значение берётся с ближайшего уровня, где оно задано: профиль → сборка → глобальные. Метка показывает, откуда пришло текущее значение; «сбросить» возвращает наследуемое.")}</span>
      </div>
      <div className="card">
        <div className="setting">
          <div className="lbl">{t("Память")}<span className="hint">{(max.value / 1024).toFixed(1).replace(".0", "")} {t("ГБ")}{total ? t(" из {0} ГБ ОЗУ", Math.round(total / 1024)) : ""}</span></div>
          <div>
            <input type="range" min={1024} max={ceiling} step={512} value={Math.min(max.value, ceiling)} aria-label={t("Память, МБ")}
              onChange={(e) => setDraft({ ...draft, max_memory_mb: Number(e.target.value), min_memory_mb: undefined })} />
            <span className="muted small">{t("Xms = Xmx: память не перевыделяется посреди игры.")}{inst.pack_settings.max_memory_mb ? t(" Сборка рекомендует {0} МБ.", inst.pack_settings.max_memory_mb) : ""}</span>
            {advice.map((a) => <span key={a} className="small" style={{ color: "var(--warn)" }}>⚠ {tc(a)}</span>)}
          </div>
          <SourceTag source={max.source} onReset={() => setDraft({ ...draft, max_memory_mb: undefined, min_memory_mb: undefined })} />
        </div>
        <div className="setting">
          <div className="lbl">Java<span className="hint">{t("по умолчанию — по javaVersion из JSON версии")}</span></div>
          <input className="field mono" placeholder={t("Авто: лаунчер подберёт или скачает нужную")} value={draft.java_path ?? ""}
            onChange={(e) => setDraft({ ...draft, java_path: e.target.value || undefined })} />
          <SourceTag source={javaEff.source} onReset={() => setDraft({ ...draft, java_path: undefined })} />
        </div>
        <div className="setting">
          <div className="lbl">{t("Аргументы JVM")}<span className="hint">{t("через пробел; -Xmx и -javaagent запрещены")}</span></div>
          <input className="field mono" placeholder={t("например -XX:+UseG1GC")} value={args} onChange={(e) => setArgs(e.target.value)} />
          <SourceTag source={args.trim() ? "profile" : inst.pack_settings.jvm_args ? "pack" : data.global.jvm_args ? "global" : "default"} onReset={() => setArgs("")} />
        </div>
        <div className="setting">
          <div className="lbl">{t("Размер окна игры")}<span className="hint">{t("пусто — как решит игра")}</span></div>
          <div className="row" style={{ flexWrap: "nowrap" }}>
            <input className="field" type="number" min={320} aria-label={t("Ширина окна")} placeholder={t("ширина")} value={draft.resolution?.[0] ?? ""}
              onChange={(e) => setDraft({ ...draft, resolution: e.target.value ? [Number(e.target.value), draft.resolution?.[1] ?? 720] : undefined })} />
            <span className="muted">×</span>
            <input className="field" type="number" min={240} aria-label={t("Высота окна")} placeholder={t("высота")} value={draft.resolution?.[1] ?? ""}
              onChange={(e) => setDraft({ ...draft, resolution: e.target.value ? [draft.resolution?.[0] ?? 1280, Number(e.target.value)] : undefined })} />
          </div>
          <SourceTag source={draft.resolution ? "profile" : inst.pack_settings.resolution ? "pack" : data.global.resolution ? "global" : "default"} onReset={() => setDraft({ ...draft, resolution: undefined })} />
        </div>
        <div className="setting">
          <div className="lbl">{t("Аргументы игры")}<span className="hint">{t("через пробел, в конце команды; вход и папки задаёт лаунчер")}</span></div>
          <input className="field mono" placeholder={t("например --fullscreen")} value={gameArgs} onChange={(e) => setGameArgs(e.target.value)} />
          <SourceTag source={gameArgs.trim() ? "profile" : inst.pack_settings.game_args ? "pack" : data.global.game_args ? "global" : "default"} onReset={() => setGameArgs("")} />
        </div>
        <div className="setting">
          <div className="lbl">{t("Переменные окружения")}<span className="hint">{t("ИМЯ=значение, по одной в строке; LD_*, DYLD_* и JAVA_TOOL_OPTIONS запрещены")}</span></div>
          <textarea className="field mono" rows={2} placeholder="MESA_GL_VERSION_OVERRIDE=4.5" value={envText} onChange={(e) => setEnvText(e.target.value)} />
          <SourceTag source={envText.trim() ? "profile" : inst.pack_settings.env ? "pack" : data.global.env ? "global" : "default"} onReset={() => setEnvText("")} />
        </div>
        <div className="setting">
          <div className="lbl">{t("Вход")}<span className="hint">{t("Ely.by — только при Microsoft-аккаунте с купленной игрой")}</span></div>
          <select className="field" value={inst.auth_mode} onChange={async (e) => {
            try { onChanged(await api.updateInstance(inst.id, { authMode: e.target.value as Instance["auth_mode"] })); }
            catch (err) { await dialog.alert(t("Нельзя переключить вход"), errText(err)); }
          }}>
            <option value="microsoft">{t("Microsoft — официальные серверы")}</option>
            <option value="ely_by" disabled={!ms.some((a) => a.owns_java_edition) || ely.length === 0}>{t("Ely.by — только совместимые серверы")}</option>
          </select>
          <span />
        </div>
        {(inst.auth_mode === "microsoft" ? ms : ely).length > 1 && (
          <div className="setting">
            <div className="lbl">{t("Аккаунт")}<span className="hint">{t("для этого профиля")}</span></div>
            <select className="field" value={inst.account ?? ""} onChange={async (e) => {
              try { onChanged(await api.updateInstance(inst.id, { account: e.target.value || null })); }
              catch (err) { await dialog.alert(t("Не удалось сменить аккаунт"), errText(err)); }
            }}>
              <option value="">{t("По умолчанию")}</option>
              {(inst.auth_mode === "microsoft" ? ms : ely).map((a) => <option key={a.uuid} value={`${a.provider}:${a.uuid}`}>{nick(a.name, streamer)}</option>)}
            </select>
            <SourceTag source={inst.account ? "profile" : "global"} />
          </div>
        )}
      </div>
      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn pri" onClick={save} disabled={saving}>{saving ? t("Сохраняем…") : t("Сохранить")}</button>
        <button className="btn" onClick={() => { setPreview("…"); api.previewCommand(inst.id).then(setPreview).catch((e) => setPreview(errText(e))); }}>{t("Показать итоговую команду")}</button>
        <span className="muted small">{t("Прирост FPS от флагов не обещаем: он зависит от модов, Java и железа.")}</span>
      </div>
      {preview && (
        <div style={{ marginTop: 12 }}>
          <span className="muted small">{t("Команда по сохранённым настройкам. Токен и UUID скрыты.")}</span>
          <div className="cmd" style={{ marginTop: 6 }}>{preview}</div>
        </div>
      )}
      <details className="card" style={{ marginTop: 16 }} open={!!inst.hooks && !inst.hooks.approved}>
        <summary>{t("Команды до и после запуска (для опытных)")}</summary>
        {inst.hooks && Object.keys(inst.hooks).some((k) => k !== "approved") && !inst.hooks.approved && (
          <div className="alert warn"><span>{t("Команды изменены вне лаунчера и не выполняются, пока вы их не подтвердите.")}</span></div>
        )}
        {([["pre", t("Перед запуском"), "./backup.sh"], ["post_exit", t("После выхода"), ""], ["post_crash", t("После краша"), ""], ["wrapper", t("Обёртка (только Linux)"), "gamemoderun"]] as const).map(([k, label, ph]) => (
          <div className="setting" key={k}>
            <div className="lbl">{label}</div>
            <input className="field mono" placeholder={ph} value={hooks[k] ?? ""} onChange={(e) => setHooks({ ...hooks, [k]: e.target.value })} />
            <span />
          </div>
        ))}
        <p className="muted small">{t("Команда выполняется в папке игры; доступны переменные QUARRY_PROFILE и QUARRY_GAME_DIR. Если команда перед запуском завершится с ошибкой, игра не запустится.")}</p>
        <button className="btn" onClick={saveHooks}>{t("Сохранить команды…")}</button>
      </details>
    </>
  );
}

export function InstancePage({ inst, mods: modCount, instances, state, accounts, initialTab, onChanged, onTrashed, onCloned, onLogs }: {
  inst: Instance;
  mods?: number;
  instances: Instance[];
  onCloned: (i: Instance) => void;
  state?: LaunchState;
  accounts: AccountRef[];
  initialTab?: Tab;
  onChanged: (i: Instance) => void;
  onTrashed: () => void;
  onLogs: () => void;
}) {
  const dialog = useDialog();
  const [[main, sub], setPlace] = useState<[Main, Sub | undefined]>(() => place(initialTab ?? "overview"));
  const go = (tab: Tab) => setPlace(place(tab));
  const subOf = (m: "mods" | "game") => (main === m && sub ? sub : SUBS[m][0][0]);
  const [reload, setReload] = useState(0);
  const [disk, setDisk] = useState<number | null>(null);
  const [more, setMore] = useState(false);
  const [profileAction, setProfileAction] = useState<{ kind: "clone" | "export"; n: number } | null>(null);
  const ask = (kind: "clone" | "export") => { setMore(false); setPlace(["game", "profile"]); setProfileAction({ kind, n: Date.now() }); };
  useEffect(() => { api.diskUsage(inst.id).then((u) => setDisk(u.total)).catch(() => setDisk(null)); }, [inst.id, reload]);
  useEffect(() => { if (initialTab) setPlace(place(initialTab)); }, [initialTab]);
  // Mod Doctor для «Обзора», счётчика на «Моды» и главной кнопки; перечитываем после смены вкладки и запуска.
  const [doctor, setDoctor] = useState<{ report: DoctorReport; journal: JournalEntry[] } | null>(null);
  useEffect(() => { modsApi.view(inst.id).then(setDoctor).catch(() => setDoctor(null)); }, [inst.id, main, reload, state?.phase]);
  const blocking = doctor?.report.findings.filter((f) => f.severity === "blocking") ?? [];
  // «Починить и играть» — только если каждое блокирующее замечание чинится выключением мода (как в Mod Doctor).
  const fix = blocking.length > 0 && blocking.every((f) => f.fixes.some((x) => x.kind !== "install")) ? async () => {
    await snapshots.create(inst.id, t("Перед «Починить и играть»"));
    for (const f of blocking) {
      const x = f.fixes.find((k) => k.kind !== "install");
      const off = x?.kind === "disable" ? [x.file_name] : x?.kind === "keep_one" ? x.file_names.slice(1) : [];
      for (const file of off) await modsApi.toggle(inst.id, file, false);
    }
  } : undefined;
  const issues = doctor?.report.findings.length ?? 0;
  const tabs: [Main, React.ReactNode][] = MAIN.map(([m, l]) => [m, m === "mods" && issues > 0
    ? <>{l} <span className={`count ${blocking.length ? "err" : "warn"}`} aria-label={t("замечаний: {0}", issues)}>{issues}</span></> : l]);
  // Меню «Ещё» закрывается кликом мимо него.
  useEffect(() => {
    if (!more) return;
    const close = (e: MouseEvent) => { if (!(e.target as HTMLElement).closest(".menuwrap")) setMore(false); };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [more]);
  // Запуск закончился ошибкой или крашем — разбор на «Обзоре».
  const failed = state?.phase === "failed" || (state?.phase === "done" && !!state.exit?.analysis);
  useEffect(() => { if (failed) setPlace(["overview", undefined]); }, [failed]);
  // 1–4 — вкладки (подсказка в нижней панели).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typing(e) || e.ctrlKey || e.metaKey || e.altKey) return;
      const n = Number(e.key);
      if (n >= 1 && n <= MAIN.length) { e.preventDefault(); setPlace([MAIN[n - 1][0], undefined]); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const trash = async () => {
    const ok = await dialog.confirm({
      title: t("Убрать «{0}» в корзину?", inst.name),
      body: <>{t("Профиль вместе с мирами, модами и конфигами переместится в папку")} <span className="mono">trash/</span> {t("внутри папки данных Quarry. Ничего не удаляется: чтобы вернуть, перенесите папку обратно в")} <span className="mono">instances/</span>.</>,
      confirm: t("Убрать в корзину"),
      danger: true,
    });
    if (!ok) return;
    try {
      await api.trashInstance(inst.id);
      onTrashed();
    } catch (e) {
      await dialog.alert(t("Не получилось"), errText(e));
    }
  };

  return (
    <>
      <div className="phead">
        <ProfileIcon id={inst.id} color={inst.color} size={44} className="pi phead-ico" />
        <div className="phead-text">
          <h2>{inst.name}</h2>
          <p className="sub">Minecraft {inst.game_version}, {loaderName[inst.loader]}{inst.loader_version ? ` ${inst.loader_version}` : ""}{modCount !== undefined && inst.loader !== "vanilla" ? `, ${modsLabel(modCount)}` : ""}{disk !== null ? `, ${t("{0} на диске", sizeText(disk))}` : ""} · <StatusBadge inst={inst} state={state} /></p>
        </div>
        <div className="phead-acts">
          <button className="btn square" aria-label={t("Папка")} title={t("Папка")} onClick={() => api.openFolder(inst.id).catch((e) => dialog.alert(t("Не удалось открыть папку"), errText(e)))}>⌂</button>
          <div className="menuwrap">
            <button className="btn square" aria-label={t("Ещё")} aria-haspopup="menu" aria-expanded={more} onClick={() => setMore(!more)}>⋯</button>
            {more && (
              <div className="menu" role="menu" onKeyDown={(e) => { if (e.key === "Escape") setMore(false); }}>
                <button role="menuitem" onClick={() => ask("clone")}>{t("Сделать копию профиля…")}</button>
                <button role="menuitem" onClick={() => ask("export")}>{t("Экспорт в .mrpack…")}</button>
                <button role="menuitem" className="danger" disabled={isActive(state)} onClick={() => { setMore(false); trash(); }}>{t("В корзину")}</button>
              </div>
            )}
          </div>
          <PlayButton inst={inst} state={state} compact fix={fix} />
        </div>
      </div>
      <div className="tabsrow">
        <TabStrip className="mtabs" label={t("Вкладки профиля")} items={tabs} value={main} onPick={(m) => setPlace([m, undefined])} />
      </div>
      {state && isActive(state) && <LaunchProgress state={state} />}
      {main === "overview" && (
        <div className="stack">
          {state?.phase === "failed" && state.error && <div className="alert err" style={{ whiteSpace: "pre-line" }}><div className="h">{t("Запуск не удался")}</div><span>{state.error}</span></div>}
          {state?.phase === "done" && state.exit?.analysis && <CrashCard inst={inst} a={state.exit.analysis} code={state.exit.code} onOpenTab={go} />}
          <Overview inst={inst} state={state} doctor={doctor} accounts={accounts} fix={fix} onGo={go} onLogs={onLogs} />
        </div>
      )}
      {main === "mods" && (
        <>
          <TabStrip className="seg subtabs" label={t("Разделы модов")} items={SUBS.mods} value={subOf("mods")} onPick={(s) => setPlace(["mods", s])} />
          {subOf("mods") === "mods" && <ModsTab key={`${inst.id}-${reload}`} inst={inst} running={isActive(state)} />}
          {subOf("mods") === "resources" && (
            <div className="split2">
              <ResourcesTab key={`r-${inst.id}`} inst={inst} running={isActive(state)} />
              <ConfigsTab key={`c-${inst.id}`} inst={inst} running={isActive(state)} />
            </div>
          )}
          {subOf("mods") === "deps" && <DepsTab key={inst.id} inst={inst} />}
        </>
      )}
      {main === "rollback" && <SnapshotsTab inst={inst} running={isActive(state)} journal={doctor?.journal ?? []} onRestored={() => setReload((n) => n + 1)} />}
      {main === "game" && (
        <>
          <TabStrip className="seg subtabs" label={t("Разделы игры")} items={SUBS.game} value={subOf("game")} onPick={(s) => setPlace(["game", s])} />
          {subOf("game") === "launch" && <LaunchTab key={inst.id} inst={inst} accounts={accounts} onChanged={onChanged} />}
          {subOf("game") === "options" && <OptionsTab key={inst.id} inst={inst} running={isActive(state)} />}
          {subOf("game") === "servers" && <ServersTab key={inst.id} inst={inst} running={isActive(state)} />}
          {subOf("game") === "profile" && <ProfileTab key={inst.id} inst={inst} instances={instances} running={isActive(state)} onChanged={onChanged} onCloned={onCloned} action={profileAction} />}
        </>
      )}
    </>
  );
}
