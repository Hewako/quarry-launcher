import { useEffect, useState } from "react";
import { api, errText, type AppInfo, type GlobalSettings } from "../api";
import { useDialog } from "../ui/Dialog";

export function Settings({ info, theme, onTheme }: { info: AppInfo; theme: string; onTheme: (t: string) => void }) {
  const dialog = useDialog();
  const [s, setS] = useState<GlobalSettings | null>(null);
  const [args, setArgs] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.globalSettings().then((g) => { setS(g); setArgs((g.launch.jvm_args ?? []).join(" ")); }).catch(() => {});
  }, []);
  if (!s) return <p className="muted"><span className="spinner" /> Загружаем…</p>;

  const save = async () => {
    try {
      const next: GlobalSettings = { ...s, theme, launch: { ...s.launch, jvm_args: args.trim() ? args.trim().split(/\s+/) : undefined } };
      await api.saveGlobalSettings(next);
      setS(next);
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch (e) {
      await dialog.alert("Настройки не сохранены", errText(e));
    }
  };

  return (
    <>
      <h2>Настройки</h2>
      <p className="sub">Глобальные значения — нижний уровень наследования: профиль и сборка их переопределяют.</p>
      <div className="card">
        <div className="setting">
          <div className="lbl">Память по умолчанию<span className="hint">для профилей без своего значения</span></div>
          <input className="field" type="number" min={1024} step={512} placeholder="4096" value={s.launch.max_memory_mb ?? ""}
            onChange={(e) => setS({ ...s, launch: { ...s.launch, max_memory_mb: e.target.value ? Number(e.target.value) : undefined, min_memory_mb: undefined } })} />
          <span className="muted small">МБ</span>
        </div>
        <div className="setting">
          <div className="lbl">Java по умолчанию<span className="hint">пусто — подбор по версии игры</span></div>
          <input className="field mono" placeholder="Авто" value={s.launch.java_path ?? ""}
            onChange={(e) => setS({ ...s, launch: { ...s.launch, java_path: e.target.value || undefined } })} />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Аргументы JVM по умолчанию</div>
          <input className="field mono" value={args} onChange={(e) => setArgs(e.target.value)} placeholder="например -XX:+UseG1GC" />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Тема</div>
          <select className="field" value={theme} onChange={(e) => onTheme(e.target.value)}>
            <option value="dark">Тёмная</option>
            <option value="light">Светлая</option>
            <option value="system">Как в системе</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Режим стримера<span className="hint">скрывать ник в логах лаунчера</span></div>
          <label className="row"><input type="checkbox" checked={s.streamer_mode} onChange={(e) => setS({ ...s, streamer_mode: e.target.checked })} /> включён</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Бэкап мира перед рискованным запуском<span className="hint">если моды менялись после прошлой игры</span></div>
          <label className="row"><input type="checkbox" checked={s.backup_before_risky_launch} onChange={(e) => setS({ ...s, backup_before_risky_launch: e.target.checked })} /> включён</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Бэкап мира после выхода из игры<span className="hint">последний мир, в который играли</span></div>
          <label className="row"><input type="checkbox" checked={s.backup_after_exit} onChange={(e) => setS({ ...s, backup_after_exit: e.target.checked })} /> включён</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Папка данных</div>
          <span className="mono" style={{ userSelect: "text" }}>{info.data_dir}</span>
          <span />
        </div>
      </div>
      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn pri" onClick={save}>{saved ? "Сохранено" : "Сохранить"}</button>
        <span className="muted small">Quarry {info.version} · открытый проект под GPL-3.0 · не связан с Mojang и Microsoft.</span>
      </div>
    </>
  );
}
