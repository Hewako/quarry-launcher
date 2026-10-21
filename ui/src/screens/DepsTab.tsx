// Вкладка «Зависимости»: граф связей между модами профиля — кто кому нужен, что рекомендовано,
// что конфликтует (по метаданным модов и находкам Mod Doctor). Моды без связей скрыты.
import { useEffect, useMemo, useState } from "react";
import { errText, mods, type DoctorReport, type JournalEntry, type ModFile, type Instance } from "../api";
import { t } from "../i18n";

/** Загрузчик и игра — не моды профиля, в связях их не показываем. */
const BUILTIN = new Set(["minecraft", "java", "fabricloader", "quilt_loader", "neoforge", "forge", "fml"]);

type Kind = "req" | "opt" | "bad";
interface Node { id: string; name: string; lib: boolean }
interface Edge { from: string; to: string; kind: Kind; why?: string }
type View = "all" | "around" | "problems";

function buildGraph(files: ModFile[], report: DoctorReport, journal: JournalEntry[]) {
  const owner = new Map<string, string>();
  for (const f of files) for (const m of f.mods) owner.set(m.id, f.file_name);
  const name = (f: ModFile) => `${f.mods[0]?.name ?? f.file_name} ${f.mods[0]?.version ?? ""}`.trim();
  const edges: Edge[] = [];
  const missing = new Map<string, string[]>();
  for (const f of files.filter((x) => x.enabled)) {
    for (const d of f.mods.flatMap((m) => m.dependencies)) {
      if (BUILTIN.has(d.id)) continue;
      const to = owner.get(d.id);
      if (!to) { if (d.kind === "required") missing.set(f.file_name, [...(missing.get(f.file_name) ?? []), d.id]); continue; }
      if (to === f.file_name) continue;
      // Обязательная зависимость, по которой Mod Doctor нашёл блокирующую проблему, — конфликт.
      const bad = report.findings.find((fd) => fd.severity === "blocking" && fd.files.includes(f.file_name) && fd.title.toLowerCase().includes(d.id));
      const kind: Kind = d.kind === "breaks" || d.kind === "conflicts" || bad ? "bad" : d.kind === "required" ? "req" : "opt";
      edges.push({ from: f.file_name, to, kind, why: bad ? `${bad.title}. ${bad.detail}` : undefined });
    }
  }
  for (const fd of report.findings.filter((x) => x.severity === "blocking" && x.files.length >= 2)) {
    edges.push({ from: fd.files[0], to: fd.files[1], kind: "bad", why: `${fd.title}. ${fd.detail}` });
  }
  const linked = new Set(edges.flatMap((e) => [e.from, e.to]));
  // «Библиотека» — мод, который лаунчер поставил как чью-то зависимость (по журналу).
  const asDep = (n: string) => journal.some((j) => j.reason.startsWith("нужен для") && j.what.startsWith(n));
  const nodes: Node[] = files.filter((f) => linked.has(f.file_name)).map((f) => ({ id: f.file_name, name: name(f), lib: asDep(f.mods[0]?.name ?? "") }));
  return { nodes, edges, missing, hidden: files.filter((f) => f.enabled && !linked.has(f.file_name)).length };
}

/** Пружинная раскладка: узлы отталкиваются, связи стягивают. Детерминированно (старт по кругу). */
function layout(nodes: Node[], edges: Edge[], w: number, h: number): Map<string, { x: number; y: number }> {
  const pos = new Map(nodes.map((n, i) => {
    const a = (2 * Math.PI * i) / Math.max(1, nodes.length);
    return [n.id, { x: w / 2 + Math.cos(a) * w * 0.3, y: h / 2 + Math.sin(a) * h * 0.3 }];
  }));
  const k = Math.sqrt((w * h) / Math.max(1, nodes.length)) * 0.6;
  for (let it = 0, temp = w / 8; it < 300; it++, temp *= 0.985) {
    const disp = new Map(nodes.map((n) => [n.id, { x: 0, y: 0 }]));
    for (const a of nodes) for (const b of nodes) {
      if (a === b) continue;
      const pa = pos.get(a.id)!, pb = pos.get(b.id)!;
      const dx = pa.x - pb.x, dy = pa.y - pb.y, d = Math.max(1, Math.hypot(dx, dy));
      const f = (k * k) / d;
      disp.get(a.id)!.x += (dx / d) * f; disp.get(a.id)!.y += (dy / d) * f;
    }
    for (const e of edges) {
      const pa = pos.get(e.from), pb = pos.get(e.to);
      if (!pa || !pb) continue;
      const dx = pa.x - pb.x, dy = pa.y - pb.y, d = Math.max(1, Math.hypot(dx, dy));
      const f = (d * d) / k;
      disp.get(e.from)!.x -= (dx / d) * f; disp.get(e.from)!.y -= (dy / d) * f;
      disp.get(e.to)!.x += (dx / d) * f; disp.get(e.to)!.y += (dy / d) * f;
    }
    for (const n of nodes) {
      const p = pos.get(n.id)!, v = disp.get(n.id)!, d = Math.max(1, Math.hypot(v.x, v.y));
      p.x = Math.min(w - 90, Math.max(30, p.x + (v.x / d) * Math.min(d, temp)));
      p.y = Math.min(h - 25, Math.max(25, p.y + (v.y / d) * Math.min(d, temp)));
    }
  }
  return pos;
}

