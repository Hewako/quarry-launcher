import { useEffect, useMemo, useRef, useState } from "react";
import { t } from "../i18n";

type Filter = "all" | "errors" | "warnings" | "loader";

const isErr = (l: string) => /\/(ERROR|FATAL)\]|^\s*(Caused by|Exception in thread)|^\s+at |Exception:|Error:/.test(l);
const isWarn = (l: string) => /\/WARN\]/.test(l);
const isLoader = (l: string) => /Fabric|Quilt|Knot|Mixin|NeoForge|FML|ModLauncher/i.test(l);

/** Подсветить метки маскирования, чтобы было видно: тут скрыто, а не пусто. */
function renderLine(l: string) {
  const parts = l.split(/(‹[^›]+›)/);
  return parts.map((p, i) => (p.startsWith("‹") ? <span key={i} className="mask">{p}</span> : p));
}

export function LogView({ lines, follow = true }: { lines: string[]; follow?: boolean }) {
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [copied, setCopied] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return lines.filter((l) => {
      if (filter === "errors" && !isErr(l)) return false;
      if (filter === "warnings" && !isWarn(l)) return false;
      if (filter === "loader" && !isLoader(l)) return false;
      return !q || l.toLowerCase().includes(q);
    });
  }, [lines, filter, query]);

  useEffect(() => {
    const el = box.current;
    if (el && follow && stick.current) el.scrollTop = el.scrollHeight;
  }, [shown, follow]);

  const copy = async () => {
    const sel = window.getSelection()?.toString();
    await navigator.clipboard.writeText(sel && sel.length > 0 ? sel : shown.join("\n"));
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const errors = lines.filter(isErr).length;
  return (
    <>
      <div className="filters">
        {([["all", t("Всё")], ["errors", t("Ошибки{0}", errors ? ` (${errors})` : "")], ["warnings", t("Предупреждения")], ["loader", t("Только загрузчик")]] as const).map(([k, label]) => (
          <button key={k} className={`btn ${filter === k ? "on" : ""}`} onClick={() => setFilter(k)}>{label}</button>
        ))}
        <input className="field" style={{ maxWidth: 240 }} placeholder={t("Поиск в логе…")} value={query} onChange={(e) => setQuery(e.target.value)} />
        <span style={{ flex: 1 }} />
        <button className="btn" onClick={copy}>{copied ? t("Скопировано") : t("Копировать фрагмент")}</button>
      </div>
      <div
        className="logbox"
        ref={box}
        tabIndex={0}
        aria-label={t("Лог игры")}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {shown.length === 0 ? <span className="i">{t("Строк нет.")}</span> : shown.map((l, i) => (
          <div key={i} className={isErr(l) ? "e" : isWarn(l) ? "w" : undefined}>{renderLine(l)}</div>
        ))}
      </div>
      <p className="muted small">{t("Токены, путь к домашней папке и IP-адреса маскируются ещё при записи лога: в файл, на экран и в отчёт они не попадают.")}</p>
    </>
  );
}
