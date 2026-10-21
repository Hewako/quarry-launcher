import { useCallback, useEffect, useState } from "react";
import { api, errText, loaderName, sourceName, type AccountRef, type Instance, type LaunchOverrides, type Resolved, type Source } from "../api";
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

export type Tab = "mods" | "resources" | "configs" | "launch" | "snapshots" | "servers" | "logs" | "profile";

const TABS: [Tab, string][] = [["mods", "Моды · Mod Doctor"], ["resources", "Ресурсы"], ["configs", "Конфиги"], ["launch", "Запуск и память"], ["snapshots", "Снимки и миры"], ["servers", "Серверы"], ["logs", "Логи"], ["profile", "Профиль"]];

function SourceTag({ source, onReset }: { source: Source; onReset?: () => void }) {
  return (
    <div className="srcbox">
      <span className={`badge src ${source === "profile" ? "ok" : ""}`}>{sourceName[source]}</span>
      {source === "profile" && onReset && <button className="reset" onClick={onReset}>сбросить</button>}
    </div>
  );
}

function LaunchTab({ inst, accounts, onChanged }: { inst: Instance; accounts: AccountRef[]; onChanged: (i: Instance) => void }) {
  const dialog = useDialog();
  const [data, setData] = useState<Awaited<ReturnType<typeof api.launchSettings>> | null>(null);
  const [draft, setDraft] = useState<LaunchOverrides>(inst.settings);
  const [args, setArgs] = useState((inst.settings.jvm_args ?? []).join(" "));
  const [gameArgs, setGameArgs] = useState((inst.settings.game_args ?? []).join(" "));
  const [envText, setEnvText] = useState(Object.entries(inst.settings.env ?? {}).map(([k, v]) => `${k}=${v}`).join("\n"));
  const [advice, setAdvice] = useState<string[]>([]);
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
    const t = setTimeout(() => api.memoryAdvice(maxForAdvice, inst.pack_settings.max_memory_mb).then(setAdvice).catch(() => setAdvice([])), 250);
    return () => clearTimeout(t);
  }, [maxForAdvice, inst.pack_settings.max_memory_mb]);

  if (!data) return <p className="muted"><span className="spinner" /> Загружаем настройки…</p>;

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
        if (i < 1) throw new Error(`Строка «${line}»: нужно ИМЯ=значение`);
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
      await dialog.alert("Настройки не сохранены", errText(e));
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
        <div className="h">Откуда берутся значения</div>
        <span className="small">Значение берётся с ближайшего уровня, где оно задано: профиль → сборка → глобальные. Метка показывает, откуда пришло текущее значение; «сбросить» возвращает наследуемое.</span>
      </div>
      <div className="card">
        <div className="setting">
          <div className="lbl">Память<span className="hint">{(max.value / 1024).toFixed(1).replace(".0", "")} ГБ{total ? ` из ${Math.round(total / 1024)} ГБ ОЗУ` : ""}</span></div>
          <div>
            <input type="range" min={1024} max={ceiling} step={512} value={Math.min(max.value, ceiling)} aria-label="Память, МБ"
              onChange={(e) => setDraft({ ...draft, max_memory_mb: Number(e.target.value), min_memory_mb: undefined })} />
            <span className="muted small">Xms = Xmx: память не перевыделяется посреди игры.{inst.pack_settings.max_memory_mb ? ` Сборка рекомендует ${inst.pack_settings.max_memory_mb} МБ.` : ""}</span>
            {advice.map((a) => <span key={a} className="small" style={{ color: "var(--warn)" }}>⚠ {a}</span>)}
          </div>
          <SourceTag source={max.source} onReset={() => setDraft({ ...draft, max_memory_mb: undefined, min_memory_mb: undefined })} />
        </div>
        <div className="setting">
          <div className="lbl">Java<span className="hint">по умолчанию — по javaVersion из JSON версии</span></div>
          <input className="field mono" placeholder="Авто: лаунчер подберёт или скачает нужную" value={draft.java_path ?? ""}
            onChange={(e) => setDraft({ ...draft, java_path: e.target.value || undefined })} />
          <SourceTag source={javaEff.source} onReset={() => setDraft({ ...draft, java_path: undefined })} />
        </div>
        <div className="setting">
          <div className="lbl">Аргументы JVM<span className="hint">через пробел; -Xmx и -javaagent запрещены</span></div>
          <input className="field mono" placeholder="например -XX:+UseG1GC" value={args} onChange={(e) => setArgs(e.target.value)} />
          <SourceTag source={args.trim() ? "profile" : inst.pack_settings.jvm_args ? "pack" : data.global.jvm_args ? "global" : "default"} onReset={() => setArgs("")} />
        </div>
        <div className="setting">
          <div className="lbl">Размер окна игры<span className="hint">пусто — как решит игра</span></div>
          <div className="row" style={{ flexWrap: "nowrap" }}>
            <input className="field" type="number" min={320} aria-label="Ширина окна" placeholder="ширина" value={draft.resolution?.[0] ?? ""}
              onChange={(e) => setDraft({ ...draft, resolution: e.target.value ? [Number(e.target.value), draft.resolution?.[1] ?? 720] : undefined })} />
            <span className="muted">×</span>
            <input className="field" type="number" min={240} aria-label="Высота окна" placeholder="высота" value={draft.resolution?.[1] ?? ""}
              onChange={(e) => setDraft({ ...draft, resolution: e.target.value ? [draft.resolution?.[0] ?? 1280, Number(e.target.value)] : undefined })} />
          </div>
          <SourceTag source={draft.resolution ? "profile" : inst.pack_settings.resolution ? "pack" : data.global.resolution ? "global" : "default"} onReset={() => setDraft({ ...draft, resolution: undefined })} />
        </div>
        <div className="setting">
          <div className="lbl">Аргументы игры<span className="hint">через пробел, в конце команды; вход и папки задаёт лаунчер</span></div>
          <input className="field mono" placeholder="например --fullscreen" value={gameArgs} onChange={(e) => setGameArgs(e.target.value)} />
          <SourceTag source={gameArgs.trim() ? "profile" : inst.pack_settings.game_args ? "pack" : data.global.game_args ? "global" : "default"} onReset={() => setGameArgs("")} />
        </div>
        <div className="setting">
          <div className="lbl">Переменные окружения<span className="hint">ИМЯ=значение, по одной в строке; LD_*, DYLD_* и JAVA_TOOL_OPTIONS запрещены</span></div>
          <textarea className="field mono" rows={2} placeholder="MESA_GL_VERSION_OVERRIDE=4.5" value={envText} onChange={(e) => setEnvText(e.target.value)} />
          <SourceTag source={envText.trim() ? "profile" : inst.pack_settings.env ? "pack" : data.global.env ? "global" : "default"} onReset={() => setEnvText("")} />
        </div>
        <div className="setting">
          <div className="lbl">Вход<span className="hint">Ely.by — только при Microsoft-аккаунте с купленной игрой</span></div>
          <select className="field" value={inst.auth_mode} onChange={async (e) => {
            try { onChanged(await api.updateInstance(inst.id, { authMode: e.target.value as Instance["auth_mode"] })); }
            catch (err) { await dialog.alert("Нельзя переключить вход", errText(err)); }
          }}>
            <option value="microsoft">Microsoft — официальные серверы</option>
            <option value="ely_by" disabled={!ms.some((a) => a.owns_java_edition) || ely.length === 0}>Ely.by — только совместимые серверы</option>
          </select>
          <span />
        </div>
        {(inst.auth_mode === "microsoft" ? ms : ely).length > 1 && (
          <div className="setting">
            <div className="lbl">Аккаунт<span className="hint">для этого профиля</span></div>
            <select className="field" value={inst.account ?? ""} onChange={async (e) => {
              try { onChanged(await api.updateInstance(inst.id, { account: e.target.value || null })); }
              catch (err) { await dialog.alert("Не удалось сменить аккаунт", errText(err)); }
            }}>
              <option value="">По умолчанию</option>
              {(inst.auth_mode === "microsoft" ? ms : ely).map((a) => <option key={a.uuid} value={`${a.provider}:${a.uuid}`}>{a.name}</option>)}
            </select>
            <SourceTag source={inst.account ? "profile" : "global"} />
          </div>
        )}
      </div>
      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn pri" onClick={save} disabled={saving}>{saving ? "Сохраняем…" : "Сохранить"}</button>
        <button className="btn" onClick={() => { setPreview("…"); api.previewCommand(inst.id).then(setPreview).catch((e) => setPreview(errText(e))); }}>Показать итоговую команду</button>
        <span className="muted small">Прирост FPS от флагов не обещаем: он зависит от модов, Java и железа.</span>
      </div>
      {preview && (
        <div style={{ marginTop: 12 }}>
          <span className="muted small">Команда по сохранённым настройкам. Токен и UUID скрыты.</span>
          <div className="cmd" style={{ marginTop: 6 }}>{preview}</div>
        </div>
      )}
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
          <summary className="muted small" style={{ cursor: "pointer" }}>Точная команда запуска (токен скрыт)</summary>
          <div className="cmd" style={{ marginTop: 6 }}>{state.command}</div>
        </details>
      )}
      {live ? <LogView lines={state.logs} /> : file ? <LogView lines={file} follow={false} /> : <p className="muted">{err || "Загружаем лог…"}</p>}
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
      title: `Убрать «${inst.name}» в корзину?`,
      body: <>Профиль вместе с мирами, модами и конфигами переместится в папку <span className="mono">trash/</span> внутри папки данных Quarry. Ничего не удаляется: чтобы вернуть, перенесите папку обратно в <span className="mono">instances/</span>.</>,
      confirm: "Убрать в корзину",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.trashInstance(inst.id);
      onTrashed();
    } catch (e) {
      await dialog.alert("Не получилось", errText(e));
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
          <button className="btn" onClick={() => api.openFolder(inst.id).catch((e) => dialog.alert("Не удалось открыть папку", errText(e)))}>Папка</button>
          <button className="btn danger" onClick={trash} disabled={isActive(state)}>В корзину</button>
          <PlayButton inst={inst} state={state} compact />
        </div>
      </div>
      {state && isActive(state) && <LaunchProgress state={state} />}
      {state?.phase === "failed" && state.error && <div className="alert err" style={{ marginTop: 12, whiteSpace: "pre-line" }}><div className="h">Запуск не удался</div><span>{state.error}</span></div>}
      {state?.phase === "done" && state.exit?.analysis && (
        <div style={{ marginTop: 12 }}><CrashCard inst={inst} a={state.exit.analysis} code={state.exit.code} onOpenTab={setTab} /></div>
      )}
      <div className="ptabs" role="tablist">
        {TABS.map(([t, label]) => (
          <button key={t} role="tab" aria-selected={tab === t} className={tab === t ? "on" : ""} onClick={() => setTab(t)}>{label}</button>
        ))}
      </div>
      {tab === "mods" && <ModsTab key={`${inst.id}-${reload}`} inst={inst} running={isActive(state)} />}
      {tab === "launch" && <LaunchTab key={inst.id} inst={inst} accounts={accounts} onChanged={onChanged} />}
      {tab === "snapshots" && <SnapshotsTab inst={inst} running={isActive(state)} onRestored={() => setReload((n) => n + 1)} />}
      {tab === "configs" && <ConfigsTab key={inst.id} inst={inst} running={isActive(state)} />}
      {tab === "resources" && <ResourcesTab key={inst.id} inst={inst} running={isActive(state)} />}
      {tab === "servers" && <ServersTab key={inst.id} inst={inst} running={isActive(state)} />}
      {tab === "logs" && <LogsTab inst={inst} state={state} />}
      {tab === "profile" && <ProfileTab key={inst.id} inst={inst} instances={instances} running={isActive(state)} onChanged={onChanged} onCloned={onCloned} />}
    </>
  );
}
