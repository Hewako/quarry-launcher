// Разбор краша: что случилось простыми словами, уверенность, строки-основания и выбор починки.
import { useEffect, useState } from "react";
import { api, errText, mods, snapshots, type CrashAction, type CrashAnalysis, type Instance, type SnapshotInfo } from "../api";
import { launches } from "../launches";
import { useDialog } from "./Dialog";
import { t, tc } from "../i18n";

type Choice = { key: string; title: string; hint: string; run: () => Promise<string | null> };

export function CrashCard({ inst, a, code, onOpenTab, onReport }: {
  inst: Instance;
  a: CrashAnalysis;
  code?: number;
  onOpenTab?: (tab: "mods" | "launch") => void;
  onReport?: () => void;
}) {
  const dialog = useDialog();
  const [snap, setSnap] = useState<SnapshotInfo | null>(null);
  const [pick, setPick] = useState(0);
  const [done, setDone] = useState("");
  const [busy, setBusy] = useState(false);
  // Снимок, сделанный до последнего изменения перед этим запуском, — к нему можно откатиться.
  useEffect(() => {
    const before = inst.last_launch?.at ?? Number.MAX_SAFE_INTEGER;
    snapshots.list(inst.id).then((l) => setSnap(l.filter((s) => s.created_at <= before).sort((x, y) => y.created_at - x.created_at)[0] ?? null)).catch(() => setSnap(null));
  }, [inst.id, inst.last_launch?.at]);

  const fromAction = (x: CrashAction): Choice | null =>
    x.kind === "disable_mod" ? {
      key: `off-${x.file_name}`,
      title: t("Выключить {0}", x.file_name),
      hint: t("Мод останется в профиле выключенным, включить обратно можно на вкладке «Моды»."),
      run: async () => { await snapshots.create(inst.id, t("Перед починкой краша")); await mods.toggle(inst.id, x.file_name, false); return t("{0} выключен, создан снимок «до починки»", x.file_name); },
    } : x.kind === "increase_memory" ? {
      key: "mem", title: t("Увеличить память"), hint: t("Откроется вкладка «Запуск и память»."),
      run: async () => { onOpenTab?.("launch"); return null; },
    } : x.kind === "open_mod_doctor" ? {
      key: "doctor", title: t("Открыть Mod Doctor"), hint: t("Посмотреть конфликты и зависимости модов."),
      run: async () => { onOpenTab?.("mods"); return null; },
    } : null;
  const choices: Choice[] = [
    ...a.actions.map(fromAction).filter((c): c is Choice => !!c),
    ...(snap ? [{
      key: "rollback",
      title: t("Откатиться к снимку «{0}»", tc(snap.reason)),
      hint: t("Моды и настройки вернутся к {0}; изменения после этого пропадут (текущее состояние сохранится отдельным снимком).", new Date(snap.created_at * 1000).toLocaleString()),
      run: async () => { await snapshots.restore(inst.id, snap.id); return t("профиль возвращён к снимку «{0}»", tc(snap.reason)); },
    }] : []),
    { key: "none", title: t("Не чинить"), hint: t("Ничего не менять. Разбор останется в журнале, запуск, скорее всего, снова упадёт."), run: async () => t("ничего не меняли") },
  ];
  const fix = async () => {
    setBusy(true);
    try { const r = await choices[pick].run(); if (r) setDone(r); } catch (e) { await dialog.alert(t("Не получилось"), errText(e)); } finally { setBusy(false); }
  };
  const play = () => { launches.begin(inst.id); api.launch(inst.id).catch((e) => dialog.alert(t("Не удалось начать запуск"), errText(e))); };

  return (
    <div className="alert err">
      <div className="h">{t("Игра упала")}{code != null ? t(" (код {0})", code) : ""}: {tc(a.title)}</div>
      <span>{tc(a.explanation)}</span>
      {a.evidence.length > 0 && <div className="trace">{a.evidence.join("\n")}</div>}
      <span className="muted small">{t("Уверенность:")} <b>{tc(a.confidence_label)}</b>{t(". Вывод сделан по логу игры по правилам, без ИИ; это подсказка, а не приговор.")}</span>
      <div className="fixopts" role="radiogroup" aria-label={t("Как починить")}>
        {choices.map((c, i) => (
          <label key={c.key} className={`fixopt ${pick === i ? "on" : ""}`}>
            <input type="radio" name={`fix-${inst.id}`} checked={pick === i} onChange={() => setPick(i)} />
            <span><b>{c.title}</b>{i === 0 && c.key !== "none" && <span className="badge ok" style={{ marginLeft: 6 }}>{t("рекомендуется")}</span>}<small>{c.hint}</small></span>
          </label>
        ))}
      </div>
      <div className="acts" style={{ justifyContent: "flex-start" }}>
        <button className="btn pri" disabled={busy} onClick={fix}>{busy ? t("Чиним…") : t("Починить")}</button>
        {a.actions.some((x) => x.kind === "safe_launch") && <button className="btn" onClick={() => { launches.begin(inst.id); api.launch(inst.id, undefined, false, true).catch(() => {}); }}>{t("Безопасный запуск без модов")}</button>}
        {onReport && <button className="btn" onClick={onReport}>{t("Отчёт для поддержки")}</button>}
      </div>
      {done ? (
        <span className="small">{t("Готово: {0}. Можно запускать —", done)} <button className="btn link small" onClick={play}>{t("▶ Играть")}</button></span>
      ) : <span className="small muted">{t("Перед починкой создаётся снимок — отменить можно на вкладке «Снимки и миры».")}</span>}
      {a.actions.some((x) => x.kind === "update_drivers") && <span className="small muted">{t("Совет: обновите драйвер видеокарты.")}</span>}
    </div>
  );
}
