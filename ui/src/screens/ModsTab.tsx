// Вкладка «Моды · Mod Doctor»: проверка сборки, список модов, каталог Modrinth, обновления.
// Любое изменение модов — через предпросмотр и автоматический снимок.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useCallback, useEffect, useMemo, useState } from "react";
import { errText, mods, sizeText, type Change, type DoctorReport, type Finding, type Instance, type JournalEntry, type ModFile, type ModState, type Plan, type ProjectKind, type SearchHit } from "../api";
import { Modal, useDialog } from "../ui/Dialog";

const stateCls: Record<ModState, string> = { ok: "ok", warning: "warn", blocked: "err", disabled: "", unknown: "warn" };
const sideName: Record<string, string> = { client: "клиент", server: "сервер", both: "обе", unknown: "—" };
const sourceName: Record<string, string> = { modrinth: "Modrinth", curseforge: "CurseForge", import: "импорт", manual: "вручную" };

/** Предпросмотр изменений с галочками. Перед применением создаётся снимок. */
export function PlanDialog({ title, plan, onClose, onApply }: { title: string; plan: Plan; onClose: () => void; onApply: (p: Plan) => Promise<void> }) {
  const [on, setOn] = useState<boolean[]>(plan.changes.map(() => true));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const chosen = plan.changes.filter((_, i) => on[i]);
  const total = chosen.reduce((s, c) => s + c.size, 0);
  const apply = async () => {
    setBusy(true);
    setError("");
    try { await onApply({ ...plan, changes: chosen }); } catch (e) { setError(errText(e)); setBusy(false); }
  };
  return (
    <Modal title={title} onClose={onClose} wide>
      {plan.changes.length === 0 ? <p className="muted">Изменений нет.</p> : (
        <div className="planlist">
          {plan.changes.map((c, i) => (
            <label key={i}>
              <input type="checkbox" checked={on[i]} onChange={(e) => setOn(on.map((v, j) => (j === i ? e.target.checked : v)))} />
              <div>
                <b>{c.kind === "add" ? "+ " : c.kind === "update" ? "~ " : "− "}{c.title}</b>{" "}
                <span className="muted small">{c.old_version ? `${c.old_version} → ` : ""}{c.new_version ?? ""}</span>
                <small>{c.reason}{c.size ? ` · ${sizeText(c.size)}` : ""}</small>
              </div>
            </label>
          ))}
        </div>
      )}
      {plan.notes.map((n, i) => <span key={i} className="muted small">• {n}</span>)}
      <span className="muted small">Перед изменением будет создан снимок — отменить можно на вкладке «Снимки». Файлы скачиваются с Modrinth и проверяются по хэшу.</span>
      {error && <div className="alert err"><span>{error}</span></div>}
      <div className="acts">
        <button className="btn" onClick={onClose}>Отмена</button>
        <button className="btn pri" disabled={busy || chosen.length === 0} onClick={apply}>
          {busy ? "Применяем…" : `Применить${chosen.length ? ` (${chosen.length}${total ? `, ${sizeText(total)}` : ""})` : ""}`}
        </button>
      </div>
    </Modal>
  );
}

function CatalogDialog({ inst, initial, onClose, onPlan }: { inst: Instance; initial: string; onClose: () => void; onPlan: (title: string, p: Plan) => void }) {
  const [query, setQuery] = useState(initial);
  const [kind, setKind] = useState<ProjectKind>("mod");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  useEffect(() => {
    const t = setTimeout(() => {
      setError("");
      mods.search(inst.id, query, kind).then((r) => setHits(r.hits)).catch((e) => { setHits([]); setError(errText(e)); });
    }, 300);
    return () => clearTimeout(t);
  }, [inst.id, query, kind]);
  const pick = async (h: SearchHit) => {
    setBusy(h.project_id);
    setError("");
    try { onPlan(`Установить ${h.title}`, await mods.planInstall(inst.id, h.project_id)); }
    catch (e) { setError(errText(e)); }
    finally { setBusy(null); }
  };
  return (
    <Modal title="Каталог Modrinth" onClose={onClose} wide>
      <div className="row">
        <input className="field" style={{ flex: 1 }} value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Найти мод…" data-autofocus />
        <div className="seg">
          {([["mod", "Моды"], ["resourcepack", "Ресурспаки"], ["shader", "Шейдеры"]] as const).map(([k, l]) => (
            <button key={k} className={kind === k ? "on" : ""} onClick={() => setKind(k)}>{l}</button>
          ))}
        </div>
      </div>
      <span className="muted small">Показано то, что подходит к Minecraft {inst.game_version}{kind === "mod" ? ` и ${inst.loader}` : ""}. Ставится только по вашему нажатию.</span>
      {kind !== "mod" && <span className="small" style={{ color: "var(--warn)" }}>Ресурспаки и шейдеры пока можно только найти — установка появится позже. Сейчас ставятся моды.</span>}
      {error && <div className="alert err"><span>{error}</span></div>}
      <div className="hits">
        {hits === null && <p className="muted"><span className="spinner" /> Ищем…</p>}
        {hits?.length === 0 && !error && <p className="muted">Ничего не нашлось.</p>}
        {hits?.map((h) => (
          <div className="hit" key={h.project_id}>
            {h.icon_url ? <img src={h.icon_url} alt="" /> : <div className="noicon" />}
            <div className="grow">
              <b>{h.title}</b> <span className="muted small">· {h.author} · {h.downloads.toLocaleString("ru-RU")} скачиваний</span>
              <small>{h.description}</small>
            </div>
            <button className="btn" disabled={kind !== "mod" || busy !== null} onClick={() => pick(h)}>{busy === h.project_id ? "…" : "Установить"}</button>
          </div>
        ))}
      </div>
    </Modal>
  );
}

