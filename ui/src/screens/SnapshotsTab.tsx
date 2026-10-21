// Вкладка «Снимки»: история изменений профиля, сравнение с текущим и откат; бэкапы миров.
import { useCallback, useEffect, useState } from "react";
import { errText, relTime, sizeText, snapshots, type Instance, type SnapshotDiff, type SnapshotInfo } from "../api";
import { useDialog } from "../ui/Dialog";

function DiffView({ d }: { d: SnapshotDiff }) {
  const empty = !d.added.length && !d.removed.length && !d.changed.length && !d.options.length;
  if (empty) return <div className="diff"><span className="muted">С тех пор ничего не изменилось.</span></div>;
  return (
    <div className="diff">
      {d.added.map((p) => <span key={p} className="add">+ {p} <span className="muted">(появилось после снимка)</span></span>)}
      {d.removed.map((p) => <span key={p} className="del">− {p} <span className="muted">(было в снимке)</span></span>)}
      {d.changed.map((p) => <span key={p} className="chg">~ {p}</span>)}
      {d.options.map(([k, a, b]) => <span key={k} className="chg">~ options.txt: {k} {a || "—"} → {b}</span>)}
    </div>
  );
}

export function SnapshotsTab({ inst, running, onRestored }: { inst: Instance; running: boolean; onRestored: () => void }) {
  const dialog = useDialog();
  const [list, setList] = useState<SnapshotInfo[] | null>(null);
  const [diffs, setDiffs] = useState<Record<string, SnapshotDiff>>({});
  const [worlds, setWorlds] = useState<string[]>([]);
  const [busy, setBusy] = useState("");

  const load = useCallback(() => {
    snapshots.list(inst.id).then(setList).catch(() => setList([]));
    snapshots.worlds(inst.id).then(setWorlds).catch(() => setWorlds([]));
  }, [inst.id]);
  useEffect(load, [load]);

  const act = async (key: string, f: () => Promise<unknown>, fail: string) => {
    setBusy(key);
    try { await f(); load(); } catch (e) { await dialog.alert(fail, errText(e)); } finally { setBusy(""); }
  };
  const restore = async (s: SnapshotInfo) => {
    const ok = await dialog.confirm({
      title: `Откатить к «${s.reason}»?`,
      body: "Моды, настройки модов и options.txt вернутся к состоянию снимка. Текущее состояние сначала сохранится отдельным снимком, так что откат тоже можно отменить. Миры не затрагиваются.",
      confirm: "Откатить",
    });
    if (ok) await act(s.id, async () => { await snapshots.restore(inst.id, s.id); setDiffs({}); onRestored(); }, "Откат не удался");
  };

  return (
    <>
      <div className="pagehead" style={{ marginBottom: 14 }}>
        <p className="muted small" style={{ margin: 0, maxWidth: 680 }}>
          Снимок создаётся сам перед установкой, обновлением и удалением модов, а также перед откатом. Хранятся 10 последних автоматических, ручные и закреплённые. В снимок входят моды, настройки модов и options.txt; миры защищают отдельные бэкапы ниже.
        </p>
        <button className="btn" disabled={running || busy !== ""} onClick={() => act("new", () => snapshots.create(inst.id, "Создан вручную"), "Снимок не создан")}>+ Создать снимок</button>
      </div>
      <div className="card">
        {list === null ? <p className="muted"><span className="spinner" /> Загружаем…</p> : list.length === 0 ? (
          <p className="muted" style={{ margin: 0 }}>Снимков пока нет. Первый появится сам перед первым изменением модов.</p>
        ) : (
          <div className="timeline">
            {list.map((s) => (
              <div className="snap" key={s.id}>
                <b>{s.reason}</b>{" "}
                <span className="muted small">· {relTime(s.created_at)} · {s.auto ? "автоматически" : "вручную"} · модов {s.mods}, конфигов {s.configs}</span>
                {s.pinned && <span className="pin"> ● закреплён — ротация его не удалит</span>}
                {diffs[s.id] && <DiffView d={diffs[s.id]} />}
                <div className="row" style={{ marginTop: 6 }}>
                  <button className="btn" disabled={running || busy !== ""} onClick={() => restore(s)}>{busy === s.id ? "Откатываем…" : "Откатить к этому снимку"}</button>
                  <button className="btn" onClick={() => snapshots.diff(inst.id, s.id).then((d) => setDiffs({ ...diffs, [s.id]: d })).catch((e) => dialog.alert("Не удалось сравнить", errText(e)))}>Сравнить с текущим</button>
                  <button className="btn link small" onClick={() => act(`pin${s.id}`, () => snapshots.pin(inst.id, s.id, !s.pinned), "Не получилось")}>{s.pinned ? "Открепить" : "Закрепить"}</button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
      <h4 style={{ margin: "22px 0 10px" }}>Бэкапы миров</h4>
      <div className="card">
        {worlds.length === 0 ? <p className="muted" style={{ margin: 0 }}>Миров в профиле пока нет.</p> : (
          <table>
            <tbody>
              {worlds.map((w) => (
                <tr key={w}>
                  <td>{w}</td>
                  <td style={{ textAlign: "right" }}>
                    <button className="btn" disabled={busy !== ""} onClick={() => act(`w${w}`, async () => {
                      const b = await snapshots.backupWorld(inst.id, w);
                      await dialog.alert("Бэкап готов", `${b.world}: ${sizeText(b.size)}. Хранятся 5 последних бэкапов каждого мира (папка backups в данных Quarry).`);
                    }, "Бэкап не создан")}>{busy === `w${w}` ? "Сохраняем…" : "Сделать бэкап"}</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </>
  );
}
