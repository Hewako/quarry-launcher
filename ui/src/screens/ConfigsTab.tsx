// Вкладка «Конфиги»: поиск по настройкам модов и игры, правка с проверкой синтаксиса,
// предпросмотр изменений, история прежних версий.
import { useCallback, useEffect, useMemo, useState } from "react";
import { configs, errText, relTime, sizeText, type ConfigFile, type DiffLine, type Instance } from "../api";
import { Modal, useDialog } from "../ui/Dialog";
import { t, tc } from "../i18n";

function Diff({ lines }: { lines: DiffLine[] }) {
  const changed = lines.filter((l) => l.kind !== "=").length;
  if (changed === 0) return <p className="muted">{t("Изменений нет.")}</p>;
  return (
    <div className="diff cfgdiff">
      {lines.map((l, i) => l.kind === "=" ? <span key={i} className="muted">  {l.text}</span>
        : <span key={i} className={l.kind === "+" ? "add" : "del"}>{l.kind} {l.text}</span>)}
    </div>
  );
}

export function ConfigsTab({ inst, running }: { inst: Instance; running: boolean }) {
  const dialog = useDialog();
  const [files, setFiles] = useState<ConfigFile[] | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<{ path: string; line: number; text: string }[]>([]);
  const [path, setPath] = useState("");
  const [orig, setOrig] = useState("");
  const [text, setText] = useState("");
  const [preview, setPreview] = useState<{ diff: DiffLine[]; error?: string } | null>(null);
  const [history, setHistory] = useState<{ at: number; size: number }[]>([]);

  const loadList = useCallback(() => { configs.list(inst.id).then(setFiles).catch(() => setFiles([])); }, [inst.id]);
  useEffect(loadList, [loadList]);
  useEffect(() => {
    if (query.trim().length < 3) { setHits([]); return; }
    const timer = setTimeout(() => configs.search(inst.id, query).then(setHits).catch(() => setHits([])), 300);
    return () => clearTimeout(timer);
  }, [inst.id, query]);

  const open = async (p: string) => {
    if (text !== orig && !(await dialog.confirm({ title: t("Не сохранено"), body: t("Изменения в открытом файле пропадут."), confirm: t("Открыть другой"), danger: true }))) return;
    try {
      const body = await configs.read(inst.id, p);
      setPath(p); setOrig(body); setText(body);
      configs.history(inst.id, p).then(setHistory).catch(() => setHistory([]));
    } catch (e) { await dialog.alert(t("Не открылся"), errText(e)); }
  };
  const showChanges = async () => {
    try { setPreview(await configs.preview(inst.id, path, text)); } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); }
  };
  const save = async () => {
    try {
      await configs.save(inst.id, path, text);
      setOrig(text); setPreview(null); loadList();
      configs.history(inst.id, path).then(setHistory).catch(() => {});
    } catch (e) { await dialog.alert(t("Не сохранено"), errText(e)); }
  };
  const restore = async (at: number) => {
    try { setText(await configs.historyRead(inst.id, path, at)); } catch (e) { await dialog.alert(t("Не открылась"), errText(e)); }
  };

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (files ?? []).filter((f) => !q || f.path.toLowerCase().includes(q));
  }, [files, query]);
  const contentHits = hits.filter((h) => h.line > 0);

  return (
    <div className="cfg">
      <div className="cfglist card" style={{ padding: 0 }}>
        <input className="field" type="search" placeholder={t("Поиск по именам и содержимому…")} aria-label={t("Поиск по конфигам")} value={query} onChange={(e) => setQuery(e.target.value)} />
        <div className="cfgscroll">
          {files === null ? <p className="muted small"><span className="spinner" /> {t("Загружаем…")}</p> : shown.length === 0 && contentHits.length === 0 ? <p className="muted small">{t("Ничего не найдено.")}</p> : null}
          {shown.map((f) => (
            <button key={f.path} className={`cfgitem ${f.path === path ? "on" : ""}`} onClick={() => open(f.path)} title={f.path}>
              <span className="mono">{f.path.replace(/^config\//, "")}</span>
              {f.user_edited && <span className="badge src" title={t("Правили вы — обновление сборки этот файл не заменит")}>{t("ваш")}</span>}
            </button>
          ))}
          {contentHits.length > 0 && <p className="muted small" style={{ margin: "10px 8px 4px" }}>{t("В содержимом:")}</p>}
          {contentHits.map((h) => (
            <button key={`${h.path}:${h.line}`} className="cfgitem" onClick={() => open(h.path)}>
              <span className="mono small">{h.path.replace(/^config\//, "")}:{h.line}</span>
              <span className="muted small hitline">{h.text}</span>
            </button>
          ))}
        </div>
      </div>
      <div className="cfgedit">
        {!path ? (
          <div className="card empty"><p>{t("Выберите файл слева. Lime проверит синтаксис JSON и TOML перед сохранением, покажет изменения и сохранит прежнюю версию в историю.")}</p></div>
        ) : (
          <>
            <div className="row" style={{ marginBottom: 8 }}>
              <b className="mono">{path}</b>
              <span style={{ flex: 1 }} />
              <button className="btn" disabled={text === orig} onClick={() => setText(orig)}>{t("Отменить правки")}</button>
              <button className="btn pri" disabled={running || text === orig} onClick={showChanges}>{t("Сохранить…")}</button>
            </div>
            {running && <p className="small" style={{ color: "var(--warn)", marginTop: 0 }}>{t("Игра запущена: она перезапишет настройки при выходе, поэтому сохранить можно после закрытия.")}</p>}
            <textarea className="field mono cfgtext" spellCheck={false} aria-label={t("Содержимое {0}", path)} value={text} onChange={(e) => setText(e.target.value)} />
            {history.length > 0 && (
              <details className="cfghist">
                <summary>{t("История:")} {history.length} {t("прежних версий")}</summary>
                {history.map((h) => (
                  <div key={h.at} className="row small">
                    <span>{new Date(h.at).toLocaleString()} <span className="muted">({relTime(Math.floor(h.at / 1000))}, {sizeText(h.size)})</span></span>
                    <button className="btn link small" onClick={() => restore(h.at)}>{t("Загрузить в редактор")}</button>
                  </div>
                ))}
              </details>
            )}
          </>
        )}
      </div>
      {preview && (
        <Modal title={t("Сохранить {0}?", path)} onClose={() => setPreview(null)} wide>
          {preview.error && <div className="alert err"><span>{tc(preview.error)}</span></div>}
          <Diff lines={preview.diff} />
          <p className="muted small">{t("Прежняя версия сохранится в историю. Файл отметится как ваш — обновление сборки его не заменит.")}</p>
          <div className="acts">
            <button className="btn" onClick={() => setPreview(null)}>{t("Вернуться к правке")}</button>
            <button className="btn pri" disabled={!!preview.error} onClick={save}>{t("Сохранить")}</button>
          </div>
        </Modal>
      )}
    </div>
  );
}
