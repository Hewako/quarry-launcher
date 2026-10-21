// Создание профиля: версия игры из манифеста Mojang → загрузчик → версия загрузчика.
// Совместимость проверяется до создания (ядро ещё раз проверит перед скачиванием).
import { useEffect, useMemo, useState } from "react";
import { api, errText, loaderName, type Instance, type LoaderKind, type LoaderVersion, type VersionEntry } from "../api";
import { Modal } from "../ui/Dialog";

const LOADERS: { kind: LoaderKind; ready: boolean; note?: string }[] = [
  { kind: "vanilla", ready: true },
  { kind: "fabric", ready: true },
  { kind: "quilt", ready: true },
  { kind: "neoforge", ready: false, note: "скоро: нужен запуск установщика" },
  { kind: "forge", ready: false, note: "позже" },
];

export function CreateInstance({ onClose, onCreated }: { onClose: () => void; onCreated: (i: Instance) => void }) {
  const [name, setName] = useState("");
  const [snapshots, setSnapshots] = useState(false);
  const [query, setQuery] = useState("");
  const [versions, setVersions] = useState<VersionEntry[] | null>(null);
  const [offline, setOffline] = useState(false);
  const [version, setVersion] = useState<string>("");
  const [java, setJava] = useState<string>("");
  const [loader, setLoader] = useState<LoaderKind>("vanilla");
  const [loaderVersions, setLoaderVersions] = useState<LoaderVersion[] | null>(null);
  const [loaderVersion, setLoaderVersion] = useState<string>("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setVersions(null);
    api.listVersions(snapshots, false)
      .then((r) => {
        setVersions(r.versions);
        setOffline(r.offline);
        setVersion((v) => v || r.latest_release);
      })
      .catch((e) => setError(errText(e)));
  }, [snapshots]);

  useEffect(() => {
    if (!version) return;
    setJava("");
    api.versionInfo(version).then((i) => setJava(`нужна Java ${i.java_major} (${i.java_reason})`)).catch(() => setJava(""));
  }, [version]);

  useEffect(() => {
    setLoaderVersions(null);
    setLoaderVersion("");
    if (loader === "vanilla" || !version) return;
    api.loaderVersions(loader, version)
      .then((list) => {
        setLoaderVersions(list);
        setLoaderVersion((list.find((v) => v.stable) ?? list[0])?.version ?? "");
      })
      .catch((e) => { setLoaderVersions([]); setError(errText(e)); });
  }, [loader, version]);

  const shown = useMemo(
    () => (versions ?? []).filter((v) => v.id.toLowerCase().includes(query.trim().toLowerCase())).slice(0, 300),
    [versions, query],
  );

  const incompatible = loader !== "vanilla" && loaderVersions !== null && loaderVersions.length === 0;
  const canCreate = name.trim() && version && !busy && !incompatible && (loader === "vanilla" || !!loaderVersion);

  const create = async () => {
    setBusy(true);
    setError("");
    try {
      onCreated(await api.createInstance(name.trim(), version, loader, loader === "vanilla" ? undefined : loaderVersion));
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="Новый профиль" onClose={onClose} wide>
      <form className="form" onSubmit={(e) => { e.preventDefault(); if (canCreate) create(); }}>
        <label>
          <span>Название</span>
          <input className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder="Например: Технология+" maxLength={64} data-autofocus />
        </label>
        <div className="form" style={{ gap: 6 }}>
          <div className="row" style={{ justifyContent: "space-between" }}>
            <span className="muted small">Версия Minecraft{offline ? " · нет сети, список из кеша" : ""}</span>
            <label className="row small" style={{ display: "flex" }}>
              <input type="checkbox" checked={snapshots} onChange={(e) => setSnapshots(e.target.checked)} /> снапшоты
            </label>
          </div>
          <input className="field" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Найти версию…" aria-label="Поиск версии" />
          <div className="vlist" role="listbox" aria-label="Версии">
            {versions === null && <div className="muted small" style={{ padding: 10 }}><span className="spinner" /> Загружаем список версий…</div>}
            {shown.map((v) => (
              <button type="button" key={v.id} role="option" aria-selected={v.id === version} className={v.id === version ? "on" : ""} onClick={() => setVersion(v.id)}>
                <span>{v.id}{v.type !== "release" ? <span className="muted"> · {v.type}</span> : null}</span>
                <span className="muted small">{v.releaseTime.slice(0, 10)}</span>
              </button>
            ))}
          </div>
          {java && <span className="muted small">{version}: {java}</span>}
        </div>
        <label>
          <span>Загрузчик</span>
          <select className="field" value={loader} onChange={(e) => setLoader(e.target.value as LoaderKind)}>
            {LOADERS.map((l) => (
              <option key={l.kind} value={l.kind} disabled={!l.ready}>{loaderName[l.kind]}{l.note ? ` — ${l.note}` : ""}</option>
            ))}
          </select>
        </label>
        {loader !== "vanilla" && (
          <label>
            <span>Версия {loaderName[loader]}</span>
            {loaderVersions === null ? (
              <span className="muted small"><span className="spinner" /> Проверяем совместимость…</span>
            ) : incompatible ? (
              <span className="badge err">{loaderName[loader]} не поддерживает Minecraft {version}</span>
            ) : (
              <select className="field" value={loaderVersion} onChange={(e) => setLoaderVersion(e.target.value)}>
                {loaderVersions.map((v) => (
                  <option key={v.version} value={v.version}>{v.version}{v.stable ? " · стабильная" : " · нестабильная"}</option>
                ))}
              </select>
            )}
          </label>
        )}
        {error && <div className="alert err"><span>{error}</span></div>}
        <div className="acts">
          <button type="button" className="btn" onClick={onClose}>Отмена</button>
          <button type="submit" className="btn pri" disabled={!canCreate}>{busy ? "Создаём…" : "Создать"}</button>
        </div>
      </form>
    </Modal>
  );
}
