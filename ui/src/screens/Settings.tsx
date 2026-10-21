import { useEffect, useState } from "react";
import { api, diagnostics, errText, sizeText, type AppInfo, type GlobalSettings } from "../api";
import { useDialog } from "../ui/Dialog";
import { path, setStreamer } from "../streamer";

const ACCENTS = ["#3fb950", "#3b7dd8", "#8b5cf6", "#d0703a", "#c2415d", "#2a9d8f"];

export function Settings({ info, theme, onTheme, onAppearance }: { info: AppInfo; theme: string; onTheme: (t: string) => void; onAppearance: (g: GlobalSettings) => void }) {
  const dialog = useDialog();
  const [s, setS] = useState<GlobalSettings | null>(null);
  const [args, setArgs] = useState("");
  const [saved, setSaved] = useState(false);
  const [storage, setStorage] = useState<{ label: string; size: number; cleanable: boolean }[] | null>(null);
  const loadStorage = () => { diagnostics.storage().then(setStorage).catch(() => setStorage([])); };

  useEffect(() => {
    api.globalSettings().then((g) => { setS(g); setArgs((g.launch.jvm_args ?? []).join(" ")); }).catch(() => {});
  }, []);
  if (!s) return <p className="muted"><span className="spinner" /> Загружаем…</p>;

  const save = async () => {
    try {
      const next: GlobalSettings = { ...s, theme, launch: { ...s.launch, jvm_args: args.trim() ? args.trim().split(/\s+/) : undefined } };
      await api.saveGlobalSettings(next);
      setStreamer(next.streamer_mode);
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
          <div className="lbl">Акцентный цвет</div>
          <div className="row" role="radiogroup" aria-label="Акцентный цвет">
            <button className={`swatch auto ${!s.accent ? "on" : ""}`} role="radio" aria-checked={!s.accent} title="Как в теме" onClick={() => { const n = { ...s, accent: undefined }; setS(n); onAppearance(n); }}>A</button>
            {ACCENTS.map((c) => (
              <button key={c} className={`swatch ${s.accent === c ? "on" : ""}`} role="radio" aria-checked={s.accent === c} aria-label={c} style={{ background: c }}
                onClick={() => { const n = { ...s, accent: c }; setS(n); onAppearance(n); }} />
            ))}
          </div>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Масштаб интерфейса</div>
          <select className="field" value={s.ui_scale} onChange={(e) => { const n = { ...s, ui_scale: Number(e.target.value) }; setS(n); onAppearance(n); }}>
            {[90, 100, 110, 125, 150].map((v) => <option key={v} value={v}>{v}%</option>)}
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Компактный режим<span className="hint">меньше отступов — больше строк на экране</span></div>
          <label className="row"><input type="checkbox" checked={s.compact} onChange={(e) => { const n = { ...s, compact: e.target.checked }; setS(n); onAppearance(n); }} /> включён</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Режим стримера<span className="hint">скрывать ник, пути и адреса серверов на экране, в логах и в отчётах</span></div>
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
          <div className="lbl">Бэкап миров по расписанию<span className="hint">перед запуском, если в мир играли после прошлого бэкапа</span></div>
          <select className="field" value={s.backup_every_days} onChange={(e) => setS({ ...s, backup_every_days: Number(e.target.value) })}>
            <option value={0}>Выключен</option>
            <option value={1}>Каждый день</option>
            <option value={3}>Раз в 3 дня</option>
            <option value={7}>Раз в неделю</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Скорость загрузок<span className="hint">чтобы не мешать другим в сети</span></div>
          <select className="field" value={s.download_limit_kbps} onChange={(e) => setS({ ...s, download_limit_kbps: Number(e.target.value) })}>
            <option value={0}>Без ограничения</option>
            <option value={512}>до 0,5 МБ/с</option>
            <option value={1024}>до 1 МБ/с</option>
            <option value={2048}>до 2 МБ/с</option>
            <option value={5120}>до 5 МБ/с</option>
            <option value={10240}>до 10 МБ/с</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Папка данных</div>
          <span className="mono" style={{ userSelect: "text" }}>{path(info.data_dir, s.streamer_mode)}</span>
          <span />
        </div>
      </div>
      <h4 style={{ margin: "22px 0 10px" }}>Место на диске</h4>
      <div className="card">
        {storage === null ? (
          <button className="btn" onClick={loadStorage}>Посчитать, сколько занимают данные Quarry</button>
        ) : (
          <>
            <div className="usage">
              {storage.map((p) => (
                <div key={p.label} className="urow">
                  <span>{p.label}</span>
                  <div className="bar"><i style={{ width: `${Math.max(1, (p.size / Math.max(1, storage[0]?.size ?? 1)) * 100)}%` }} /></div>
                  <span className="muted small">{sizeText(p.size)}</span>
                </div>
              ))}
            </div>
            <div className="row" style={{ marginTop: 12 }}>
              <button className="btn" disabled={!storage.some((p) => p.cleanable && p.size > 0)} onClick={async () => {
                try { const freed = await diagnostics.cleanCache(); loadStorage(); await dialog.alert("Готово", `Освобождено ${sizeText(freed)}. Миры, моды, настройки и корзина не тронуты.`); }
                catch (e) { await dialog.alert("Не очищено", errText(e)); }
              }}>Очистить временные файлы и кеш</button>
              <span className="muted small">Удаляется только то, что лаунчер скачает или соберёт заново.</span>
            </div>
          </>
        )}
      </div>
      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn pri" onClick={save}>{saved ? "Сохранено" : "Сохранить"}</button>
        <span className="muted small">Quarry {info.version} · открытый проект под GPL-3.0 · не связан с Mojang и Microsoft.</span>
      </div>
    </>
  );
}
