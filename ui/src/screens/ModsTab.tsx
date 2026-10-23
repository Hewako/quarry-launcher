// Вкладка «Моды · Mod Doctor»: проверка сборки, список модов, каталог Modrinth, обновления.
// Любое изменение модов — через предпросмотр и автоматический снимок.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errText, mods, sizeText, type Bisect, type CatalogSource, type Channel, type Change, type DoctorReport, type Finding, type Instance, type JournalEntry, type ModFile, type ModState, type Plan, type ProjectKind, type SearchHit } from "../api";
import { Modal, useDialog } from "../ui/Dialog";
import { t, tc } from "../i18n";
import { typing } from "./InstancePage";

const stateCls: Record<ModState, string> = { ok: "ok", warning: "warn", blocked: "err", disabled: "", unknown: "warn" };
const sideName: Record<string, string> = { client: t("клиент"), server: t("сервер"), both: t("обе"), unknown: "—" };
const sourceName: Record<string, string> = { modrinth: "Modrinth", curseforge: "CurseForge", import: t("импорт"), manual: t("вручную") };

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
      {plan.changes.length === 0 ? <p className="muted">{t("Изменений нет.")}</p> : (
        <div className="planlist">
          {plan.changes.map((c, i) => (
            <label key={i}>
              <input type="checkbox" checked={on[i]} onChange={(e) => setOn(on.map((v, j) => (j === i ? e.target.checked : v)))} />
              <div>
                <b>{c.kind === "add" ? "+ " : c.kind === "update" ? "~ " : "− "}{c.title}</b>{" "}
                <span className="muted small">{c.old_version ? `${c.old_version} → ` : ""}{c.new_version ?? ""}</span>
                <small>{tc(c.reason)}{c.size ? ` · ${sizeText(c.size)}` : ""}</small>
              </div>
            </label>
          ))}
        </div>
      )}
      {plan.notes.map((n, i) => <span key={i} className="muted small">• {tc(n)}</span>)}
      <span className="muted small">{t("Перед изменением будет создан снимок — отменить можно на вкладке «Снимки». Файлы скачиваются с Modrinth и проверяются по хэшу.")}</span>
      {error && <div className="alert err"><span>{error}</span></div>}
      <div className="acts">
        <button className="btn" onClick={onClose}>{t("Отмена")}</button>
        <button className="btn pri" disabled={busy || chosen.length === 0} onClick={apply}>
          {busy ? t("Применяем…") : t("Применить{0}", chosen.length ? ` (${chosen.length}${total ? `, ${sizeText(total)}` : ""})` : "")}
        </button>
      </div>
    </Modal>
  );
}

/** Категории Modrinth для фильтра каталога (подписи — для игрока). */
const MOD_CATS: [string, string][] = [["optimization", t("Оптимизация")], ["utility", t("Удобство")], ["decoration", t("Декор")], ["worldgen", t("Генерация мира")], ["technology", t("Техника")], ["magic", t("Магия")], ["adventure", t("Приключения")], ["storage", t("Хранение")], ["mobs", t("Мобы")], ["equipment", t("Снаряжение")], ["food", t("Еда")], ["transportation", t("Транспорт")], ["game-mechanics", t("Механики")], ["management", t("Управление")], ["library", t("Библиотеки")]];
const CATS: Record<ProjectKind, [string, string][]> = {
  mod: MOD_CATS,
  datapack: MOD_CATS,
  resourcepack: [["vanilla-like", t("Как ванилла")], ["realistic", t("Реалистичные")], ["simplistic", t("Простые")], ["themed", t("Тематические")], ["tweaks", t("Мелкие правки")], ["16x", "16x"], ["32x", "32x"], ["64x", "64x"]],
  shader: [["vanilla-like", t("Как ванилла")], ["semi-realistic", t("Полуреалистичные")], ["realistic", t("Реалистичные")], ["fantasy", t("Сказочные")], ["potato", t("Для слабых ПК")], ["low", t("Лёгкие")], ["high", t("Тяжёлые")]],
};
const CHANNELS: [Channel, string][] = [["auto", t("Стабильная, если есть")], ["release", t("Только стабильные")], ["beta", t("Бета и стабильные")], ["alpha", t("Любые, включая альфа")]];

