import { useEffect, useState } from "react";
import { api, diagnostics, errText, sizeText, type AppInfo, type GlobalSettings } from "../api";
import { useDialog } from "../ui/Dialog";
import { path, setStreamer } from "../streamer";
import { switchLang, t, tc } from "../i18n";

const ACCENTS: [string, string][] = [["#3fb950", t("Зелёный")], ["#2563eb", t("Синий")], ["#7c3aed", t("Фиолетовый")], ["#d0703a", t("Оранжевый")], ["#c2415d", t("Малиновый")], ["#2a9d8f", t("Бирюзовый")]];

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
  if (!s) return <p className="muted"><span className="spinner" /> {t("Загружаем…")}</p>;

  const save = async () => {
    try {
      const next: GlobalSettings = { ...s, theme, launch: { ...s.launch, jvm_args: args.trim() ? args.trim().split(/\s+/) : undefined } };
      await api.saveGlobalSettings(next);
      setStreamer(next.streamer_mode);
      setS(next);
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } catch (e) {
      await dialog.alert(t("Настройки не сохранены"), errText(e));
    }
  };

  return (
    <>
      <h2>{t("Настройки")}</h2>
      <p className="sub">{t("Глобальные значения — нижний уровень наследования: профиль и сборка их переопределяют.")}</p>
      <div className="card">
        <div className="setting">
          <div className="lbl">{t("Память по умолчанию")}<span className="hint">{t("для профилей без своего значения")}</span></div>
          <input className="field" type="number" min={1024} step={512} placeholder="4096" value={s.launch.max_memory_mb ?? ""}
            onChange={(e) => setS({ ...s, launch: { ...s.launch, max_memory_mb: e.target.value ? Number(e.target.value) : undefined, min_memory_mb: undefined } })} />
          <span className="muted small">{t("МБ")}</span>
        </div>
        <div className="setting">
          <div className="lbl">{t("Java по умолчанию")}<span className="hint">{t("пусто — подбор по версии игры")}</span></div>
          <input className="field mono" placeholder={t("Авто")} value={s.launch.java_path ?? ""}
            onChange={(e) => setS({ ...s, launch: { ...s.launch, java_path: e.target.value || undefined } })} />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Аргументы JVM по умолчанию")}</div>
          <input className="field mono" value={args} onChange={(e) => setArgs(e.target.value)} placeholder={t("например -XX:+UseG1GC")} />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Язык")}<span className="hint">Language</span></div>
          <select className="field" value={s.language} onChange={async (e) => {
            const next = { ...s, language: e.target.value };
            try { await api.saveGlobalSettings(next); switchLang(next.language); } catch (er) { await dialog.alert(t("Настройки не сохранены"), errText(er)); }
          }}>
            <option value="ru">Русский</option>
            <option value="en">English</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Тема")}</div>
          <select className="field" value={theme} onChange={(e) => onTheme(e.target.value)}>
            <option value="dark">{t("Тёмная")}</option>
            <option value="light">{t("Светлая")}</option>
            <option value="system">{t("Как в системе")}</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Акцентный цвет")}</div>
          <div className="row" role="radiogroup" aria-label={t("Акцентный цвет")}>
            <button className={`swatch auto ${!s.accent ? "on" : ""}`} role="radio" aria-checked={!s.accent} aria-label={t("Как в теме")} title={t("Как в теме")} onClick={() => { const n = { ...s, accent: undefined }; setS(n); onAppearance(n); }}>A</button>
            {ACCENTS.map(([c, name]) => (
              <button key={c} className={`swatch ${s.accent === c ? "on" : ""}`} role="radio" aria-checked={s.accent === c} aria-label={name} title={name} style={{ background: c }}
                onClick={() => { const n = { ...s, accent: c }; setS(n); onAppearance(n); }} />
            ))}
          </div>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Масштаб интерфейса")}</div>
          <select className="field" value={s.ui_scale} onChange={(e) => { const n = { ...s, ui_scale: Number(e.target.value) }; setS(n); onAppearance(n); }}>
            {[90, 100, 110, 125, 150].map((v) => <option key={v} value={v}>{v}%</option>)}
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Шрифт")}</div>
          <select className="field" value={s.font} onChange={(e) => { const n = { ...s, font: e.target.value }; setS(n); onAppearance(n); }}>
            <option value="system">{t("Системный")}</option>
            <option value="readable">{t("Разборчивый (шире буквы)")}</option>
            <option value="serif">{t("С засечками")}</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Компактный режим")}<span className="hint">{t("меньше отступов — больше строк на экране")}</span></div>
          <label className="row"><input type="checkbox" checked={s.compact} onChange={(e) => { const n = { ...s, compact: e.target.checked }; setS(n); onAppearance(n); }} /> {t("включён")}</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Режим стримера")}<span className="hint">{t("скрывать ник, пути и адреса серверов на экране, в логах и в отчётах")}</span></div>
          <label className="row"><input type="checkbox" checked={s.streamer_mode} onChange={(e) => setS({ ...s, streamer_mode: e.target.checked })} /> {t("включён")}</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Бэкап мира перед рискованным запуском")}<span className="hint">{t("если моды менялись после прошлой игры")}</span></div>
          <label className="row"><input type="checkbox" checked={s.backup_before_risky_launch} onChange={(e) => setS({ ...s, backup_before_risky_launch: e.target.checked })} /> {t("включён")}</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Бэкап мира после выхода из игры")}<span className="hint">{t("последний мир, в который играли")}</span></div>
          <label className="row"><input type="checkbox" checked={s.backup_after_exit} onChange={(e) => setS({ ...s, backup_after_exit: e.target.checked })} /> {t("включён")}</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Бэкап миров по расписанию")}<span className="hint">{t("перед запуском, если в мир играли после прошлого бэкапа")}</span></div>
          <select className="field" value={s.backup_every_days} onChange={(e) => setS({ ...s, backup_every_days: Number(e.target.value) })}>
            <option value={0}>{t("Выключен")}</option>
            <option value={1}>{t("Каждый день")}</option>
            <option value={3}>{t("Раз в 3 дня")}</option>
            <option value={7}>{t("Раз в неделю")}</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Скорость загрузок")}<span className="hint">{t("чтобы не мешать другим в сети")}</span></div>
          <select className="field" value={s.download_limit_kbps} onChange={(e) => setS({ ...s, download_limit_kbps: Number(e.target.value) })}>
            <option value={0}>{t("Без ограничения")}</option>
            <option value={512}>{t("до 0,5 МБ/с")}</option>
            <option value={1024}>{t("до 1 МБ/с")}</option>
            <option value={2048}>{t("до 2 МБ/с")}</option>
            <option value={5120}>{t("до 5 МБ/с")}</option>
            <option value={10240}>{t("до 10 МБ/с")}</option>
          </select>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Сообщать о новой версии Quarry")}<span className="hint">{t("проверка на GitHub при запуске; скачивание — только по вашему нажатию")}</span></div>
          <label className="row"><input type="checkbox" checked={s.check_launcher_updates} onChange={(e) => setS({ ...s, check_launcher_updates: e.target.checked })} /> {t("включено")}</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Папка данных")}</div>
          <span className="mono" style={{ userSelect: "text" }}>{path(info.data_dir, s.streamer_mode)}</span>
          <span />
        </div>
      </div>
      <h4 style={{ margin: "22px 0 10px" }}>{t("Место на диске")}</h4>
      <div className="card">
        {storage === null ? (
          <button className="btn" onClick={loadStorage}>{t("Посчитать, сколько занимают данные Quarry")}</button>
        ) : (
          <>
            <div className="usage">
              {storage.map((p) => (
                <div key={p.label} className="urow">
                  <span>{tc(p.label)}</span>
                  <div className="bar"><i style={{ width: `${Math.max(1, (p.size / Math.max(1, storage[0]?.size ?? 1)) * 100)}%` }} /></div>
                  <span className="muted small">{sizeText(p.size)}</span>
                </div>
              ))}
            </div>
            <div className="row" style={{ marginTop: 12 }}>
              <button className="btn" disabled={!storage.some((p) => p.cleanable && p.size > 0)} onClick={async () => {
                try { const freed = await diagnostics.cleanCache(); loadStorage(); await dialog.alert(t("Готово"), t("Освобождено {0}. Миры, моды, настройки и корзина не тронуты.", sizeText(freed))); }
                catch (e) { await dialog.alert(t("Не очищено"), errText(e)); }
              }}>{t("Очистить временные файлы и кеш")}</button>
              <span className="muted small">{t("Удаляется только то, что лаунчер скачает или соберёт заново.")}</span>
            </div>
          </>
        )}
      </div>
      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn pri" onClick={save}>{saved ? t("Сохранено") : t("Сохранить")}</button>
        <span className="muted small">Quarry {info.version} {t("· открытый проект под GPL-3.0 · не связан с Mojang и Microsoft.")}</span>
      </div>
    </>
  );
}
