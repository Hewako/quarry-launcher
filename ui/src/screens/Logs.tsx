import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, diagnostics, duration, errText, relTime, sizeText, statusOf, type Instance } from "../api";
import { Modal, useDialog } from "../ui/Dialog";
import type { LaunchState } from "../launches";
import { LogView } from "../ui/LogView";
import { CrashCard } from "../ui/CrashCard";
import { t, tc } from "../i18n";

/** Логи и краши (макет 06): слева последние запуски всех профилей, справа разбор и лог выбранного. */
export function Logs({ instances, states, id, onPick }: { instances: Instance[]; states: Map<string, LaunchState>; id?: string; onPick: (id: string) => void }) {
  const inst = instances.find((i) => i.id === id) ?? instances[0];
  const state = inst ? states.get(inst.id) : undefined;
  const [file, setFile] = useState<string[] | null>(null);
  const [err, setErr] = useState("");
  const live = !!state && state.logs.length > 0;
  const dialog = useDialog();
  const [crashes, setCrashes] = useState<{ file: string; when: string; size: number; description: string }[]>([]);
  const [shown, setShown] = useState<{ title: string; text: string } | null>(null);
  const [report, setReport] = useState<{ name: string; content: string }[] | null>(null);
  const [part, setPart] = useState(0);
  useEffect(() => { if (inst) diagnostics.crashReports(inst.id).then(setCrashes).catch(() => setCrashes([])); }, [inst?.id, state?.phase]);

  useEffect(() => {
    setFile(null);
    setErr("");
    if (!inst || live) return;
    api.readLog(inst.id).then(setFile).catch((e) => setErr(errText(e)));
  }, [inst?.id, live]);

  const openReport = () => { if (inst) diagnostics.reportPreview(inst.id).then((r) => { setReport(r); setPart(0); }).catch((e) => dialog.alert(t("Отчёт не собран"), errText(e))); };
  if (!inst) return <p className="muted">{t("Профилей нет — логов тоже.")}</p>;
  const last = inst.last_launch;
  const exit = state?.exit;
  const crashed = exit ? exit.crashed : last?.outcome === "crashed";
  // Слева — последние запуски всех профилей (новые сверху); выбранный профиль — справа подробно.
  const runs = instances.filter((i) => i.last_launch || i.id === inst.id).sort((a, b) => (b.last_launch?.at ?? 0) - (a.last_launch?.at ?? 0));
  const tone = (i: Instance) => { const o = i.last_launch?.outcome; return o === "success" ? "ok" : o === "crashed" || o === "failed_to_start" ? "err" : o === "killed" ? "warn" : ""; };
  return (
    <>
      <h2>{t("Логи и краши")}</h2>
      <p className="sub">
        {last ? <>{t("Профиль «")}{inst.name}{t("» · запуск")} {relTime(last.at)}{last.duration_secs ? ` · ${duration(last.duration_secs)}` : ""}
          {last.prepare_secs !== undefined ? t(" · подготовка {0}", duration(last.prepare_secs)) : ""}{last.peak_ram_mb ? t(" · память до {0} ГБ", (last.peak_ram_mb / 1024).toFixed(1)) : ""}{last.cpu_percent ? t(" · процессор в среднем {0} %", last.cpu_percent) : ""}</> : <>{t("Профиль «")}{inst.name}{t("» ещё не запускался")}</>}
      </p>
      <div className="logsgrid">
        <nav className="card runs" aria-label={t("Последние запуски профилей")}>
          <h3 className="muted small">{t("Последние запуски профилей")}</h3>
          {runs.map((i) => (
            <button key={i.id} className={`run ${tone(i)} ${i.id === inst.id ? "on" : ""}`} aria-current={i.id === inst.id ? "true" : undefined} onClick={() => onPick(i.id)}>
              <span className="muted small">{i.last_launch ? relTime(i.last_launch.at) : t("ещё не запускался")}</span>
              <b>{i.name}</b>
              <span className="muted small">{statusOf(i, states.get(i.id)?.phase).text}{i.last_launch?.duration_secs ? ` · ${duration(i.last_launch.duration_secs)}` : ""}</span>
            </button>
          ))}
        </nav>
        <div className="stack">
          {exit?.analysis && <CrashCard inst={inst} a={exit.analysis} code={exit.code} onReport={openReport} />}
          {crashed && !exit?.analysis && (
            <div className="alert err">
              <div className="h">{t("Игра упала")}{exit?.code != null ? t(" с кодом {0}", exit.code) : last?.exit_code != null ? t(" с кодом {0}", last.exit_code) : ""}</div>
              {last?.message && <span>{tc(last.message)}</span>}
              <span className="small">{t("Подробный разбор виден сразу после краша. Здесь — сохранённый лог: посмотрите вкладку «Ошибки», обычно причина в первой строке «Caused by».")}</span>
              {exit?.crash_report && <span className="small mono" style={{ userSelect: "text" }}>Crash report: {exit.crash_report}</span>}
            </div>
          )}
          {last?.outcome === "failed_to_start" && !live && (
            <div className="alert err"><div className="h">{t("Запуск не начался")}</div><span style={{ userSelect: "text" }}>{tc(last.message)}</span></div>
          )}
          <section className="card">
            <h3>{t("Лог запуска")}{last ? `, ${relTime(last.at)}` : ""}</h3>
            {live ? <LogView lines={state.logs} /> : file ? <LogView lines={file} follow={false} /> : <p className="muted">{err || <><span className="spinner" /> {t("Загружаем лог…")}</>}</p>}
            <div className="row" style={{ marginTop: 10 }}>
              <button className="btn" onClick={openReport}>{t("Отчёт для поддержки…")}</button>
              <button className="btn" onClick={() => api.openFolder(inst.id, "logs")}>{t("Папка логов игры")}</button>
              <button className="btn" onClick={() => api.openFolder(inst.id, "crash-reports")}>{t("Открыть crash-reports")}</button>
            </div>
          </section>
        {crashes.length > 0 && (
          <>
            <h4 className="subhead">{t("Отчёты о крашах ·")} {crashes.length}</h4>
            <div className="card" style={{ padding: 0 }}>
              <table className="servers">
                <tbody>
                  {crashes.map((c) => (
                    <tr key={c.file}>
                      <td><b>{c.description || t("Без описания")}</b><div className="muted small">{c.when} · {sizeText(c.size)}</div></td>
                      <td style={{ textAlign: "right" }}>
                        <button className="btn" onClick={() => diagnostics.readCrash(inst.id, c.file).then((text) => setShown({ title: c.when, text })).catch((e) => dialog.alert(t("Не открылся"), errText(e)))}>{t("Открыть")}</button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
        </div>
      </div>
      {shown && (
        <Modal title={t("Отчёт о краше · {0}", shown.title)} onClose={() => setShown(null)} wide>
          <pre className="cfgdiff" style={{ userSelect: "text" }}>{shown.text}</pre>
          <div className="acts"><button className="btn pri" onClick={() => setShown(null)}>{t("Закрыть")}</button></div>
        </Modal>
      )}
      {report && (
        <Modal title={t("Отчёт для поддержки")} onClose={() => setReport(null)} wide>
          <p className="small muted" style={{ marginTop: 0 }}>{t("Вот что попадёт в архив — проверьте перед отправкой. Токены, путь к домашней папке и IP-адреса уже скрыты")}{" "}{t("(в режиме стримера — и ник).")}</p>
          <div className="seg" role="tablist" style={{ flexWrap: "wrap" }}>
            {report.map((f, i) => <button key={f.name} role="tab" aria-selected={part === i} className={part === i ? "on" : ""} onClick={() => setPart(i)}>{f.name}</button>)}
          </div>
          <pre className="cfgdiff" style={{ userSelect: "text", marginTop: 8 }}>{report[part]?.content}</pre>
          <div className="acts">
            <button className="btn" onClick={() => setReport(null)}>{t("Закрыть")}</button>
            <button className="btn pri" onClick={async () => {
              const path = await save({ title: t("Сохранить отчёт"), defaultPath: `quarry-report-${inst.id}.zip`, filters: [{ name: "ZIP", extensions: ["zip"] }] }).catch(() => null);
              if (!path) return;
              try { await diagnostics.reportSave(inst.id, path); setReport(null); await dialog.alert(t("Отчёт сохранён"), t("Приложите этот архив к вопросу в поддержку или в issue на GitHub.")); }
              catch (e) { await dialog.alert(t("Не сохранён"), errText(e)); }
            }}>{t("Сохранить .zip…")}</button>
          </div>
        </Modal>
      )}
    </>
  );
}
