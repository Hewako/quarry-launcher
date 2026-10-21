// Вкладка «Моды · Mod Doctor»: проверка сборки, список модов, каталог Modrinth, обновления.
// Любое изменение модов — через предпросмотр и автоматический снимок.
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, errText, mods, sizeText, type Bisect, type CatalogSource, type Channel, type Change, type DoctorReport, type Finding, type Instance, type JournalEntry, type ModFile, type ModState, type Plan, type ProjectKind, type SearchHit } from "../api";
import { Modal, useDialog } from "../ui/Dialog";
import { t } from "../i18n";

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
                <small>{c.reason}{c.size ? ` · ${sizeText(c.size)}` : ""}</small>
              </div>
            </label>
          ))}
        </div>
      )}
      {plan.notes.map((n, i) => <span key={i} className="muted small">• {n}</span>)}
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

/** Загрузчик и игра — не моды профиля, в зависимостях их не показываем. */
const BUILTIN_DEPS = new Set(["minecraft", "java", "fabricloader", "quilt_loader", "neoforge", "forge", "fml"]);

/** Кто кому нужен: обязательные зависимости между модами профиля. */
function DepsView({ files }: { files: ModFile[] }) {
  const owner = new Map<string, string>();
  for (const f of files) for (const m of f.mods) owner.set(m.id, m.name || f.file_name);
  const rows = files.filter((f) => f.mods.length > 0).map((f) => {
    const ids = new Set(f.mods.map((m) => m.id));
    const needs = [...new Set(f.mods.flatMap((m) => m.dependencies.filter((d) => d.kind === "required" && !BUILTIN_DEPS.has(d.id)).map((d) => d.id)))];
    const usedBy = files.filter((g) => g !== f && g.mods.some((m) => m.dependencies.some((d) => d.kind === "required" && ids.has(d.id)))).map((g) => g.mods[0]?.name ?? g.file_name);
    return { name: f.mods[0].name || f.file_name, needs, usedBy };
  }).filter((r) => r.needs.length || r.usedBy.length);
  if (rows.length === 0) return <p className="muted small">{t("Связей между модами нет: каждый работает сам по себе.")}</p>;
  return (
    <table className="servers">
      <tbody>
        {rows.map((r) => (
          <tr key={r.name}>
            <td><b>{r.name}</b></td>
            <td className="small">
              {r.needs.length > 0 && <div>{t("нужны:")} {r.needs.map((id, i) => <span key={id}>{i > 0 && ", "}{owner.has(id) ? owner.get(id) : <span style={{ color: "var(--err)" }}>{id} ({t("нет в профиле")})</span>}</span>)}</div>}
              {r.usedBy.length > 0 && <div className="muted">{t("нужен для:")} {r.usedBy.join(", ")}</div>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

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
  // CurseForge подключён только для модов.
  const src: CatalogSource = kind === "mod" ? source : "modrinth";
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
      else { await api.installResource(inst.id, h.project_id, kind, world); setDone((d) => [...d, h.project_id]); onInstalled?.(); }
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
        {cf && kind === "mod" && (
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
      <div className="h">{f.severity === "blocking" ? "✖" : "⚠"} {f.title}</div>
      <span className="small">{f.detail}</span>
      <span className="muted small">{t("Основание:")} {f.evidence}</span>
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

  return (
    <>
      {running && <div className="alert warn"><span className="small">{t("Игра запущена — менять моды можно после её закрытия.")}</span></div>}
      {dragging && <div className="alert info"><div className="h">{t("Отпустите, чтобы добавить моды в «")}{inst.name}»</div><span className="small">{t("Перед добавлением будет создан снимок.")}</span></div>}
      <div className="findings">
        {r.findings.map((f, i) => <FindingCard key={i} f={f} files={data.files} onToggle={toggle} onSearch={(q) => setCatalog(q)} />)}
      </div>
      <div className="summary">
        <div className="kpi"><b>{r.total}</b><span>{t("модов в профиле")}</span></div>
        <div className="kpi ok"><b>{r.ok}</b><span>{t("конфликтов не найдено")}</span></div>
        <div className="kpi warn"><b>{r.warnings}</b><span>{t("есть предупреждение")}</span></div>
        <div className="kpi err"><b>{r.blocked}</b><span>{t("блокирует запуск")}</span></div>
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
                  disabled={running} onChange={(e) => toggleGroup(g, e.target.checked)} /> {t(g)} <span className="muted">{on}/{files.length}</span>
              </label>
            );
          })}
          {!bisect && data.files.filter((f) => f.enabled).length > 1 && <button className="btn link small" disabled={running} onClick={() => bisectStep("start")}>{t("Найти виновника краша…")}</button>}
          <button className="btn link small" disabled={grouping} onClick={autoGroups}>{grouping ? t("Раскладываем…") : t("Разложить автоматически")}</button>
        </div>
      )}
      <div className="modbar">
        <input className="field" style={{ maxWidth: 260 }} placeholder={t("Найти мод…")} aria-label={t("Найти мод…")} value={filter} onChange={(e) => setFilter(e.target.value)} />
        <span style={{ flex: 1 }} />
        <span className="muted small">{available === null ? t("обновления проверяются…") : updateCount ? t("обновления есть у {0}", updateCount) : t("обновлений нет")}</span>
        <button className="btn" disabled={running || checking || updateCount === 0} onClick={updates}>
          {checking ? t("Проверяем…") : picked.size ? t("Обновить выбранные ({0})… (предпросмотр)", picked.size) : t("Обновить всё… (предпросмотр)")}
        </button>
        <button className="btn pri" disabled={running} onClick={() => setCatalog("")}>{t("+ Добавить мод")}</button>
      </div>
      {data.files.length === 0 ? (
        <div className="card empty"><p>{t("Модов пока нет. Добавьте их из каталога Modrinth или перетащите JAR-файлы в окно.")}</p></div>
      ) : (
        <div className="card" style={{ padding: 0, overflow: "auto" }}>
          <table className="modtable">
            <thead><tr><th style={{ width: 44 }} /><th>{t("Мод")}</th><th>{t("Версия")}</th><th>{t("Доступна версия")}</th><th>{t("Группа")}</th><th>{t("Сторона")}</th><th>{t("Источник")}</th><th>{t("Слой")}</th><th>{t("Состояние")}</th><th /></tr></thead>
            <tbody>
              {shown.map((f) => {
                const st = stateOf(f);
                const m = f.mods[0];
                return (
                  <tr key={f.rel_path} className={f.enabled ? "" : "off"}>
                    <td><button className={`tog ${f.enabled ? "on" : ""}`} disabled={running} aria-label={f.enabled ? t("Выключить") : t("Включить")} onClick={() => toggle(f.file_name, !f.enabled)} /></td>
                    <td><b>{m?.name ?? f.file_name}</b>{(st?.notes.length ?? 0) > 0 && <small>{st!.notes[0]}</small>}{!m && <small>{f.file_name}</small>}</td>
                    <td>{m?.version ?? "?"}{f.source?.pinned && <small style={{ color: "var(--warn)" }}>{t("закреплена")}</small>}</td>
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
                    <td>
                      <select className="field auto mini" aria-label={t("Группа для {0}", m?.name ?? f.file_name)} value={groups.groups[f.file_name] ?? ""}
                        onChange={(e) => mods.setGroup(inst.id, f.file_name, e.target.value || null).then(load).catch((er) => dialog.alert(t("Не получилось"), errText(er)))}>
                        <option value="">—</option>
                        {groups.names.map((g) => <option key={g} value={g}>{t(g)}</option>)}
                      </select>
                    </td>
                    <td>{sideName[m?.side ?? "unknown"]}</td>
                    <td>{f.source ? sourceName[f.source.source] : t("вручную")}</td>
                    <td className="layer">{f.source?.layer === "base" ? t("сборка") : t("мои моды")}</td>
                    <td>{st && <span className={`badge ${stateCls[st.state]}`}>{t(st.state_label)}</span>}</td>
                    <td style={{ whiteSpace: "nowrap" }}>
                      <button className="btn link small" disabled={running} onClick={() => pin(f)} title={t("Закреплённая версия не обновляется")}>{f.source?.pinned ? t("Открепить") : t("Закрепить")}</button>{" · "}
                      <button className="btn link small" disabled={running} onClick={() => remove(f)}>{t("Удалить")}</button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      {data.files.length > 0 && (
        <details className="card" style={{ marginTop: 12 }}>
          <summary>{t("Зависимости между модами")}</summary>
          <DepsView files={data.files} />
        </details>
      )}
      <p className="muted small">{t("Состояние — по метаданным модов (fabric.mod.json, quilt.mod.json, mods.toml), а не по имени файла. «Конфликтов не найдено» не гарантирует, что игра запустится: настоящую проверку даёт запуск.")}</p>
      {data.journal.length > 0 && (
        <details>
          <summary className="muted small" style={{ cursor: "pointer" }}>{t("Журнал изменений (")}{data.journal.length})</summary>
          <ul className="small" style={{ margin: "8px 0", paddingLeft: 18 }}>
            {data.journal.map((j, i) => <li key={i}>{new Date(j.at * 1000).toLocaleString("ru-RU")} · <b>{j.action}</b> {j.what} <span className="muted">— {j.reason}</span></li>)}
          </ul>
        </details>
      )}
      {catalog !== null && <CatalogDialog inst={inst} initial={catalog} onClose={() => setCatalog(null)} onPlan={(title, p) => { setCatalog(null); setPlan({ title, plan: p, reason: t("Перед: {0}", title.toLowerCase()) }); }} />}
      {plan && (
        <PlanDialog title={plan.title} plan={plan.plan} onClose={() => setPlan(null)} onApply={async (p) => { await mods.apply(inst.id, p, plan.reason); setPlan(null); setPicked(new Set()); load(); refreshUpdates(); }} />
      )}
    </>
  );
}
