// Импорт: инстансы Prism/MultiMC, .mrpack, zip CurseForge, папка официального лаунчера.
// Исходные файлы только читаются.
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, errText, importer, type ImportResult, type Instance, type LoaderKind } from "../api";
import { Modal } from "../ui/Dialog";

export function ImportDialog({ onClose, onDone }: { onClose: () => void; onDone: (i: Instance) => void }) {
  const [sources, setSources] = useState<{ prism: [string, string][]; official?: string } | null>(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [result, setResult] = useState<ImportResult | null>(null);
  const [link, setLink] = useState<{ path: string; name: string; version: string; loader: LoaderKind } | null>(null);

  useEffect(() => { importer.sources().then(setSources).catch(() => setSources({ prism: [] })); }, []);

  const run = async (key: string, f: () => Promise<ImportResult>) => {
    setBusy(key);
    setError("");
    try { setResult(await f()); } catch (e) { setError(errText(e)); } finally { setBusy(""); }
  };
  const pickFile = async () => {
    const path = await open({ multiple: false, directory: false, filters: [{ name: "Сборка", extensions: ["mrpack", "zip"] }] });
    if (typeof path === "string") await run("file", () => importer.path(path));
  };
  const pickFolder = async () => {
    const path = await open({ multiple: false, directory: true });
    if (typeof path === "string") await run("folder", () => importer.path(path));
  };

  if (result) {
    return (
      <Modal title="Импорт завершён" onClose={() => onDone(result.instance)}>
        <p style={{ margin: 0 }}>Профиль «{result.instance.name}» — Minecraft {result.instance.game_version} · {result.instance.loader}.</p>
        {result.notes.map((n, i) => <span key={i} className="muted small">• {n}</span>)}
        {result.manual.length > 0 && (
          <div className="alert warn">
            <div className="h">Скачайте вручную: {result.manual.length}</div>
            <span className="small">CurseForge разрешает скачивать эти моды только через свой сайт. Скачайте файлы и положите их в папку mods профиля — Mod Doctor проверит, всё ли на месте.</span>
            <div style={{ maxHeight: 160, overflow: "auto" }} className="small mono">
              {result.manual.map((m) => <div key={m.file_id}>проект {m.project_id}, файл {m.file_id}</div>)}
            </div>
            <div className="acts"><button className="btn" onClick={() => api.openFolder(result.instance.id, "mods")}>Открыть папку mods</button></div>
          </div>
        )}
        <div className="acts"><button className="btn pri" data-autofocus onClick={() => onDone(result.instance)}>Открыть профиль</button></div>
      </Modal>
    );
  }

  return (
    <Modal title="Импорт" onClose={onClose} wide>
      <span className="muted small">Исходные файлы не меняются: Quarry делает свою копию. Если импорт не удастся, недоделанный профиль убирается.</span>
      <h4 style={{ margin: 0 }}>Из Prism Launcher или MultiMC</h4>
      {sources === null ? <span className="muted small"><span className="spinner" /> Ищем инстансы…</span> : sources.prism.length === 0 ? (
        <span className="muted small">Инстансов Prism/MultiMC в стандартной папке не найдено.</span>
      ) : (
        <div className="hits" style={{ maxHeight: 220 }}>
          {sources.prism.map(([path, name]) => (
            <div className="hit" key={path}>
              <div className="grow"><b>{name}</b><small>{path}</small></div>
              <button className="btn" disabled={busy !== ""} onClick={() => run(path, () => importer.path(path))}>{busy === path ? "Импортируем…" : "Импортировать"}</button>
            </div>
          ))}
        </div>
      )}
      <div className="row">
        <button className="btn" disabled={busy !== ""} onClick={pickFolder}>Выбрать папку инстанса…</button>
        <button className="btn pri" disabled={busy !== ""} onClick={pickFile}>{busy === "file" ? "Скачиваем моды сборки…" : "Выбрать .mrpack или .zip CurseForge…"}</button>
      </div>
      {sources?.official && (
        <>
          <h4 style={{ margin: 0 }}>Папка официального лаунчера</h4>
          {!link ? (
            <div className="row">
              <span className="muted small">{sources.official}</span>
              <button className="btn" onClick={() => setLink({ path: sources.official!, name: "Официальный лаунчер", version: "", loader: "vanilla" })}>Подключить без копирования…</button>
            </div>
          ) : (
            <form className="form" onSubmit={(e) => { e.preventDefault(); run("link", () => importer.link(link.path, link.name, link.version, link.loader)); }}>
              <span className="muted small">Профиль будет работать прямо в этой папке: миры и настройки общие с официальным лаунчером. Корзина Quarry эту папку не трогает.</span>
              <div className="row">
                <input className="field" style={{ flex: 2 }} value={link.name} onChange={(e) => setLink({ ...link, name: e.target.value })} aria-label="Название" />
                <input className="field" style={{ flex: 1 }} placeholder="Версия, например 26.3" value={link.version} onChange={(e) => setLink({ ...link, version: e.target.value.trim() })} aria-label="Версия" />
                <select className="field" style={{ flex: 1 }} value={link.loader} onChange={(e) => setLink({ ...link, loader: e.target.value as LoaderKind })}>
                  <option value="vanilla">Vanilla</option><option value="fabric">Fabric</option><option value="quilt">Quilt</option><option value="neoforge">NeoForge</option>
                </select>
                <button className="btn pri" disabled={!link.version || busy !== ""}>Подключить</button>
              </div>
            </form>
          )}
        </>
      )}
      {error && <div className="alert err"><span style={{ userSelect: "text" }}>{error}</span></div>}
      <div className="acts"><button className="btn" onClick={onClose}>Закрыть</button></div>
    </Modal>
  );
}