function FindingCard({ f, files, onToggle, onSearch }: { f: Finding; files: ModFile[]; onToggle: (file: string, enabled: boolean) => void; onSearch: (q: string) => void }) {
  const name = (file: string) => files.find((x) => x.file_name === file)?.mods[0]?.name ?? file;
  return (
    <div className={`alert ${f.severity === "blocking" ? "err" : "warn"}`}>
      <div className="h">{f.severity === "blocking" ? "✖" : "⚠"} {f.title}</div>
      <span className="small">{f.detail}</span>
      <span className="muted small">Основание: {f.evidence}</span>
      {f.fixes.length > 0 && (
        <div className="acts">
          {f.fixes.map((x, i) =>
            x.kind === "install" ? (
              <button key={i} className="btn pri" onClick={() => onSearch(x.mod_id)}>Найти «{x.mod_id}» в каталоге</button>
            ) : x.kind === "disable" ? (
              <button key={i} className="btn" onClick={() => onToggle(x.file_name, false)}>Выключить {name(x.file_name)}</button>
            ) : (
              x.file_names.map((keep) => (
                <button key={keep} className="btn" onClick={() => x.file_names.filter((o) => o !== keep).forEach((o) => onToggle(o, false))}>Оставить {keep}</button>
              ))
            ),
          )}
        </div>
      )}
      {f.fixes.some((x) => x.kind === "disable" || x.kind === "keep_one") && <span className="muted small">Выключенный мод остаётся в профиле, запись попадает в журнал.</span>}
    </div>
  );
}

