import { useCallback, useEffect, useState } from "react";
import { api, errText, loaderName, sourceName, type AccountRef, type Instance, type LaunchOverrides, type Resolved, type Source } from "../api";
import { isActive, type LaunchState } from "../launches";
import { useDialog } from "../ui/Dialog";
import { LaunchProgress, PlayButton, StatusBadge } from "../ui/common";
import { LogView } from "../ui/LogView";

type Tab = "launch" | "logs";

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
  const [saving, setSaving] = useState(false);

  const load = useCallback(() => {
    api.launchSettings(inst.id).then((d) => { setData(d); setDraft(d.profile); setArgs((d.profile.jvm_args ?? []).join(" ")); }).catch(() => setData(null));
  }, [inst.id]);
  useEffect(load, [load]);

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
      const updated = await api.updateInstance(inst.id, { settings: { ...draft, jvm_args: parsed } });
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
            <span className="muted small">Xms = Xmx: память не перевыделяется посреди игры. Больше 8 ГБ обычно нужно только большим сборкам.{total && max.value > total * 0.75 ? " Внимание: системе и браузеру останется мало памяти." : ""}</span>
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
      </div>
      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn pri" onClick={save} disabled={saving}>{saving ? "Сохраняем…" : "Сохранить"}</button>
        <span className="muted small">Прирост FPS от флагов не обещаем: он зависит от модов, Java и железа.</span>
      </div>
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

export function InstancePage({ inst, state, accounts, initialTab, onChanged, onTrashed }: {
  inst: Instance;
  state?: LaunchState;
  accounts: AccountRef[];
  initialTab?: Tab;
  onChanged: (i: Instance) => void;
  onTrashed: () => void;
}) {
  const dialog = useDialog();
  const [tab, setTab] = useState<Tab>(initialTab ?? "launch");
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
      {state?.phase === "failed" && state.error && <div className="alert err" style={{ marginTop: 12 }}><div className="h">Запуск не удался</div><span>{state.error}</span></div>}
      <div className="ptabs" role="tablist">
        <button role="tab" aria-selected={tab === "launch"} className={tab === "launch" ? "on" : ""} onClick={() => setTab("launch")}>Запуск и память</button>
        <button role="tab" aria-selected={tab === "logs"} className={tab === "logs" ? "on" : ""} onClick={() => setTab("logs")}>Логи</button>
      </div>
      {tab === "launch" ? <LaunchTab key={inst.id} inst={inst} accounts={accounts} onChanged={onChanged} /> : <LogsTab inst={inst} state={state} />}
    </>
  );
}
