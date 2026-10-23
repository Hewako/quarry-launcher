// Импорт: инстансы Prism/MultiMC, .mrpack, zip CurseForge, папка официального лаунчера.
// Исходные файлы только читаются.
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, errText, importer, type ImportResult, type Instance, type LoaderKind, type SearchHit } from "../api";
import { Modal, Panel } from "../ui/Dialog";
import { t, tc } from "../i18n";

export function ImportDialog({ onClose, onDone, embedded }: { onClose: () => void; onDone: (i: Instance) => void; embedded?: boolean }) {
  const Wrap = embedded ? Panel : Modal;
  const [sources, setSources] = useState<{ prism: [string, string][]; official?: string } | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [result, setResult] = useState<ImportResult | null>(null);
  const [link, setLink] = useState<{ path: string; name: string; version: string; loader: LoaderKind } | null>(null);
  const [optional, setOptional] = useState<{ path: string; files: string[]; off: string[]; projectId?: string; versionId?: string } | null>(null);
  const [packQuery, setPackQuery] = useState("");
  const [packs, setPacks] = useState<SearchHit[]>([]);
  useEffect(() => {
    if (packQuery.trim().length < 2) { setPacks([]); return; }
    const timer = setTimeout(() => importer.searchPacks(packQuery).then((r) => setPacks(r.hits.slice(0, 6))).catch(() => setPacks([])), 300);
    return () => clearTimeout(timer);
  }, [packQuery]);
  /** Сборка из каталога: скачать .mrpack, спросить про необязательные файлы, импортировать. */
  const installPack = async (h: SearchHit) => {
    setBusy(h.project_id);
    setError("");
    try {
      const v = await importer.download(h.project_id);
      const files = (await importer.preview(v.path!)).optional;
      if (files.length) setOptional({ path: v.path!, files, off: [], projectId: h.project_id, versionId: v.version_id });
      else setResult(await importer.path(v.path!, [], h.project_id, v.version_id));
    } catch (e) { setError(errText(e)); } finally { setBusy(""); }
  };

  useEffect(() => { importer.sources().then(setSources).catch(() => setSources({ prism: [] })); }, []);

  const run = async (key: string, f: () => Promise<ImportResult>) => {
    setBusy(key);
    setError("");
    try { setResult(await f()); } catch (e) { setError(errText(e)); } finally { setBusy(""); }
  };
  const pickFile = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: t("Сборка"), extensions: ["mrpack", "zip"] }] });
    if (typeof path !== "string") return;
    // Опциональные компоненты сборки — выбор игрока перед импортом.
    const files = path.endsWith(".mrpack") ? await importer.preview(path).then((p) => p.optional).catch(() => []) : [];
    if (files.length) setOptional({ path, files, off: [] });
    else await run("file", () => importer.path(path));
  };
  const pickFolder = async () => {
    const path = await open({ multiple: false, directory: true });
    if (typeof path === "string") await run("folder", () => importer.path(path));
  };

  if (optional) {
    const o = optional;
    return (
      <Modal title={t("Дополнительные компоненты сборки")} onClose={() => setOptional(null)}>
        <p className="small muted" style={{ marginTop: 0 }}>{t("Автор сборки сделал эти файлы необязательными. Снимите галочки с тех, что не нужны.")}</p>
        {o.files.map((f) => (
          <label key={f} className="row small">
            <input type="checkbox" checked={!o.off.includes(f)} onChange={(e) => setOptional({ ...o, off: e.target.checked ? o.off.filter((x) => x !== f) : [...o.off, f] })} />
            <span className="mono">{f.split("/").pop()}</span>
          </label>
        ))}
        <div className="acts">
          <button className="btn" onClick={() => setOptional(null)}>{t("Отмена")}</button>
          <button className="btn pri" disabled={busy !== ""} onClick={async () => { setOptional(null); await run("file", () => importer.path(o.path, o.off, o.projectId, o.versionId)); }}>{t("Импортировать")}</button>
        </div>
      </Modal>
    );
  }

  if (result) {
    return (
      <Modal title={t("Импорт завершён")} onClose={() => onDone(result.instance)}>
        <p style={{ margin: 0 }}>{t("Профиль «")}{result.instance.name}» — Minecraft {result.instance.game_version} · {result.instance.loader}.</p>
        {result.notes.map((n, i) => <span key={i} className="muted small">• {tc(n)}</span>)}
        {result.manual.length > 0 && (
          <div className="alert warn">
            <div className="h">{t("Скачайте вручную:")} {result.manual.length}</div>
            <span className="small">{t("CurseForge разрешает скачивать эти моды только через свой сайт. Скачайте файлы и положите их в папку mods профиля — Mod Doctor проверит, всё ли на месте.")}</span>
            <div style={{ maxHeight: 160, overflow: "auto" }} className="small mono">
              {result.manual.map((m) => <div key={m.file_id}>{t("проект")} {m.project_id}{t(", файл")} {m.file_id}</div>)}
            </div>
            <div className="acts"><button className="btn" onClick={() => api.openFolder(result.instance.id, "mods")}>{t("Открыть папку mods")}</button></div>
          </div>
        )}
        <div className="acts"><button className="btn pri" data-autofocus onClick={() => onDone(result.instance)}>{t("Открыть профиль")}</button></div>
      </Modal>
    );
  }

  return (
    <Wrap title={embedded ? t("Импортировать готовую сборку") : t("Импорт")} onClose={onClose} wide>
      <h4 style={{ margin: 0 }}>{t("Сборка из каталога Modrinth")}</h4>
      <input className="field" type="search" aria-label={t("Найти сборку")} placeholder={t("Найти сборку…")} value={packQuery} onChange={(e) => setPackQuery(e.target.value)} />
      {packs.map((h) => (
        <div className="hit" key={h.project_id}>
          {h.icon_url ? <img src={h.icon_url} alt="" /> : <div className="noicon" />}
          <div className="grow"><b>{h.title}</b> <span className="muted small">· {h.author}</span><small>{h.description}</small></div>
          <button className="btn" disabled={busy !== ""} onClick={() => installPack(h)}>{busy === h.project_id ? t("Скачиваем…") : t("Установить")}</button>
        </div>
      ))}
      <span className="muted small">{t("Исходные файлы не меняются: Axel делает свою копию. Если импорт не удастся, недоделанный профиль убирается.")}</span>
      <h4 style={{ margin: 0 }}>{t("Из Prism Launcher или MultiMC")}</h4>
      {sources === null ? <span className="muted small"><span className="spinner" /> {t("Ищем инстансы…")}</span> : sources.prism.length === 0 ? (
        <span className="muted small">{t("Инстансов Prism/MultiMC в стандартной папке не найдено.")}</span>
      ) : (
        <div className="hits" style={{ maxHeight: 220 }}>
          {sources.prism.map(([path, name]) => (
            <div className="hit" key={path}>
              <div className="grow"><b>{name}</b><small>{path}</small></div>
              <button className="btn" disabled={busy !== ""} onClick={() => run(path, () => importer.path(path))}>{busy === path ? t("Импортируем…") : t("Импортировать")}</button>
            </div>
          ))}
        </div>
      )}
      <div className="row">
        <button className="btn" disabled={busy !== ""} onClick={pickFolder}>{t("Выбрать папку инстанса…")}</button>
        <button className="btn pri" disabled={busy !== ""} onClick={pickFile}>{busy === "file" ? t("Скачиваем моды сборки…") : t("Выбрать .mrpack или .zip CurseForge…")}</button>
      </div>
      {sources?.official && (
        <>
          <h4 style={{ margin: 0 }}>{t("Папка официального лаунчера")}</h4>
          {!link ? (
            <div className="row">
              <span className="muted small">{sources.official}</span>
              <button className="btn" onClick={() => setLink({ path: sources.official!, name: t("Официальный лаунчер"), version: "", loader: "vanilla" })}>{t("Подключить без копирования…")}</button>
            </div>
          ) : (
            <form className="form" onSubmit={(e) => { e.preventDefault(); run("link", () => importer.link(link.path, link.name, link.version, link.loader)); }}>
              <span className="muted small">{t("Профиль будет работать прямо в этой папке: миры и настройки общие с официальным лаунчером. Корзина Axel эту папку не трогает.")}</span>
              <div className="row">
                <input className="field" style={{ flex: 2 }} value={link.name} onChange={(e) => setLink({ ...link, name: e.target.value })} aria-label={t("Название")} />
                <input className="field" style={{ flex: 1 }} placeholder={t("Версия, например 26.3")} value={link.version} onChange={(e) => setLink({ ...link, version: e.target.value.trim() })} aria-label={t("Версия")} />
                <select className="field" style={{ flex: 1 }} value={link.loader} onChange={(e) => setLink({ ...link, loader: e.target.value as LoaderKind })}>
                  <option value="vanilla">Vanilla</option><option value="fabric">Fabric</option><option value="quilt">Quilt</option><option value="neoforge">NeoForge</option>
                </select>
                <button className="btn pri" disabled={!link.version || busy !== ""}>{t("Подключить")}</button>
              </div>
            </form>
          )}
        </>
      )}
      {error && <div className="alert err"><span style={{ userSelect: "text" }}>{error}</span></div>}
      {!embedded && <div className="acts"><button className="btn" onClick={onClose}>{t("Закрыть")}</button></div>}
    </Wrap>
  );
}
