import { useEffect, useState } from "react";
import { api, duration, errText, relTime, type Instance } from "../api";
import type { LaunchState } from "../launches";
import { LogView } from "../ui/LogView";

/** Логи и краши: последний запуск выбранного профиля, живой или из файла. */
export function Logs({ instances, states, id, onPick }: { instances: Instance[]; states: Map<string, LaunchState>; id?: string; onPick: (id: string) => void }) {
  const inst = instances.find((i) => i.id === id) ?? instances[0];
  const state = inst ? states.get(inst.id) : undefined;
  const [file, setFile] = useState<string[] | null>(null);
  const [err, setErr] = useState("");
  const live = !!state && state.logs.length > 0;

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
            {last ? <>Профиль «{inst.name}» · запуск {relTime(last.at)}{last.duration_secs ? ` · ${duration(last.duration_secs)}` : ""}</> : <>Профиль «{inst.name}» ещё не запускался</>}
          </p>
        </div>
        <select className="field" style={{ maxWidth: 260 }} value={inst.id} onChange={(e) => onPick(e.target.value)} aria-label="Профиль">
          {instances.map((i) => <option key={i.id} value={i.id}>{i.name}</option>)}
        </select>
      </div>
      {crashed && (
        <div className="alert err">
          <div className="h">Игра упала{exit?.code != null ? ` с кодом ${exit.code}` : last?.exit_code != null ? ` с кодом ${last.exit_code}` : ""}</div>
          <span className="small">
            Разбор причины по стектрейсу (вероятный виновник среди модов) появится вместе с Mod Doctor. Пока: посмотрите строки на вкладке «Ошибки» — обычно причина в первой строке «Caused by».
          </span>
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
    </>
  );
}
