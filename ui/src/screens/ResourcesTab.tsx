// Вкладка «Ресурсы»: ресурспаки (порядок, совместимость с версией игры) и шейдеры (выбор для Iris).
import { useCallback, useEffect, useState } from "react";
import { api, errText, sizeText, snapshots, type Instance, type PackInfo, type ResKind, type WorldInfo } from "../api";
import { useDialog } from "../ui/Dialog";
import { CatalogDialog } from "./ModsTab";
import { t } from "../i18n";

type View = { packs: PackInfo[]; game_format?: number; shader_loader: boolean };

function PackRow({ p, children }: { p: PackInfo; children: React.ReactNode }) {
  return (
    <tr>
      <td style={{ width: 48 }}>{p.icon ? <img className="sicon" src={p.icon} alt="" /> : <div className="sicon" aria-hidden />}</td>
      <td>
        <b>{p.file_name.replace(/\.zip$/i, "")}</b>
        {p.compatible === false && <span className="badge warn" style={{ marginLeft: 6 }} title={t("Сделан для формата {0}–{1}", p.formats?.[0], p.formats?.[1])}>{t("для другой версии игры")}</span>}
        <div className="muted small">{p.description || (p.is_dir ? t("папка") : "")}{p.description || p.is_dir ? " · " : ""}{sizeText(p.size)}</div>
      </td>
      <td style={{ textAlign: "right", whiteSpace: "nowrap" }}>{children}</td>
    </tr>
  );
}

