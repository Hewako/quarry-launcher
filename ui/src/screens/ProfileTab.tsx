// Вкладка «Профиль»: оформление, место на диске, копия профиля и перенос настроек из другого.
import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, errText, importer, metaOf, type Channel, sizeText, SHARABLE, type PackUpdatePlan, type CloneParts, type Instance, type ProfileMeta } from "../api";
import { Modal, useDialog } from "../ui/Dialog";
import { path, useStreamer } from "../streamer";
import { t, tc } from "../i18n";

const COLORS = ["#4f8a5b", "#3b7dd8", "#8b5cf6", "#d0703a", "#c2415d", "#2a9d8f", "#a3a3a3"];

function CloneDialog({ inst, onClose, onDone }: { inst: Instance; onClose: () => void; onDone: (i: Instance) => void }) {
  const dialog = useDialog();
  const [name, setName] = useState(t("{0} (копия)", inst.name));
  const [parts, setParts] = useState<CloneParts>({ mods: true, configs: true, resources: true, worlds: false });
  const [busy, setBusy] = useState(false);
  const opts: [keyof CloneParts, string][] = [
    ["mods", t("Моды")],
    ["configs", t("Настройки модов и игры, список серверов")],
    ["resources", t("Ресурспаки и шейдеры")],
    ["worlds", t("Миры (займёт больше места)")],
  ];
  const go = async () => {
    setBusy(true);
    try { onDone(await api.cloneInstance(inst.id, name, parts)); } catch (e) { await dialog.alert(t("Копия не создана"), errText(e)); } finally { setBusy(false); }
  };
  return (
    <Modal title={t("Копия профиля")} onClose={onClose}>
      <label className="small muted" htmlFor="clone-name">{t("Имя новой копии")}</label>
      <input id="clone-name" className="field" style={{ width: "100%" }} value={name} onChange={(e) => setName(e.target.value)} />
      <p className="small muted">{t("Версия игры, загрузчик и настройки запуска копируются всегда. Что ещё взять:")}</p>
      {opts.map(([k, label]) => (
        <label key={k} className="row"><input type="checkbox" checked={parts[k]} onChange={(e) => setParts({ ...parts, [k]: e.target.checked })} /> {label}</label>
      ))}
      <div className="acts">
        <button className="btn" onClick={onClose}>{t("Отмена")}</button>
        <button className="btn pri" disabled={busy || !name.trim()} onClick={go}>{busy ? t("Копируем…") : t("Создать копию")}</button>
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
      await dialog.alert(t("Перенесено"), t("Перед переносом сохранён снимок — вернуть как было можно на вкладке «Снимки»."));
    } catch (e) { await dialog.alert(t("Не перенесено"), errText(e)); } finally { setBusy(false); }
  };
  return (
    <Modal title={t("Перенести в «{0}»", inst.name)} onClose={onClose}>
      {others.length === 0 ? <p className="muted">{t("Других профилей нет.")}</p> : (
        <>
          <label className="small muted" htmlFor="copy-from">{t("Из профиля")}</label>
          <select id="copy-from" className="field" style={{ width: "100%" }} value={from} onChange={(e) => setFrom(e.target.value)}>
            {others.map((i) => <option key={i.id} value={i.id}>{i.name} · {i.game_version}</option>)}
          </select>
          <p className="small muted">{t("Файлы с такими же именами заменятся. Ресурспаки, шейдеры и настройки модов добавятся к уже имеющимся.")}</p>
          {SHARABLE.map(([k, label]) => (
            <label key={k} className="row">
              <input type="checkbox" checked={items.includes(k)} onChange={(e) => setItems(e.target.checked ? [...items, k] : items.filter((x) => x !== k))} /> {label}
            </label>
          ))}
        </>
      )}
      <div className="acts">
        <button className="btn" onClick={onClose}>{t("Отмена")}</button>
        <button className="btn pri" disabled={busy || !from || items.length === 0} onClick={go}>{busy ? t("Переносим…") : t("Перенести")}</button>
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
  const [moving, setMoving] = useState(false);
  const [update, setUpdate] = useState<{ path: string; plan: PackUpdatePlan; off: string[]; versionId?: string; changelog?: string } | null>(null);
  const [checking, setChecking] = useState(false);
  const checkPack = async () => {
    setChecking(true);
    try {
      const v = await importer.check(inst.id);
      if (!v) { await dialog.alert(t("Обновлений нет"), t("Установлена последняя версия сборки в выбранном канале.")); return; }
      const d = await importer.download(inst.pack!.project_id!, v.version_id);
      setUpdate({ path: d.path!, plan: await importer.updatePlan(inst.id, d.path!), off: [], versionId: v.version_id, changelog: v.changelog });
    } catch (e) { await dialog.alert(t("Сборку не обновить"), errText(e)); } finally { setChecking(false); }
  };
  const packSettings = async (channel: Channel, pinned: boolean) => {
    try { onChanged(await importer.settings(inst.id, channel, pinned)); } catch (e) { await dialog.alert(t("Не сохранено"), errText(e)); }
  };
  const [updating, setUpdating] = useState(false);
  const exportPack = async () => {
    const path = await saveDialog({ title: t("Экспорт в .mrpack"), defaultPath: `${inst.name}.mrpack`, filters: [{ name: ".mrpack", extensions: ["mrpack"] }] }).catch(() => null);
    if (!path) return;
    try {
      const r = await importer.exportMrpack(inst.id, path, inst.pack?.name ?? inst.name, inst.pack?.version ?? "1.0");
      await dialog.alert(t("Сборка сохранена"), <>
        <p>{t("Модов ссылками на Modrinth: {0}. Рядом сохранён отчёт с лицензиями:", r.linked)} <span className="mono small">{r.report_path}</span></p>
        {r.bundled.length > 0 && <p className="small" style={{ color: "var(--warn)" }}>{t("Внутрь архива вложены файлы без источника — перед публикацией проверьте их лицензии:")} {r.bundled.map((b) => b.split("/").pop()).join(", ")}</p>}
      </>);
    } catch (e) { await dialog.alert(t("Не сохранено"), errText(e)); }
  };
  const pickUpdate = async () => {
    const path = await open({ title: t("Новая версия сборки"), filters: [{ name: ".mrpack", extensions: ["mrpack"] }] }).catch(() => null);
    if (typeof path !== "string") return;
    try { setUpdate({ path, plan: await importer.updatePlan(inst.id, path), off: [] }); } catch (e) { await dialog.alert(t("Сборку не обновить"), errText(e)); }
  };
  const applyUpdate = async () => {
    if (!update) return;
    setUpdating(true);
    try {
      await importer.updateApply(inst.id, update.path, update.off, update.versionId);
      setUpdate(null);
      onChanged({ ...inst, pack: inst.pack && { ...inst.pack, version: update.plan.to, version_id: update.versionId ?? inst.pack.version_id } });
      await dialog.alert(t("Сборка обновлена"), t("Перед обновлением сохранён снимок — вернуть как было можно на вкладке «Снимки и миры»."));
    } catch (e) { await dialog.alert(t("Сборку не обновить"), errText(e)); } finally { setUpdating(false); }
  };
  const streamer = useStreamer();
  const groups = [...new Set(instances.map((i) => i.group).filter(Boolean))] as string[];

  useEffect(() => { api.diskUsage(inst.id).then(setUsage).catch(() => setUsage(null)); }, [inst.id]);

  const save = async (patch: Partial<ProfileMeta> = {}, newName = name) => {
    const next = { ...meta, ...patch };
    try {
      onChanged(await api.updateInstance(inst.id, { name: newName, meta: next }));
      setMeta(next);
    } catch (e) { await dialog.alert(t("Не сохранено"), errText(e)); }
  };

  const move = async (toQuarry: boolean) => {
    let target: string | null = null;
    if (!toQuarry) {
      const picked = await open({ directory: true, title: t("Куда перенести папку игры") }).catch(() => null);
      if (typeof picked !== "string") return;
      target = picked;
    }
    const ok = await dialog.confirm({
      title: toQuarry ? t("Вернуть папку игры в Quarry?") : t("Перенести папку игры?"),
      body: toQuarry
        ? t("Миры, моды и настройки переедут обратно в папку данных Quarry.")
        : <>{t("Миры, моды и настройки переедут в")} <span className="mono">{target}</span>{t(". На другой диск это займёт время; исходная папка удалится только после полной проверки копии.")}</>,
      confirm: t("Перенести"),
    });
    if (!ok) return;
    setMoving(true);
    try {
      const path = await api.moveGameDir(inst.id, target);
      onChanged({ ...inst, external_game_dir: toQuarry ? undefined : path });
      await dialog.alert(t("Перенесено"), <>{t("Папка игры теперь здесь:")} <span className="mono">{path}</span></>);
    } catch (e) { await dialog.alert(t("Не перенесено"), errText(e)); } finally { setMoving(false); }
  };

  return (
    <>
      <div className="card">
        <div className="setting">
          <div className="lbl">{t("Название")}</div>
          <input className="field" value={name} maxLength={64} onChange={(e) => setName(e.target.value)} onBlur={() => name.trim() && name !== inst.name && save()} />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Группа")}<span className="hint">{t("для фильтра на главной")}</span></div>
          <input className="field" list="groups" placeholder={t("Без группы")} value={meta.group ?? ""} maxLength={40}
            onChange={(e) => setMeta({ ...meta, group: e.target.value })} onBlur={() => save()} />
          <datalist id="groups">{groups.map((g) => <option key={g} value={g} />)}</datalist>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Заметка")}</div>
          <input className="field" placeholder={t("Например: сервер с друзьями")} value={meta.description ?? ""} maxLength={300}
            onChange={(e) => setMeta({ ...meta, description: e.target.value })} onBlur={() => save()} />
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Цвет значка")}</div>
          <div className="row" role="radiogroup" aria-label={t("Цвет значка")}>
            <button className={`swatch auto ${!meta.color ? "on" : ""}`} role="radio" aria-checked={!meta.color} title={t("Автоматически")} onClick={() => save({ color: undefined })}>A</button>
            {COLORS.map((c) => (
              <button key={c} className={`swatch ${meta.color === c ? "on" : ""}`} role="radio" aria-checked={meta.color === c} aria-label={c} style={{ background: c }} onClick={() => save({ color: c })} />
            ))}
          </div>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Избранное")}<span className="hint">{t("показывается первым")}</span></div>
          <label className="row"><input type="checkbox" checked={meta.favorite} onChange={(e) => save({ favorite: e.target.checked })} /> {t("в избранном")}</label>
          <span />
        </div>
        <div className="setting">
          <div className="lbl">{t("Архив")}<span className="hint">{t("скрыть с главной, файлы останутся на месте")}</span></div>
          <label className="row"><input type="checkbox" checked={meta.archived} onChange={(e) => save({ archived: e.target.checked })} /> {t("в архиве")}</label>
          <span />
        </div>
      </div>

      <h4 style={{ margin: "22px 0 10px" }}>{t("Место на диске")}{usage ? ` · ${sizeText(usage.total)}` : ""}</h4>
      <div className="card">
        {!usage ? <p className="muted" style={{ margin: 0 }}><span className="spinner" /> {t("Считаем…")}</p> : usage.parts.length === 0 ? <p className="muted" style={{ margin: 0 }}>{t("Профиль пока пустой.")}</p> : (
          <div className="usage">
            {usage.parts.map((p) => (
              <div key={p.label} className="urow">
                <span>{tc(p.label)}</span>
                <div className="bar"><i style={{ width: `${Math.max(1, (p.size / usage.total) * 100)}%` }} /></div>
                <span className="muted small">{sizeText(p.size)}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      {inst.pack && (
        <>
          <h4 style={{ margin: "22px 0 10px" }}>{t("Сборка")}</h4>
          <div className="card row">
            <span><b>{inst.pack.name}</b> {inst.pack.version}</span>
            <span style={{ flex: 1 }} />
            {inst.pack.project_id && (
              <>
                <select className="field auto" aria-label={t("Какие версии сборки предлагать")} value={inst.pack.channel ?? "auto"} onChange={(e) => packSettings(e.target.value as Channel, !!inst.pack?.pinned)}>
                  <option value="auto">{t("Стабильная, если есть")}</option>
                  <option value="release">{t("Только стабильные")}</option>
                  <option value="beta">{t("Бета и стабильные")}</option>
                </select>
                <label className="row small"><input type="checkbox" checked={!!inst.pack.pinned} onChange={(e) => packSettings(inst.pack?.channel ?? "auto", e.target.checked)} /> {t("закрепить версию")}</label>
                <button className="btn" disabled={running || checking || !!inst.pack.pinned} onClick={checkPack}>{checking ? t("Проверяем…") : t("Проверить обновление")}</button>
              </>
            )}
            <button className="btn" disabled={running} onClick={pickUpdate}>{t("Обновить из файла .mrpack…")}</button>
          </div>
        </>
      )}
      <h4 style={{ margin: "22px 0 10px" }}>{t("Где лежит папка игры")}</h4>
      <div className="card">
        <p className="small" style={{ marginTop: 0 }}>
          {inst.external_game_dir ? <span className="mono" style={{ userSelect: "text" }}>{path(inst.external_game_dir, streamer)}</span> : t("В папке данных Quarry.")}
        </p>
        <div className="row">
          <button className="btn" disabled={running || moving} onClick={() => move(false)}>{moving ? t("Переносим…") : t("Перенести в другую папку или на другой диск…")}</button>
          {inst.external_game_dir && <button className="btn" disabled={running || moving} onClick={() => move(true)}>{t("Вернуть в Quarry")}</button>}
        </div>
      </div>

      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn" onClick={() => setModal("clone")}>{t("Сделать копию профиля…")}</button>
        <button className="btn" onClick={exportPack}>{t("Экспорт в .mrpack…")}</button>
        <button className="btn" disabled={running} onClick={() => setModal("copy")}>{t("Перенести настройки из другого профиля…")}</button>
      </div>
      {update && (
        <Modal title={t("Обновить сборку {0} → {1}?", update.plan.from || "?", update.plan.to)} onClose={() => setUpdate(null)} wide>
          {update.changelog && (
            <details open><summary>{t("Что нового в версии")}</summary><pre className="cfgdiff" style={{ whiteSpace: "pre-wrap", userSelect: "text" }}>{update.changelog}</pre></details>
          )}
          {update.plan.game_change && <div className="alert warn"><span>{t("Сборка меняет версию:")} {tc(update.plan.game_change)}</span></div>}
          <div className="diff cfgdiff">
            {update.plan.add.map((f) => <span key={f} className="add">+ {f}</span>)}
            {update.plan.update.map((f) => <span key={f} className="chg">~ {f}</span>)}
            {update.plan.remove.map((f) => <span key={f} className="del">− {f}</span>)}
            {update.plan.add.length + update.plan.update.length + update.plan.remove.length === 0 && <span className="muted">{t("Файлы модов не меняются.")}</span>}
          </div>
          {update.plan.kept_mods.length > 0 && <p className="small">{t("Ваши моды останутся:")} {update.plan.kept_mods.map((m) => m.split("/").pop()).join(", ")}</p>}
          {update.plan.kept_configs.length > 0 && <p className="small">{t("Ваши конфиги не заменятся:")} {update.plan.kept_configs.join(", ")}</p>}
          {update.plan.optional.map((f) => (
            <label key={f} className="row small">
              <input type="checkbox" checked={!update.off.includes(f)} onChange={(e) => setUpdate({ ...update, off: e.target.checked ? update.off.filter((x) => x !== f) : [...update.off, f] })} />
              {t("необязательно:")} <span className="mono">{f.split("/").pop()}</span>
            </label>
          ))}
          <div className="acts">
            <button className="btn" onClick={() => setUpdate(null)}>{t("Отмена")}</button>
            <button className="btn pri" disabled={updating} onClick={applyUpdate}>{updating ? t("Обновляем…") : t("Обновить")}</button>
          </div>
        </Modal>
      )}
      {modal === "clone" && <CloneDialog inst={inst} onClose={() => setModal("")} onDone={(i) => { setModal(""); onCloned(i); }} />}
      {modal === "copy" && <CopyFromDialog inst={inst} instances={instances} onClose={() => setModal("")} />}
    </>
  );
}
