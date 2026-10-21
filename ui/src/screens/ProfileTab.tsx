// Вкладка «Профиль»: оформление, место на диске, копия профиля и перенос настроек из другого.
import { useEffect, useState } from "react";
import { api, errText, metaOf, sizeText, SHARABLE, type CloneParts, type Instance, type ProfileMeta } from "../api";
import { Modal, useDialog } from "../ui/Dialog";

const COLORS = ["#4f8a5b", "#3b7dd8", "#8b5cf6", "#d0703a", "#c2415d", "#2a9d8f", "#a3a3a3"];

function CloneDialog({ inst, onClose, onDone }: { inst: Instance; onClose: () => void; onDone: (i: Instance) => void }) {
  const dialog = useDialog();
  const [name, setName] = useState(`${inst.name} (копия)`);
  const [parts, setParts] = useState<CloneParts>({ mods: true, configs: true, resources: true, worlds: false });
  const [busy, setBusy] = useState(false);
  const opts: [keyof CloneParts, string][] = [
    ["mods", "Моды"],
    ["configs", "Настройки модов и игры, список серверов"],
    ["resources", "Ресурспаки и шейдеры"],
    ["worlds", "Миры (займёт больше места)"],
  ];
  const go = async () => {
    setBusy(true);
    try { onDone(await api.cloneInstance(inst.id, name, parts)); } catch (e) { await dialog.alert("Копия не создана", errText(e)); } finally { setBusy(false); }
  };
  return (
    <Modal title="Копия профиля" onClose={onClose}>
      <label className="small muted" htmlFor="clone-name">Имя новой копии</label>
      <input id="clone-name" className="field" style={{ width: "100%" }} value={name} onChange={(e) => setName(e.target.value)} />
      <p className="small muted">Версия игры, загрузчик и настройки запуска копируются всегда. Что ещё взять:</p>
      {opts.map(([k, label]) => (
        <label key={k} className="row"><input type="checkbox" checked={parts[k]} onChange={(e) => setParts({ ...parts, [k]: e.target.checked })} /> {label}</label>
      ))}
      <div className="acts">
        <button className="btn" onClick={onClose}>Отмена</button>
        <button className="btn pri" disabled={busy || !name.trim()} onClick={go}>{busy ? "Копируем…" : "Создать копию"}</button>
      </div>
    </Modal>
  );
}

function CopyFromDialog({ inst, instances, onClose }: { inst: Instance; instances: Instance[]; onClose: () => void }) {
  const dialog = useDialog();
  const others = instances.filter((i) => i.id !== inst.id);
  const [from, setFrom] = useState(others[0]?.id ?? "");
  const [items, setItems] = useState<string[]>(["options.txt"]);
  const [busy, setBusy] = useState(false);
  const go = async () => {
    setBusy(true);
    try {
      await api.copyBetween(from, inst.id, items);
      onClose();
      await dialog.alert("Перенесено", "Перед переносом сохранён снимок — вернуть как было можно на вкладке «Снимки».");
    } catch (e) { await dialog.alert("Не перенесено", errText(e)); } finally { setBusy(false); }
  };
  return (
    <Modal title={`Перенести в «${inst.name}»`} onClose={onClose}>
      {others.length === 0 ? <p className="muted">Других профилей нет.</p> : (
        <>
          <label className="small muted" htmlFor="copy-from">Из профиля</label>
          <select id="copy-from" className="field" style={{ width: "100%" }} value={from} onChange={(e) => setFrom(e.target.value)}>
            {others.map((i) => <option key={i.id} value={i.id}>{i.name} · {i.game_version}</option>)}
          </select>
          <p className="small muted">Файлы с такими же именами заменятся. Ресурспаки, шейдеры и настройки модов добавятся к уже имеющимся.</p>
          {SHARABLE.map(([k, label]) => (
            <label key={k} className="row">
              <input type="checkbox" checked={items.includes(k)} onChange={(e) => setItems(e.target.checked ? [...items, k] : items.filter((x) => x !== k))} /> {label}
            </label>
          ))}
        </>
      )}
      <div className="acts">
        <button className="btn" onClick={onClose}>Отмена</button>
        <button className="btn pri" disabled={busy || !from || items.length === 0} onClick={go}>{busy ? "Переносим…" : "Перенести"}</button>
      </div>
    </Modal>
  );
}

