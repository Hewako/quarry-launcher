import { useCallback, useEffect, useState } from "react";
import { api, errText, loaderName, type Hooks, sourceName, type AccountRef, type Instance, type LaunchOverrides, type Resolved, type Source } from "../api";
import { isActive, type LaunchState } from "../launches";
import { useDialog } from "../ui/Dialog";
import { LaunchProgress, PlayButton, StatusBadge } from "../ui/common";
import { LogView } from "../ui/LogView";
import { CrashCard } from "../ui/CrashCard";
import { ModsTab } from "./ModsTab";
import { SnapshotsTab } from "./SnapshotsTab";
import { ProfileTab } from "./ProfileTab";
import { ServersTab } from "./ServersTab";
import { ResourcesTab } from "./ResourcesTab";
import { ConfigsTab } from "./ConfigsTab";
import { OptionsTab } from "./OptionsTab";
import { nick, useStreamer } from "../streamer";
import { t } from "../i18n";

export type Tab = "mods" | "resources" | "configs" | "options" | "launch" | "snapshots" | "servers" | "logs" | "profile";

const TABS: [Tab, string][] = [["mods", t("Моды · Mod Doctor")], ["resources", t("Ресурсы")], ["configs", t("Конфиги")], ["launch", t("Запуск и память")], ["options", t("Настройки игры")], ["snapshots", t("Снимки и миры")], ["servers", t("Серверы")], ["logs", t("Логи")], ["profile", t("Профиль")]];

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
            {advice.map((a) => <span key={a} className="small" style={{ color: "var(--warn)" }}>⚠ {a}</span>)}
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

function LogsTab({ inst, state }: { inst: Instance; state?: LaunchState }) {
  const [file, setFile] = useState<string[] | null>(null);
  const [err, setErr] = useState("");
  const live = state && state.logs.length > 0;
  useEffect(() => {
    if (live) return;
    api.readLog(inst.id).then(setFile).catch((e) => setErr(errText(e)));
  }, [inst.id, live]);
  return (
    <>
      {state?.command && (
        <details style={{ marginBottom: 12 }}>
          <summary className="muted small" style={{ cursor: "pointer" }}>{t("Точная команда запуска (токен скрыт)")}</summary>
          <div className="cmd" style={{ marginTop: 6 }}>{state.command}</div>
        </details>
      )}
      {live ? <LogView lines={state.logs} /> : file ? <LogView lines={file} follow={false} /> : <p className="muted">{err || t("Загружаем лог…")}</p>}
    </>
  );
}

export function InstancePage({ inst, instances, state, accounts, initialTab, onChanged, onTrashed, onCloned }: {
  inst: Instance;
  instances: Instance[];
  onCloned: (i: Instance) => void;
  state?: LaunchState;
  accounts: AccountRef[];
  initialTab?: Tab;
  onChanged: (i: Instance) => void;
  onTrashed: () => void;
}) {
  const dialog = useDialog();
  const [tab, setTab] = useState<Tab>(initialTab ?? (inst.loader === "vanilla" ? "launch" : "mods"));
  const [reload, setReload] = useState(0);
  useEffect(() => { if (initialTab) setTab(initialTab); }, [initialTab]);

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
      <div className="pagehead">
        <div>
          <h2>{inst.name}</h2>
          <p className="sub" style={{ margin: 0 }}>Minecraft {inst.game_version} · {loaderName[inst.loader]}{inst.loader_version ? ` ${inst.loader_version}` : ""} · <StatusBadge inst={inst} state={state} /></p>
        </div>
        <div className="row">
          <button className="btn" onClick={() => api.openFolder(inst.id).catch((e) => dialog.alert(t("Не удалось открыть папку"), errText(e)))}>{t("Папка")}</button>
          <button className="btn danger" onClick={trash} disabled={isActive(state)}>{t("В корзину")}</button>
          <PlayButton inst={inst} state={state} compact />
        </div>
      </div>
      {state && isActive(state) && <LaunchProgress state={state} />}
      {state?.phase === "failed" && state.error && <div className="alert err" style={{ marginTop: 12, whiteSpace: "pre-line" }}><div className="h">{t("Запуск не удался")}</div><span>{state.error}</span></div>}
      {state?.phase === "done" && state.exit?.analysis && (
        <div style={{ marginTop: 12 }}><CrashCard inst={inst} a={state.exit.analysis} code={state.exit.code} onOpenTab={setTab} /></div>
      )}
      <div className="ptabs" role="tablist">
        {TABS.map(([tabId, label]) => (
          <button key={tabId} role="tab" aria-selected={tab === tabId} tabIndex={tab === tabId ? 0 : -1} className={tab === tabId ? "on" : ""} onClick={() => setTab(tabId)}
            onKeyDown={(e) => {
              const i = TABS.findIndex(([x]) => x === tabId);
              const next = e.key === "ArrowRight" ? (i + 1) % TABS.length : e.key === "ArrowLeft" ? (i - 1 + TABS.length) % TABS.length : e.key === "Home" ? 0 : e.key === "End" ? TABS.length - 1 : -1;
              if (next < 0) return;
              e.preventDefault();
              setTab(TABS[next][0]);
              (e.currentTarget.parentElement?.children[next] as HTMLElement | undefined)?.focus();
            }}>{label}</button>
        ))}
      </div>
      {tab === "mods" && <ModsTab key={`${inst.id}-${reload}`} inst={inst} running={isActive(state)} />}
      {tab === "launch" && <LaunchTab key={inst.id} inst={inst} accounts={accounts} onChanged={onChanged} />}
      {tab === "snapshots" && <SnapshotsTab inst={inst} running={isActive(state)} onRestored={() => setReload((n) => n + 1)} />}
      {tab === "options" && <OptionsTab key={inst.id} inst={inst} running={isActive(state)} />}
      {tab === "configs" && <ConfigsTab key={inst.id} inst={inst} running={isActive(state)} />}
      {tab === "resources" && <ResourcesTab key={inst.id} inst={inst} running={isActive(state)} />}
      {tab === "servers" && <ServersTab key={inst.id} inst={inst} running={isActive(state)} />}
      {tab === "logs" && <LogsTab inst={inst} state={state} />}
      {tab === "profile" && <ProfileTab key={inst.id} inst={inst} instances={instances} running={isActive(state)} onChanged={onChanged} onCloned={onCloned} />}
    </>
  );
}