export function DepsTab({ inst }: { inst: Instance }) {
  const [data, setData] = useState<{ files: ModFile[]; report: DoctorReport; journal: JournalEntry[] } | null>(null);
  const [error, setError] = useState("");
  const [view, setView] = useState<View>("all");
  const [sel, setSel] = useState<string | null>(null);
  useEffect(() => { mods.view(inst.id).then(setData).catch((e) => setError(errText(e))); }, [inst.id]);
  const g = useMemo(() => (data ? buildGraph(data.files, data.report, data.journal) : null), [data]);
  const W = 900, H = 520;
  const pos = useMemo(() => (g ? layout(g.nodes, g.edges, W, H) : new Map()), [g]);

  if (error) return <div className="alert err"><span>{error}</span></div>;
  if (!g) return <p className="muted"><span className="spinner" /> {t("Загружаем…")}</p>;
  if (g.nodes.length === 0) return <div className="card empty"><p>{t("Связей между модами нет: каждый работает сам по себе.")}</p></div>;

  const near = new Set(sel ? [sel, ...g.edges.filter((e) => e.from === sel || e.to === sel).flatMap((e) => [e.from, e.to])] : []);
  const edges = g.edges.filter((e) => view === "all" || (view === "problems" ? e.kind === "bad" : !sel || near.has(e.from) && near.has(e.to) && (e.from === sel || e.to === sel)));
  const shown = new Set(view === "all" ? g.nodes.map((n) => n.id) : edges.flatMap((e) => [e.from, e.to]).concat(sel ? [sel] : []));
  const node = g.nodes.find((n) => n.id === sel);
  const nameOf = (id: string) => g.nodes.find((n) => n.id === id)?.name ?? id;

  return (
    <>
      <div className="row" style={{ justifyContent: "space-between", marginBottom: 10 }}>
        <div className="seg" role="group" aria-label={t("Какие связи показывать")}>
          {([["all", t("Все связи")], ["around", t("Вокруг выбранного")], ["problems", t("Только проблемы")]] as const).map(([v, l]) => (
            <button key={v} className={view === v ? "on" : ""} onClick={() => setView(v)}>{l}</button>
          ))}
        </div>
        <span className="muted small">{t("моды без связей скрыты ({0}) · клик по узлу — подробности", g.hidden)}</span>
      </div>
      <div className="card graph">
        <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t("Граф зависимостей модов")}>
          {edges.map((e, i) => {
            const a = pos.get(e.from), b = pos.get(e.to);
            if (!a || !b) return null;
            return (
              <g key={i}>
                <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className={`edge ${e.kind}`} />
                {e.kind === "bad" && <text x={(a.x + b.x) / 2 + 6} y={(a.y + b.y) / 2} className="edge-label">× {t("конфликт")}</text>}
              </g>
            );
          })}
          {g.nodes.filter((n) => shown.has(n.id)).map((n) => {
            const p = pos.get(n.id)!;
            const bad = g.edges.some((e) => e.kind === "bad" && (e.from === n.id || e.to === n.id));
            return (
              <g key={n.id} className={`node ${n.lib ? "lib" : ""} ${bad ? "bad" : ""} ${sel === n.id ? "on" : ""}`} role="button" tabIndex={0} aria-label={n.name}
                onClick={() => setSel(sel === n.id ? null : n.id)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setSel(sel === n.id ? null : n.id); } }}>
                <circle cx={p.x} cy={p.y} r={n.lib ? 6 : 10} />
                <text x={p.x + 14} y={p.y + 4}>{n.name}</text>
              </g>
            );
          })}
        </svg>
        {node && (
          <div className="graph-info" aria-live="polite">
            <b>{node.name}</b>
            {g.edges.filter((e) => e.kind === "bad" && (e.from === node.id || e.to === node.id)).map((e, i) => (
              <span key={i} className="small" style={{ color: "var(--err)" }}>{e.why ?? t("Конфликт с {0}", nameOf(e.from === node.id ? e.to : e.from))}</span>
            ))}
            {g.edges.some((e) => e.from === node.id && e.kind !== "bad") && <span className="small">{t("нужны:")} {g.edges.filter((e) => e.from === node.id && e.kind !== "bad").map((e) => nameOf(e.to)).join(", ")}</span>}
            {g.edges.some((e) => e.to === node.id && e.kind !== "bad") && <span className="small muted">{t("нужен для:")} {g.edges.filter((e) => e.to === node.id && e.kind !== "bad").map((e) => nameOf(e.from)).join(", ")}</span>}
            {(g.missing.get(node.id) ?? []).length > 0 && <span className="small" style={{ color: "var(--err)" }}>{t("нет в профиле")}: {g.missing.get(node.id)!.join(", ")}</span>}
          </div>
        )}
      </div>
      <p className="muted small legend">
        <span><i className="lg req" /> {t("требует")}</span>
        <span><i className="lg opt" /> {t("необязательно")}</span>
        <span><i className="lg bad" /> {t("конфликт")}</span>
        <span>{t("серый узел — библиотека, поставлена как зависимость")}</span>
      </p>
    </>
  );
}