export function ResourcesTab({ inst, running }: { inst: Instance; running: boolean }) {
  const dialog = useDialog();
  const [packs, setPacks] = useState<View | null>(null);
  const [shaders, setShaders] = useState<View | null>(null);
  const [catalog, setCatalog] = useState<ResKind | null>(null);
  const [worlds, setWorlds] = useState<WorldInfo[]>([]);
  const [world, setWorld] = useState("");
  const [datapacks, setDatapacks] = useState<View | null>(null);

  const load = useCallback(() => {
    api.resources(inst.id, "resourcepack").then(setPacks).catch(() => setPacks({ packs: [], shader_loader: false }));
    api.resources(inst.id, "shader").then(setShaders).catch(() => setShaders({ packs: [], shader_loader: false }));
  }, [inst.id]);
  useEffect(load, [load]);
  useEffect(() => {
    snapshots.worlds(inst.id).then((w) => { setWorlds(w); setWorld((cur) => cur || w[0]?.folder || ""); }).catch(() => setWorlds([]));
  }, [inst.id]);
  const loadDatapacks = useCallback(() => {
    if (!world) { setDatapacks(null); return; }
    api.resources(inst.id, "datapack", world).then(setDatapacks).catch(() => setDatapacks({ packs: [], shader_loader: false }));
  }, [inst.id, world]);
  useEffect(loadDatapacks, [loadDatapacks]);

  const act = async (f: () => Promise<unknown>) => {
    try { await f(); load(); } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); }
  };
  const enabled = packs?.packs.filter((p) => p.enabled).map((p) => p.file_name) ?? [];
  const setOrder = (next: string[]) => act(() => api.setResourcePacks(inst.id, next));
  const move = (i: number, d: -1 | 1) => {
    const next = [...enabled];
    [next[i], next[i + d]] = [next[i + d], next[i]];
    setOrder(next);
  };
  const enable = async (p: PackInfo) => {
    if (p.compatible === false) {
      const ok = await dialog.confirm({
        title: t("Ресурспак для другой версии игры"),
        body: t("Он может выглядеть неправильно или не загрузиться. Включить всё равно?"),
        confirm: t("Включить"),
      });
      if (!ok) return;
    }
    setOrder([p.file_name, ...enabled]);
  };
  const remove = async (kind: ResKind, p: PackInfo) => {
    const ok = await dialog.confirm({ title: t("Убрать {0}?", p.file_name), body: t("Файл переместится в корзину Lime (папка trash в данных лаунчера) — его можно вернуть вручную."), confirm: t("Убрать"), danger: true });
    if (ok) await act(() => api.removeResource(inst.id, kind, p.file_name, kind === "datapack" ? world : undefined));
    if (ok && kind === "datapack") loadDatapacks();
  };
  const shaderOn = shaders?.packs.find((p) => p.enabled)?.file_name ?? null;

  return (
    <>
      <div className="pagehead" style={{ marginBottom: 14 }}>
        <p className="muted small" style={{ margin: 0, maxWidth: 680 }}>
          {t("Порядок как в игре: верхний ресурспак главный и перекрывает те, что ниже. Менять список можно, пока игра закрыта.")}
          {packs && packs.game_format === undefined && t(" Совместимость проверится после первого запуска этой версии игры.")}
        </p>
        <button className="btn" onClick={() => setCatalog("resourcepack")}>{t("Найти ресурспак…")}</button>
      </div>
      <div className="card" style={{ padding: 0 }}>
        {packs === null ? <p className="muted" style={{ padding: 14 }}><span className="spinner" /> {t("Загружаем…")}</p> : packs.packs.length === 0 ? (
          <p className="muted" style={{ padding: 14, margin: 0 }}>{t("Ресурспаков нет. Найдите их в каталоге или положите .zip в папку resourcepacks.")}</p>
        ) : (
          <table className="servers">
            <tbody>
              {packs.packs.map((p) => {
                const i = enabled.indexOf(p.file_name);
                return (
                  <PackRow key={p.file_name} p={p}>
                    {p.enabled ? (
                      <>
                        <button className="btn link small" aria-label={t("Поднять {0}", p.file_name)} disabled={running || i === 0} onClick={() => move(i, -1)}>{t("↑ выше")}</button>
                        <button className="btn link small" aria-label={t("Опустить {0}", p.file_name)} disabled={running || i === enabled.length - 1} onClick={() => move(i, 1)}>{t("↓ ниже")}</button>
                        <button className="btn" disabled={running} onClick={() => setOrder(enabled.filter((x) => x !== p.file_name))}>{t("Выключить")}</button>
                      </>
                    ) : (
                      <>
                        <button className="btn link small" disabled={running} onClick={() => remove("resourcepack", p)}>{t("Убрать")}</button>
                        <button className="btn pri" disabled={running} onClick={() => enable(p)}>{t("Включить")}</button>
                      </>
                    )}
                  </PackRow>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      <div className="pagehead" style={{ margin: "22px 0 10px" }}>
        <h4 style={{ margin: 0 }}>{t("Шейдеры")}</h4>
        <button className="btn" onClick={() => setCatalog("shader")}>{t("Найти шейдер…")}</button>
      </div>
      {shaders && !shaders.shader_loader && (
        <div className="alert warn" style={{ marginBottom: 10 }}>
          <span>{t("Шейдеры работают только с модом Iris (Fabric, Quilt) или Oculus (NeoForge).")} {inst.loader === "vanilla" ? t("В профиле без загрузчика модов шейдеры не запустятся.") : t("Установите Iris на вкладке «Моды».")}</span>
        </div>
      )}
      <div className="card" style={{ padding: 0 }}>
        {shaders === null ? <p className="muted" style={{ padding: 14 }}><span className="spinner" /> {t("Загружаем…")}</p> : shaders.packs.length === 0 ? (
          <p className="muted" style={{ padding: 14, margin: 0 }}>{t("Шейдеров нет.")}</p>
        ) : (
          <table className="servers">
            <tbody>
              <tr>
                <td /><td><label className="row"><input type="radio" name="shader" checked={shaderOn === null} disabled={running} onChange={() => act(() => api.setShader(inst.id, null))} /> {t("Без шейдеров")}</label></td>
                <td />
              </tr>
              {shaders.packs.map((p) => (
                <PackRow key={p.file_name} p={{ ...p, description: "" }}>
                  <label className="row" style={{ display: "inline-flex" }}>
                    <input type="radio" name="shader" checked={p.enabled} disabled={running} onChange={() => act(() => api.setShader(inst.id, p.file_name))} /> {t("выбран")}
                  </label>
                  <button className="btn link small" disabled={running || p.enabled} onClick={() => remove("shader", p)}>{t("Убрать")}</button>
                </PackRow>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <div className="pagehead" style={{ margin: "22px 0 10px" }}>
        <h4 style={{ margin: 0 }}>{t("Датапаки мира")}</h4>
        <div className="row">
          {worlds.length > 0 && (
            <select className="field auto" aria-label={t("Мир")} value={world} onChange={(e) => setWorld(e.target.value)}>
              {worlds.map((w) => <option key={w.folder} value={w.folder}>{w.name}</option>)}
            </select>
          )}
          <button className="btn" disabled={!world} onClick={() => setCatalog("datapack")}>{t("Найти датапак…")}</button>
        </div>
      </div>
      <div className="card" style={{ padding: 0 }}>
        {worlds.length === 0 ? <p className="muted" style={{ padding: 14, margin: 0 }}>{t("Датапаки ставятся в мир. Миров пока нет — создайте мир в игре.")}</p>
          : datapacks === null ? <p className="muted" style={{ padding: 14 }}><span className="spinner" /> {t("Загружаем…")}</p>
          : datapacks.packs.length === 0 ? <p className="muted" style={{ padding: 14, margin: 0 }}>{t("В этом мире датапаков нет. Новые игра включит сама при входе в мир.")}</p>
          : (
            <table className="servers">
              <tbody>
                {datapacks.packs.map((p) => (
                  <PackRow key={p.file_name} p={p}>
                    <button className="btn link small" disabled={running} onClick={() => remove("datapack", p)}>{t("Убрать")}</button>
                  </PackRow>
                ))}
              </tbody>
            </table>
          )}
      </div>
      {catalog && <CatalogDialog inst={inst} initial="" kinds={[catalog]} world={catalog === "datapack" ? world : undefined} onClose={() => setCatalog(null)} onInstalled={catalog === "datapack" ? loadDatapacks : load} />}
    </>
  );
}
