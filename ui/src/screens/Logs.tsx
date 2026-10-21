import { save } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, diagnostics, duration, errText, relTime, sizeText, type Instance } from "../api";
import { Modal, useDialog } from "../ui/Dialog";
import type { LaunchState } from "../launches";
import { LogView } from "../ui/LogView";
import { CrashCard } from "../ui/CrashCard";

/** Логи и краши: последний запуск выбранного профиля, живой или из файла. */
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

  if (!inst) return <p className="muted">Профилей нет — логов тоже.</p>;
  const last = inst.last_launch;
  const exit = state?.exit;
  const crashed = exit ? exit.crashed : last?.outcome === "crashed";
  return (
    <>
      <div className="pagehead">
        <div>
          <h2>Логи и краши</h2>
          <p className="sub">
            {last ? <>Профиль «{inst.name}» · запуск {relTime(last.at)}{last.duration_secs ? ` · ${duration(last.duration_secs)}` : ""}
              {last.prepare_secs !== undefined ? ` · подготовка ${duration(last.prepare_secs)}` : ""}{last.peak_ram_mb ? ` · память до ${(last.peak_ram_mb / 1024).toFixed(1)} ГБ` : ""}</> : <>Профиль «{inst.name}» ещё не запускался</>}
          </p>
        </div>
        <button className="btn" onClick={() => diagnostics.reportPreview(inst.id).then((r) => { setReport(r); setPart(0); }).catch((e) => dialog.alert("Отчёт не собран", errText(e)))}>Отчёт для поддержки…</button>
        <select className="field" style={{ maxWidth: 260 }} value={inst.id} onChange={(e) => onPick(e.target.value)} aria-label="Профиль">
          {instances.map((i) => <option key={i.id} value={i.id}>{i.name}</option>)}
        </select>
      </div>
      {exit?.analysis && <CrashCard inst={inst} a={exit.analysis} code={exit.code} />}
      {crashed && !exit?.analysis && (
        <div className="alert err">
          <div className="h">Игра упала{exit?.code != null ? ` с кодом ${exit.code}` : last?.exit_code != null ? ` с кодом ${last.exit_code}` : ""}</div>
          {last?.message && <span>{last.message}</span>}
          <span className="small">Подробный разбор виден сразу после краша. Здесь — сохранённый лог: посмотрите вкладку «Ошибки», обычно причина в первой строке «Caused by».</span>
          {exit?.crash_report && <span className="small mono" style={{ userSelect: "text" }}>Crash report: {exit.crash_report}</span>}
          <div className="acts">
            <button className="btn" onClick={() => api.openFolder(inst.id, "crash-reports")}>Открыть crash-reports</button>
            <button className="btn" onClick={() => api.openFolder(inst.id, "logs")}>Папка логов игры</button>
          </div>
        </div>
      )}
      {last?.outcome === "failed_to_start" && !live && (
        <div className="alert err"><div className="h">Запуск не начался</div><span style={{ userSelect: "text" }}>{last.message}</span></div>
      )}
      {live ? <LogView lines={state.logs} /> : file ? <LogView lines={file} follow={false} /> : <p className="muted">{err || <><span className="spinner" /> Загружаем лог…</>}</p>}
      {crashes.length > 0 && (
        <>
          <h4 style={{ margin: "22px 0 10px" }}>Отчёты о крашах · {crashes.length}</h4>
          <div className="card" style={{ padding: 0 }}>
            <table className="servers">
              <tbody>
                {crashes.map((c) => (
                  <tr key={c.file}>
                    <td><b>{c.description || "Без описания"}</b><div className="muted small">{c.when} · {sizeText(c.size)}</div></td>
                    <td style={{ textAlign: "right" }}>
                      <button className="btn" onClick={() => diagnostics.readCrash(inst.id, c.file).then((t) => setShown({ title: c.when, text: t })).catch((e) => dialog.alert("Не открылся", errText(e)))}>Открыть</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
      {shown && (
        <Modal title={`Отчёт о краше · ${shown.title}`} onClose={() => setShown(null)} wide>
          <pre className="cfgdiff" style={{ userSelect: "text" }}>{shown.text}</pre>
          <div className="acts"><button className="btn pri" onClick={() => setShown(null)}>Закрыть</button></div>
        </Modal>
      )}
      {report && (
        <Modal title="Отчёт для поддержки" onClose={() => setReport(null)} wide>
          <p className="small muted" style={{ marginTop: 0 }}>Вот что попадёт в архив — проверьте перед отправкой. Токены, путь к домашней папке и IP-адреса уже скрыты{" "}(в режиме стримера — и ник).</p>
          <div className="seg" role="tablist" style={{ flexWrap: "wrap" }}>
            {report.map((f, i) => <button key={f.name} role="tab" aria-selected={part === i} className={part === i ? "on" : ""} onClick={() => setPart(i)}>{f.name}</button>)}
          </div>
          <pre className="cfgdiff" style={{ userSelect: "text", marginTop: 8 }}>{report[part]?.content}</pre>
          <div className="acts">
            <button className="btn" onClick={() => setReport(null)}>Закрыть</button>
            <button className="btn pri" onClick={async () => {
              const path = await save({ title: "Сохранить отчёт", defaultPath: `quarry-report-${inst.id}.zip`, filters: [{ name: "ZIP", extensions: ["zip"] }] }).catch(() => null);
              if (!path) return;
              try { await diagnostics.reportSave(inst.id, path); setReport(null); await dialog.alert("Отчёт сохранён", "Приложите этот архив к вопросу в поддержку или в issue на GitHub."); }
              catch (e) { await dialog.alert("Не сохранён", errText(e)); }
            }}>Сохранить .zip…</button>
          </div>
        </Modal>
      )}
    </>
  );
}