const KINDS: [ProjectKind, string][] = [["mod", t("Моды")], ["resourcepack", t("Ресурспаки")], ["shader", t("Шейдеры")], ["datapack", t("Датапаки")]];

export function CatalogDialog({ inst, initial, onClose, onPlan, kinds = ["mod", "resourcepack", "shader"], onInstalled, world }: {
  world?: string;
  inst: Instance;
  initial: string;
  onClose: () => void;
  onPlan?: (title: string, p: Plan) => void;
  kinds?: ProjectKind[];
  onInstalled?: () => void;
}) {
  const [query, setQuery] = useState(initial);
  const [kind, setKind] = useState<ProjectKind>(kinds[0]);
  const [done, setDone] = useState<string[]>([]);
  const [category, setCategory] = useState("");
  const [channel, setChannel] = useState<Channel>("auto");
  const [source, setSource] = useState<CatalogSource>("modrinth");
  const [cf, setCf] = useState(false);
  useEffect(() => { api.appInfo().then((i) => setCf(i.curseforge)).catch(() => {}); }, []);
  const src: CatalogSource = cf ? source : "modrinth";
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  useEffect(() => {
    const timer = setTimeout(() => {
      setError("");
      mods.search(inst.id, query, kind, 0, src === "modrinth" ? category : "", src).then((r) => setHits(r.hits)).catch((e) => { setHits([]); setError(errText(e)); });
    }, 300);
    return () => clearTimeout(timer);
  }, [inst.id, query, kind, category, src]);
  const pick = async (h: SearchHit) => {
    setBusy(h.project_id);
    setError("");
    try {
      if (kind === "mod") onPlan?.(t("Установить {0}", h.title), await mods.planInstall(inst.id, h.project_id, channel, src));
      else { await api.installResource(inst.id, h.project_id, kind, world, src); setDone((d) => [...d, h.project_id]); onInstalled?.(); }
    } catch (e) { setError(errText(e)); }
    finally { setBusy(null); }
  };
  return (
    <Modal title={src === "curseforge" ? t("Каталог CurseForge") : t("Каталог Modrinth")} onClose={onClose} wide>
      <div className="row">
        <input className="field" style={{ flex: 1 }} value={query} onChange={(e) => setQuery(e.target.value)} placeholder={{ mod: t("Найти мод…"), shader: t("Найти шейдер…"), resourcepack: t("Найти ресурспак…"), datapack: t("Найти датапак…") }[kind]} data-autofocus />
        <div className="seg">
          {KINDS.filter(([k]) => kinds.includes(k)).map(([k, l]) => (
            <button key={k} className={kind === k ? "on" : ""} onClick={() => { setKind(k); setCategory(""); }}>{l}</button>
          ))}
        </div>
      </div>
      <div className="row">
        {cf && (
          <div className="seg" role="group" aria-label={t("Источник")}>
            <button className={source === "modrinth" ? "on" : ""} onClick={() => setSource("modrinth")}>Modrinth</button>
            <button className={source === "curseforge" ? "on" : ""} onClick={() => { setSource("curseforge"); setCategory(""); }}>CurseForge</button>
          </div>
        )}
        {src === "modrinth" && <select className="field auto" aria-label={t("Категория")} value={category} onChange={(e) => setCategory(e.target.value)}>
          <option value="">{t("Все категории")}</option>
          {CATS[kind].map(([v, l]) => <option key={v} value={v}>{l}</option>)}
        </select>}
        {kind === "mod" && (
          <select className="field auto" aria-label={t("Какие версии ставить")} value={channel} onChange={(e) => setChannel(e.target.value as Channel)}>
            {CHANNELS.map(([v, l]) => <option key={v} value={v}>{t("Версии:")} {l.toLowerCase()}</option>)}
          </select>
        )}
      </div>
      <span className="muted small">{t("Показано то, что подходит к Minecraft")} {inst.game_version}{kind === "mod" ? t(" и {0}", inst.loader) : ""}{t(". Ставится только по вашему нажатию.")}</span>
      {kind === "shader" && <span className="muted small">{t("Шейдеры работают с модом Iris (Fabric/Quilt) или Oculus (NeoForge). После установки шейдер можно выбрать на вкладке «Ресурсы».")}</span>}
      {error && <div className="alert err"><span>{error}</span></div>}
      <div className="hits">
        {hits === null && <p className="muted"><span className="spinner" /> {t("Ищем…")}</p>}
        {hits?.length === 0 && !error && <p className="muted">{t("Ничего не нашлось.")}</p>}
        {hits?.map((h) => (
          <div className="hit" key={h.project_id}>
            {h.icon_url ? <img src={h.icon_url} alt="" /> : <div className="noicon" />}
            <div className="grow">
              <b>{h.title}</b> <span className="muted small">· {h.author} · {h.downloads.toLocaleString("ru-RU")} {t("скачиваний")}</span>
              <small>{h.description}</small>
            </div>
            <button className="btn" disabled={busy !== null || done.includes(h.project_id)} onClick={() => pick(h)}>{busy === h.project_id ? "…" : done.includes(h.project_id) ? t("Установлено") : t("Установить")}</button>
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
      <div className="h">{f.severity === "blocking" ? "✖" : "⚠"} {tc(f.title)}</div>
      <span className="small">{tc(f.detail)}</span>
      <span className="muted small">{t("Основание:")} {tc(f.evidence)}</span>
      {f.fixes.length > 0 && (
        <div className="acts">
          {f.fixes.map((x, i) =>
            x.kind === "install" ? (
              <button key={i} className="btn pri" onClick={() => onSearch(x.mod_id)}>{t("Найти «")}{x.mod_id}{t("» в каталоге")}</button>
            ) : x.kind === "disable" ? (
              <button key={i} className="btn" onClick={() => onToggle(x.file_name, false)}>{t("Выключить")} {name(x.file_name)}</button>
            ) : (
              x.file_names.map((keep) => (
                <button key={keep} className="btn" onClick={() => x.file_names.filter((o) => o !== keep).forEach((o) => onToggle(o, false))}>{t("Оставить")} {keep}</button>
              ))
            ),
          )}
        </div>
      )}
      {f.fixes.some((x) => x.kind === "disable" || x.kind === "keep_one") && <span className="muted small">{t("Выключенный мод остаётся в профиле, запись попадает в журнал.")}</span>}
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

  const [groups, setGroups] = useState<{ names: string[]; groups: Record<string, string> }>({ names: [], groups: {} });
  const [grouping, setGrouping] = useState(false);
  const [bisect, setBisect] = useState<Bisect | null>(null);
  const [chip, setChip] = useState<"all" | "issues" | "updates" | "off">("all");
  const [selected, setSelected] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);
  // «/» — к поиску модов (как подсказано в поле).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "/" && !typing(e)) { e.preventDefault(); search.current?.focus(); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const load = useCallback(() => {
    mods.view(inst.id).then((d) => { setData(d); setError(""); }).catch((e) => setError(errText(e)));
    mods.groups(inst.id).then(setGroups).catch(() => {});
    mods.bisectStatus(inst.id).then(setBisect).catch(() => setBisect(null));
  }, [inst.id]);
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
          if (jars.length === 0) { await dialog.alert(t("Не JAR-файлы"), t("Перетащите файлы модов с расширением .jar.")); return; }
          try {
            const added = await mods.addFiles(inst.id, jars);
            load();
            refreshUpdates();
            const unknown = added.filter((a) => !a.known);
            await dialog.alert(
              t("Добавлено модов: {0}", added.length),
              unknown.length === 0
                ? t("Все файлы опознаны в каталоге Modrinth. Перед добавлением создан снимок.")
                : t("Не найдены в каталоге Modrinth: {0}. Происхождение этих файлов неизвестно — ставьте моды только из надёжных источников (через заражённые моды распространялся вирус Fractureiser). Перед добавлением создан снимок.", unknown.map((u) => u.title).join(", ")),
            );
          } catch (err) {
            await dialog.alert(t("Файлы не добавлены"), errText(err));
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

  if (inst.loader === "vanilla") return <div className="card empty"><p>{t("В профиле без загрузчика моды не работают. Создайте профиль с Fabric, Quilt или NeoForge.")}</p></div>;
  if (error) return <div className="alert err"><span>{error}</span></div>;
  if (!data) return <p className="muted"><span className="spinner" /> {t("Проверяем моды…")}</p>;

  const bisectStep = async (action: "start" | "answer" | "cancel", crashed?: boolean) => {
    if (action === "start" && !(await dialog.confirm({
      title: t("Найти мод, из-за которого падает игра?"),
      body: t("Лаунчер будет по очереди выключать половину модов (вместе с теми, кому они нужны). После каждого шага запустите игру и ответьте, повторился ли краш. Обычно хватает 3–6 запусков. Перед началом сохраняется снимок."),
      confirm: t("Начать"),
    }))) return;
    try {
      const r = await mods.bisectStep(inst.id, action, crashed);
      load();
      if (r?.kind === "found") {
        const off = await dialog.confirm({
          title: t("Вероятный виновник: {0}", r.file),
          body: t("Все моды снова включены. Выключить этот мод? Его можно будет включить обратно в любой момент."),
          confirm: t("Выключить"),
          cancel: t("Оставить"),
        });
        if (off) await toggle(r.file, false);
      }
    } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); load(); }
  };
  const toggleGroup = async (g: string, enabled: boolean) => {
    try { await mods.toggleGroup(inst.id, g, enabled); load(); } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); }
  };
  const autoGroups = async () => {
    setGrouping(true);
    try {
      const n = await mods.autoGroups(inst.id);
      load();
      await dialog.alert(t("Готово"), n ? t("Разложено по группам: {0}. Остальные можно отнести к группе вручную в колонке «Группа».", n) : t("Новых модов, которые можно разложить автоматически, нет. Отнесите их к группе вручную в колонке «Группа»."));
    } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); } finally { setGrouping(false); }
  };
  const toggle = async (file: string, enabled: boolean) => {
    try { await mods.toggle(inst.id, file, enabled); load(); } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); }
  };
  const remove = async (f: ModFile) => {
    const world = f.world_content
      ? t(" Этот мод добавляет в мир блоки, измерения или генерацию: без него часть построек и чанков может пропасть. Quarry сначала сделает бэкап последнего мира.")
      : "";
    const ok = await dialog.confirm({ title: t("Удалить {0}?", f.mods[0]?.name ?? f.file_name), body: t("Перед удалением будет создан снимок — вернуть мод можно на вкладке «Снимки». Если сомневаетесь, лучше выключите мод.") + world, confirm: t("Удалить"), danger: true });
    if (!ok) return;
    try { await mods.remove(inst.id, f.file_name); load(); } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); }
  };
  const updates = async () => {
    setChecking(true);
    try {
      const p = await mods.checkUpdates(inst.id);
      setAvailable(p);
      const chosen = picked.size ? p.changes.filter((c) => picked.has(base(c.old_file ?? ""))) : p.changes;
      setPlan({ title: picked.size ? t("Обновить выбранные") : t("Обновления модов"), plan: { ...p, changes: chosen }, reason: t("Перед обновлением модов") });
    }
    catch (e) { await dialog.alert(t("Не удалось проверить обновления"), errText(e)); }
    finally { setChecking(false); }
  };
  const pin = async (f: ModFile) => {
    try { await mods.pin(inst.id, f.file_name, !f.source?.pinned); load(); refreshUpdates(); } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); }
  };
  const r = data.report;
  const base = (name: string) => name.replace(/\.disabled$/, "");
  const updateFor = (f: ModFile): Change | undefined => available?.changes.find((c) => base(c.old_file ?? "") === f.file_name);
  const updateCount = available?.changes.length ?? 0;
  const stateOf = (f: ModFile) => r.mods.find((m) => m.file_name === f.file_name);

  const issue = (f: ModFile) => { const s = stateOf(f)?.state; return s === "blocked" || s === "warning"; };
  const counts = { all: data.files.length, issues: data.files.filter(issue).length, updates: data.files.filter((f) => updateFor(f)).length, off: data.files.filter((f) => !f.enabled).length };
  const visible = shown.filter((f) => chip === "all" || (chip === "issues" ? issue(f) : chip === "updates" ? !!updateFor(f) : !f.enabled));
  const rowsIssues = visible.filter(issue);
  const rowsRest = visible.filter((f) => !issue(f));
  const sel = data.files.find((f) => f.file_name === selected) ?? rowsIssues[0] ?? visible[0];
  const CHIPS: [typeof chip, string, number][] = [["all", t("Все"), counts.all], ["issues", t("Замечания"), counts.issues], ["updates", t("Обновления"), counts.updates], ["off", t("Выключены"), counts.off]];
  const row = (f: ModFile) => {
    const st = stateOf(f);
    const m = f.mods[0];
    const u = updateFor(f);
    return (
      <tr key={f.rel_path} className={`${f.enabled ? "" : "off"} ${sel?.file_name === f.file_name ? "sel" : ""}`} onClick={() => setSelected(f.file_name)}>
        <td><button className={`tog ${f.enabled ? "on" : ""}`} disabled={running} aria-label={`${f.enabled ? t("Выключить") : t("Включить")} ${m?.name ?? f.file_name}`} onClick={(e) => { e.stopPropagation(); toggle(f.file_name, !f.enabled); }} /></td>
        <td><button className="rowname" aria-pressed={sel?.file_name === f.file_name} onClick={() => setSelected(f.file_name)}><b>{m?.name ?? f.file_name}</b>{(st?.notes.length ?? 0) > 0 && <span className="muted"> {tc(st!.notes[0])}</span>}</button></td>
        <td className="mono">{m?.version ?? "?"}{u && <span className="upd"> → {u.new_version}</span>}{f.source?.pinned && <span className="muted" title={t("закреплена")}> 🔒</span>}</td>
        <td>{f.source ? sourceName[f.source.source] : t("вручную")}</td>
        <td>{st && <span className={`state ${stateCls[st.state]}`}>{tc(st.state_label)}</span>}</td>
      </tr>
    );
  };
  // Карточка выбранного мода: почему мешает (с кнопками починки), сведения и действия.
  const card = (f: ModFile) => {
    const m = f.mods[0];
    const u = updateFor(f);
    const needs = m ? data.files.filter((o) => o.mods.some((x) => x.dependencies.some((d) => d.id === m.id && d.kind === "required"))).map((o) => o.mods[0]?.name ?? o.file_name) : [];
    return (
      <aside className="card modcard" aria-label={t("Карточка мода")}>
        <h3>{m?.name ?? f.file_name} {m?.version && <span className="mono muted">{m.version}</span>}</h3>
        <p className="muted small">{f.file_name}</p>
        {r.findings.filter((x) => x.files.includes(f.file_name)).map((x, i) => <FindingCard key={i} f={x} files={data.files} onToggle={toggle} onSearch={(q) => setCatalog(q)} />)}
        <dl className="kv">
          <dt>{t("Источник")}</dt><dd>{f.source ? sourceName[f.source.source] : t("вручную")}</dd>
          <dt>{t("Сторона")}</dt><dd>{sideName[m?.side ?? "unknown"]}</dd>
          <dt>{t("Слой")}</dt><dd>{f.source?.layer === "base" ? t("сборка") : t("мои моды")}</dd>
          <dt>{t("Нужен для")}</dt><dd>{needs.length ? needs.join(", ") : t("никто не требует")}</dd>
          <dt>{t("Размер")}</dt><dd>{sizeText(f.size)}</dd>
          <dt>{t("Группа")}</dt>
          <dd>
            <select className="field auto mini" aria-label={t("Группа для {0}", m?.name ?? f.file_name)} value={groups.groups[f.file_name] ?? ""}
              onChange={(e) => mods.setGroup(inst.id, f.file_name, e.target.value || null).then(load).catch((er) => dialog.alert(t("Не получилось"), errText(er)))}>
              <option value="">—</option>
              {groups.names.map((g) => <option key={g} value={g}>{tc(g)}</option>)}
            </select>
          </dd>
          {u && <><dt>{t("Доступна версия")}</dt><dd>
            <label className="row" style={{ gap: 6, justifyContent: "flex-end" }}>
              <input type="checkbox" checked={picked.has(f.file_name)} onChange={(e) => { const n = new Set(picked); if (e.target.checked) n.add(f.file_name); else n.delete(f.file_name); setPicked(n); }} />
              <span className="mono upd">{u.new_version}</span>
            </label>
          </dd></>}
        </dl>
        <div className="acts">
          <button className={`btn ${f.enabled ? "cta" : ""}`} disabled={running} onClick={() => toggle(f.file_name, !f.enabled)}>{f.enabled ? t("Выключить") : t("Включить")}</button>
          <button className="btn" disabled={running} onClick={() => remove(f)}>{t("Удалить")}</button>
          <button className="btn" disabled={running} onClick={() => pin(f)} title={t("Закреплённая версия не обновляется")}>{f.source?.pinned ? t("Открепить") : t("Закрепить")}</button>
          {f.source?.source === "modrinth" && f.source.project_id && <button className="btn" onClick={() => api.openLink(`https://modrinth.com/mod/${f.source!.project_id}`).catch((e) => dialog.alert(t("Не получилось"), errText(e)))}>{t("На Modrinth")}</button>}
        </div>
      </aside>
    );
  };

  return (
    <>
      {running && <div className="alert warn"><span className="small">{t("Игра запущена — менять моды можно после её закрытия.")}</span></div>}
      {dragging && <div className="alert info"><div className="h">{t("Отпустите, чтобы добавить моды в «")}{inst.name}»</div><span className="small">{t("Перед добавлением будет создан снимок.")}</span></div>}
      {/* Замечания без файла (например, не хватает мода) — над таблицей; остальные — в карточке мода. */}
      <div className="findings">
        {r.findings.filter((f) => f.files.length === 0).map((f, i) => <FindingCard key={i} f={f} files={data.files} onToggle={toggle} onSearch={(q) => setCatalog(q)} />)}
      </div>
      {bisect && (
        <div className="alert warn" style={{ marginBottom: 12 }}>
          <div className="h">{t("Поиск виновника краша · шаг {0}", bisect.round)}</div>
          <span>{t("Выключено модов: {0}. Подозреваемых осталось: {1}. Запустите игру и ответьте:", bisect.disabled.length, bisect.suspects.length)}</span>
          <div className="row" style={{ marginTop: 8 }}>
            <button className="btn" disabled={running} onClick={() => bisectStep("answer", true)}>{t("Игра снова упала")}</button>
            <button className="btn" disabled={running} onClick={() => bisectStep("answer", false)}>{t("Игра работает")}</button>
            <button className="btn link small" disabled={running} onClick={() => bisectStep("cancel")}>{t("Прекратить поиск")}</button>
          </div>
        </div>
      )}
      <div className="modbar">
        <div className="chips" role="group" aria-label={t("Фильтр модов")}>
          {CHIPS.map(([k, l, n]) => <button key={k} className={`fchip ${chip === k ? "on" : ""}`} aria-pressed={chip === k} onClick={() => setChip(k)}>{l} <span className="muted">{n}</span></button>)}
        </div>
        <span style={{ flex: 1 }} />
        <input ref={search} className="field" style={{ maxWidth: 220 }} placeholder={t("Найти мод…")} aria-label={t("Найти мод…")} aria-keyshortcuts="/" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <button className="btn" disabled={running || checking || updateCount === 0} onClick={updates} title={available === null ? t("обновления проверяются…") : updateCount ? t("обновления есть у {0}", updateCount) : t("обновлений нет")}>
          {checking ? t("Проверяем…") : picked.size ? t("Обновить выбранные ({0})… (предпросмотр)", picked.size) : t("Обновить всё… (предпросмотр)")}
        </button>
        <button className="btn pri" disabled={running} onClick={() => setCatalog("")}>{t("+ Добавить мод")}</button>
      </div>
      {data.files.length > 0 && (
        <div className="groupbar" role="group" aria-label={t("Наборы модов")}>
          <span className="muted small">{t("Наборы:")}</span>
          {groups.names.map((g) => {
            const files = data.files.filter((f) => groups.groups[f.file_name] === g);
            if (files.length === 0) return null;
            const on = files.filter((f) => f.enabled).length;
            return (
              <label key={g} className="chip" title={t("Включено {0} из {1}", on, files.length)}>
                <input type="checkbox" checked={on === files.length} ref={(el) => { if (el) el.indeterminate = on > 0 && on < files.length; }}
                  disabled={running} onChange={(e) => toggleGroup(g, e.target.checked)} /> {tc(g)} <span className="muted">{on}/{files.length}</span>
              </label>
            );
          })}
          {!bisect && data.files.filter((f) => f.enabled).length > 1 && <button className="btn link small" disabled={running} onClick={() => bisectStep("start")}>{t("Найти виновника краша…")}</button>}
          <button className="btn link small" disabled={grouping} onClick={autoGroups}>{grouping ? t("Раскладываем…") : t("Разложить автоматически")}</button>
        </div>
      )}
      {data.files.length === 0 ? (
        <div className="card empty"><p>{t("Модов пока нет. Добавьте их из каталога Modrinth или перетащите JAR-файлы в окно.")}</p></div>
      ) : (
        <div className="modsgrid">
          <div className="card" style={{ padding: 0, overflow: "auto" }}>
            <table className="modtable">
              <thead><tr><th style={{ width: 52 }} /><th>{t("Мод")}</th><th>{t("Версия")}</th><th>{t("Источник")}</th><th>{t("Состояние")}</th></tr></thead>
              <tbody>
                {rowsIssues.length > 0 && <tr className="grp"><td colSpan={5}>{t("Замечания: {0}", rowsIssues.length)}</td></tr>}
                {rowsIssues.map(row)}
                {rowsIssues.length > 0 && rowsRest.length > 0 && <tr className="grp"><td colSpan={5}>{t("Остальные: {0}", rowsRest.length)}</td></tr>}
                {rowsRest.map(row)}
                {visible.length === 0 && <tr><td colSpan={5} className="muted">{t("Ничего не нашлось.")}</td></tr>}
              </tbody>
            </table>
          </div>
          {sel && card(sel)}
        </div>
      )}

      <p className="muted small">{t("Состояние — по метаданным модов (fabric.mod.json, quilt.mod.json, mods.toml), а не по имени файла. «Конфликтов не найдено» не гарантирует, что игра запустится: настоящую проверку даёт запуск.")}</p>
      {catalog !== null && <CatalogDialog inst={inst} initial={catalog} onClose={() => setCatalog(null)} onPlan={(title, p) => { setCatalog(null); setPlan({ title, plan: p, reason: t("Перед: {0}", title.toLowerCase()) }); }} />}
      {plan && (
        <PlanDialog title={plan.title} plan={plan.plan} onClose={() => setPlan(null)} onApply={async (p) => { await mods.apply(inst.id, p, plan.reason); setPlan(null); setPicked(new Set()); load(); refreshUpdates(); }} />
      )}
    </>
  );
}
