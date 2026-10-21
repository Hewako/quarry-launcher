// Разбор краша: что случилось простыми словами, уверенность, строки-основания и действия.
import { api, errText, mods, type CrashAction, type CrashAnalysis, type Instance } from "../api";
import { launches } from "../launches";
import { useDialog } from "./Dialog";
import { t } from "../i18n";

export function CrashCard({ inst, a, code, onOpenTab }: { inst: Instance; a: CrashAnalysis; code?: number; onOpenTab?: (tab: "mods" | "launch") => void }) {
  const dialog = useDialog();
  const run = async (x: CrashAction) => {
    try {
      if (x.kind === "disable_mod") {
        await mods.toggle(inst.id, x.file_name, false);
        await dialog.alert(t("Мод выключен"), t("{0} выключен — файл остался в профиле, включить обратно можно на вкладке «Моды». Попробуйте запустить игру ещё раз.", x.file_name));
      } else if (x.kind === "safe_launch") {
        launches.begin(inst.id);
        await api.launch(inst.id, undefined, false, true);
      } else if (x.kind === "increase_memory") {
        onOpenTab?.("launch");
      } else if (x.kind === "open_mod_doctor") {
        onOpenTab?.("mods");
      }
    } catch (e) {
      await dialog.alert(t("Не получилось"), errText(e));
    }
  };
  const label = (x: CrashAction) =>
    x.kind === "disable_mod" ? t("Выключить {0}", x.file_name) :
    x.kind === "safe_launch" ? t("Запустить без модов") :
    x.kind === "increase_memory" ? t("Увеличить память") :
    x.kind === "open_mod_doctor" ? t("Открыть Mod Doctor") : t("Как обновить драйвер — на сайте производителя видеокарты");
  return (
    <div className="alert err">
      <div className="h">{t("Игра упала")}{code != null ? t(" (код {0})", code) : ""}: {a.title}</div>
      <span>{a.explanation}</span>
      {a.evidence.length > 0 && <div className="trace">{a.evidence.join("\n")}</div>}
      <span className="muted small">{t("Уверенность:")} <b>{t(a.confidence_label)}</b>{t(". Вывод сделан по логу игры по правилам, без ИИ; это подсказка, а не приговор.")}</span>
      <div className="acts">
        {a.actions.filter((x) => x.kind !== "update_drivers").map((x, i) => (
          <button key={i} className={i === 0 ? "btn pri" : "btn"} onClick={() => run(x)}>{label(x)}</button>
        ))}
      </div>
      {a.actions.some((x) => x.kind === "update_drivers") && <span className="small muted">{t("Совет: обновите драйвер видеокарты.")}</span>}
    </div>
  );
}