export function ProfileTab({ inst, instances, running, onChanged, onCloned }: {
  inst: Instance;
  instances: Instance[];
  running: boolean;
  onChanged: (i: Instance) => void;
  onCloned: (i: Instance) => void;
}) {
  const dialog = useDialog();
  const [meta, setMeta] = useState<ProfileMeta>(metaOf(inst));
  const [name, setName] = useState(inst.name);
  const [usage, setUsage] = useState<{ parts: { label: string; size: number }[]; total: number } | null>(null);
  const [modal, setModal] = useState<"" | "clone" | "copy">("");
  const groups = [...new Set(instances.map((i) => i.group).filter(Boolean))] as string[];

  useEffect(() => { api.diskUsage(inst.id).then(setUsage).catch(() => setUsage(null)); }, [inst.id]);

  const save = async (patch: Partial<ProfileMeta> = {}, newName = name) => {
    const next = { ...meta, ...patch };
    try {
      onChanged(await api.updateInstance(inst.id, { name: newName, meta: next }));
      setMeta(next);
    } catch (e) { await dialog.alert("Не сохранено", errText(e)); }
  };

  return (
    <>
      <div className="card">
        <div className="setting">
          <div className="lbl">Название</div>
          <input className="field" value={name} maxLength={64} onChange={(e) => setName(e.target.value)} onBlur={() => name.trim() && name !== inst.name && save()} />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Группа<span className="hint">для фильтра на главной</span></div>
          <input className="field" list="groups" placeholder="Без группы" value={meta.group ?? ""} maxLength={40}
            onChange={(e) => setMeta({ ...meta, group: e.target.value })} onBlur={() => save()} />
          <datalist id="groups">{groups.map((g) => <option key={g} value={g} />)}</datalist>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Заметка</div>
          <input className="field" placeholder="Например: сервер с друзьями" value={meta.description ?? ""} maxLength={300}
            onChange={(e) => setMeta({ ...meta, description: e.target.value })} onBlur={() => save()} />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Цвет значка</div>
          <div className="row" role="radiogroup" aria-label="Цвет значка">
            <button className={`swatch auto ${!meta.color ? "on" : ""}`} role="radio" aria-checked={!meta.color} title="Автоматически" onClick={() => save({ color: undefined })}>A</button>
            {COLORS.map((c) => (
              <button key={c} className={`swatch ${meta.color === c ? "on" : ""}`} role="radio" aria-checked={meta.color === c} aria-label={c} style={{ background: c }} onClick={() => save({ color: c })} />
            ))}
          </div>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Избранное<span className="hint">показывается первым</span></div>
          <label className="row"><input type="checkbox" checked={meta.favorite} onChange={(e) => save({ favorite: e.target.checked })} /> в избранном</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">Архив<span className="hint">скрыть с главной, файлы останутся на месте</span></div>
          <label className="row"><input type="checkbox" checked={meta.archived} onChange={(e) => save({ archived: e.target.checked })} /> в архиве</label>
          <span />
        </div>
      </div>

      <h4 style={{ margin: "22px 0 10px" }}>Место на диске{usage ? ` · ${sizeText(usage.total)}` : ""}</h4>
      <div className="card">
        {!usage ? <p className="muted" style={{ margin: 0 }}><span className="spinner" /> Считаем…</p> : usage.parts.length === 0 ? <p className="muted" style={{ margin: 0 }}>Профиль пока пустой.</p> : (
          <div className="usage">
            {usage.parts.map((p) => (
              <div key={p.label} className="urow">
                <span>{p.label}</span>
                <div className="bar"><i style={{ width: `${Math.max(1, (p.size / usage.total) * 100)}%` }} /></div>
                <span className="muted small">{sizeText(p.size)}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn" onClick={() => setModal("clone")}>Сделать копию профиля…</button>
        <button className="btn" disabled={running} onClick={() => setModal("copy")}>Перенести настройки из другого профиля…</button>
      </div>
      {modal === "clone" && <CloneDialog inst={inst} onClose={() => setModal("")} onDone={(i) => { setModal(""); onCloned(i); }} />}
      {modal === "copy" && <CopyFromDialog inst={inst} instances={instances} onClose={() => setModal("")} />}
    </>
  );
}