export function ModsTab({ inst, running }: { inst: Instance; running: boolean }) {
  const dialog = useDialog();
  const [data, setData] = useState<{ files: ModFile[]; report: DoctorReport; journal: JournalEntry[] } | null>(null);
  const [error, setError] = useState("");
  const [filter, setFilter] = useState("");
  const [catalog, setCatalog] = useState<string | null>(null);
  const [plan, setPlan] = useState<{ title: string; plan: Plan; reason: string } | null>(null);
  const [checking, setChecking] = useState(false);
  // Доступные обновления: проверяются сами при открытии вкладки (один запрос к Modrinth).
  const [available, setAvailable] = useState<Plan | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [dragging, setDragging] = useState(false);

  const load = useCallback(() => { mods.view(inst.id).then((d) => { setData(d); setError(""); }).catch((e) => setError(errText(e))); }, [inst.id]);
  useEffect(load, [load]);
  const refreshUpdates = useCallback(() => {
    if (inst.loader === "vanilla") return;
    mods.checkUpdates(inst.id).then(setAvailable).catch(() => setAvailable(null));
  }, [inst.id, inst.loader]);
  useEffect(refreshUpdates, [refreshUpdates]);

  // Перетаскивание JAR-файлов в окно.
  useEffect(() => {
    if (inst.loader === "vanilla" || running) return;
    let off: (() => void) | undefined;
    let alive = true;
    let webview: ReturnType<typeof getCurrentWebview>;
    try { webview = getCurrentWebview(); } catch { return; } // в предпросмотре без Tauri окна нет
    webview
      .onDragDropEvent(async (e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          const jars = e.payload.paths.filter((p) => p.toLowerCase().endsWith(".jar"));
          if (jars.length === 0) { await dialog.alert("Не JAR-файлы", "Перетащите файлы модов с расширением .jar."); return; }
          try {
            const added = await mods.addFiles(inst.id, jars);
            load();
            refreshUpdates();
            const unknown = added.filter((a) => !a.known);
            await dialog.alert(
              `Добавлено модов: ${added.length}`,
              unknown.length === 0
                ? "Все файлы опознаны в каталоге Modrinth. Перед добавлением создан снимок."
                : `Не найдены в каталоге Modrinth: ${unknown.map((u) => u.title).join(", ")}. Происхождение этих файлов неизвестно — ставьте моды только из надёжных источников (через заражённые моды распространялся вирус Fractureiser). Перед добавлением создан снимок.`,
            );
          } catch (err) {
            await dialog.alert("Файлы не добавлены", errText(err));
          }
        }
      })
      .then((u) => { if (alive) off = u; else u(); })
      .catch(() => {});
    return () => { alive = false; off?.(); };
  }, [inst.id, inst.loader, running, dialog, load, refreshUpdates]);

  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return (data?.files ?? []).filter((f) => !q || f.file_name.toLowerCase().includes(q) || f.mods.some((m) => m.name.toLowerCase().includes(q) || m.id.includes(q)));
  }, [data, filter]);

  if (inst.loader === "vanilla") return <div className="card empty"><p>В профиле без загрузчика моды не работают. Создайте профиль с Fabric, Quilt или NeoForge.</p></div>;
  if (error) return <div className="alert err"><span>{error}</span></div>;
  if (!data) return <p className="muted"><span className="spinner" /> Проверяем моды…</p>;

  const toggle = async (file: string, enabled: boolean) => {
    try { await mods.toggle(inst.id, file, enabled); load(); } catch (e) { await dialog.alert("Не получилось", errText(e)); }
  };
  const remove = async (f: ModFile) => {
    const world = f.world_content
      ? " Этот мод добавляет в мир блоки, измерения или генерацию: без него часть построек и чанков может пропасть. Quarry сначала сделает бэкап последнего мира."
      : "";
    const ok = await dialog.confirm({ title: `Удалить ${f.mods[0]?.name ?? f.file_name}?`, body: "Перед удалением будет создан снимок — вернуть мод можно на вкладке «Снимки». Если сомневаетесь, лучше выключите мод." + world, confirm: "Удалить", danger: true });
    if (!ok) return;
    try { await mods.remove(inst.id, f.file_name); load(); } catch (e) { await dialog.alert("Не получилось", errText(e)); }
  };
  const updates = async () => {
    setChecking(true);
    try {
      const p = await mods.checkUpdates(inst.id);
      setAvailable(p);
      const chosen = picked.size ? p.changes.filter((c) => picked.has(base(c.old_file ?? ""))) : p.changes;
      setPlan({ title: picked.size ? "Обновить выбранные" : "Обновления модов", plan: { ...p, changes: chosen }, reason: "Перед обновлением модов" });
    }
    catch (e) { await dialog.alert("Не удалось проверить обновления", errText(e)); }
    finally { setChecking(false); }
  };
  const pin = async (f: ModFile) => {
    try { await mods.pin(inst.id, f.file_name, !f.source?.pinned); load(); refreshUpdates(); } catch (e) { await dialog.alert("Не получилось", errText(e)); }
  };
  const r = data.report;
  const base = (name: string) => name.replace(/\.disabled$/, "");
  const updateFor = (f: ModFile): Change | undefined => available?.changes.find((c) => base(c.old_file ?? "") === f.file_name);
  const updateCount = available?.changes.length ?? 0;
  const stateOf = (f: ModFile) => r.mods.find((m) => m.file_name === f.file_name);

  return (
    <>
      {running && <div className="alert warn"><span className="small">Игра запущена — менять моды можно после её закрытия.</span></div>}
      {dragging && <div className="alert info"><div className="h">Отпустите, чтобы добавить моды в «{inst.name}»</div><span className="small">Перед добавлением будет создан снимок.</span></div>}
      <div className="findings">
        {r.findings.map((f, i) => <FindingCard key={i} f={f} files={data.files} onToggle={toggle} onSearch={(q) => setCatalog(q)} />)}
      </div>
      <div className="summary">
        <div className="kpi"><b>{r.total}</b><span>модов в профиле</span></div>
        <div className="kpi ok"><b>{r.ok}</b><span>конфликтов не найдено</span></div>
        <div className="kpi warn"><b>{r.warnings}</b><span>есть предупреждение</span></div>
        <div className="kpi err"><b>{r.blocked}</b><span>блокирует запуск</span></div>
      </div>
      <div className="modbar">
        <input className="field" style={{ maxWidth: 260 }} placeholder="Найти мод…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <span style={{ flex: 1 }} />
        <span className="muted small">{available === null ? "обновления проверяются…" : updateCount ? `обновления есть у ${updateCount}` : "обновлений нет"}</span>
        <button className="btn" disabled={running || checking || updateCount === 0} onClick={updates}>
          {checking ? "Проверяем…" : picked.size ? `Обновить выбранные (${picked.size})… (предпросмотр)` : "Обновить всё… (предпросмотр)"}
        </button>
        <button className="btn pri" disabled={running} onClick={() => setCatalog("")}>+ Добавить мод</button>
      </div>
      {data.files.length === 0 ? (
        <div className="card empty"><p>Модов пока нет. Добавьте их из каталога Modrinth или перетащите JAR-файлы в окно.</p></div>
      ) : (
        <div className="card" style={{ padding: 0, overflow: "auto" }}>
          <table className="modtable">
            <thead><tr><th style={{ width: 44 }} /><th>Мод</th><th>Версия</th><th>Доступна версия</th><th>Сторона</th><th>Источник</th><th>Слой</th><th>Состояние</th><th /></tr></thead>
            <tbody>
              {shown.map((f) => {
                const st = stateOf(f);
                const m = f.mods[0];
                return (
                  <tr key={f.rel_path} className={f.enabled ? "" : "off"}>
                    <td><button className={`tog ${f.enabled ? "on" : ""}`} disabled={running} aria-label={f.enabled ? "Выключить" : "Включить"} onClick={() => toggle(f.file_name, !f.enabled)} /></td>
                    <td><b>{m?.name ?? f.file_name}</b>{(st?.notes.length ?? 0) > 0 && <small>{st!.notes[0]}</small>}{!m && <small>{f.file_name}</small>}</td>
                    <td>{m?.version ?? "?"}{f.source?.pinned && <small style={{ color: "var(--warn)" }}>закреплена</small>}</td>
                    <td>{(() => {
                      const u = updateFor(f);
                      if (!u) return <span className="muted">—</span>;
                      return (
                        <label className="row" style={{ gap: 6, flexWrap: "nowrap" }}>
                          <input type="checkbox" checked={picked.has(f.file_name)} onChange={(e) => { const n = new Set(picked); if (e.target.checked) n.add(f.file_name); else n.delete(f.file_name); setPicked(n); }} />
                          <span className="upd" style={{ color: "var(--blue)" }}>→ {u.new_version}</span>
                        </label>
                      );
                    })()}</td>
                    <td>{sideName[m?.side ?? "unknown"]}</td>
                    <td>{f.source ? sourceName[f.source.source] : "вручную"}</td>
                    <td className="layer">{f.source?.layer === "base" ? "сборка" : "мои моды"}</td>
                    <td>{st && <span className={`badge ${stateCls[st.state]}`}>{st.state_label}</span>}</td>
                    <td style={{ whiteSpace: "nowrap" }}>
                      <button className="btn link small" disabled={running} onClick={() => pin(f)} title="Закреплённая версия не обновляется">{f.source?.pinned ? "Открепить" : "Закрепить"}</button>{" · "}
                      <button className="btn link small" disabled={running} onClick={() => remove(f)}>Удалить</button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <p className="muted small">Состояние — по метаданным модов (fabric.mod.json, quilt.mod.json, mods.toml), а не по имени файла. «Конфликтов не найдено» не гарантирует, что игра запустится: настоящую проверку даёт запуск.</p>
      {data.journal.length > 0 && (
        <details>
          <summary className="muted small" style={{ cursor: "pointer" }}>Журнал изменений ({data.journal.length})</summary>
          <ul className="small" style={{ margin: "8px 0", paddingLeft: 18 }}>
            {data.journal.map((j, i) => <li key={i}>{new Date(j.at * 1000).toLocaleString("ru-RU")} · <b>{j.action}</b> {j.what} <span className="muted">— {j.reason}</span></li>)}
          </ul>
        </details>
      )}
      {catalog !== null && <CatalogDialog inst={inst} initial={catalog} onClose={() => setCatalog(null)} onPlan={(title, p) => { setCatalog(null); setPlan({ title, plan: p, reason: `Перед: ${title.toLowerCase()}` }); }} />}
      {plan && (
        <PlanDialog title={plan.title} plan={plan.plan} onClose={() => setPlan(null)} onApply={async (p) => { await mods.apply(inst.id, p, plan.reason); setPlan(null); setPicked(new Set()); load(); refreshUpdates(); }} />
      )}
    </>
  );
}
