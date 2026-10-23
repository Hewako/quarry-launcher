// Вкладка «Откат» (макет 03): снимки с метками и сравнением, журнал изменений модов; справа миры и их бэкапы.
import { Fragment, useCallback, useEffect, useState } from "react";
import { errText, relTime, sizeText, snapshots, type Instance, type JournalEntry, type SnapshotDiff, type SnapshotInfo, type WorldBackup, type WorldInfo } from "../api";
import { useDialog } from "../ui/Dialog";
import { t, tc } from "../i18n";

function DiffView({ d }: { d: SnapshotDiff }) {
  const empty = !d.added.length && !d.removed.length && !d.changed.length && !d.options.length;
  if (empty) return <div className="diff"><span className="muted">{t("С тех пор ничего не изменилось.")}</span></div>;
  return (
    <div className="diff">
      {d.added.map((p) => <span key={p} className="add">+ {p} <span className="muted">{t("(появилось после снимка)")}</span></span>)}
      {d.removed.map((p) => <span key={p} className="del">− {p} <span className="muted">{t("(было в снимке)")}</span></span>)}
      {d.changed.map((p) => <span key={p} className="chg">~ {p}</span>)}
      {d.options.map(([k, a, b]) => <span key={k} className="chg">~ options.txt: {k} {a || "—"} → {b}</span>)}
    </div>
  );
}

export function SnapshotsTab({ inst, running, journal, onRestored }: { inst: Instance; running: boolean; journal: JournalEntry[]; onRestored: () => void }) {
  const dialog = useDialog();
  const [list, setList] = useState<SnapshotInfo[] | null>(null);
  const [diffs, setDiffs] = useState<Record<string, SnapshotDiff>>({});
  const [worlds, setWorlds] = useState<WorldInfo[]>([]);
  const [open, setOpen] = useState("");
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
      title: t("Откатить к «{0}»?", tc(s.reason)),
      body: t("Моды, настройки модов и options.txt вернутся к состоянию снимка. Текущее состояние сначала сохранится отдельным снимком, так что откат тоже можно отменить. Миры не затрагиваются."),
      confirm: t("Откатить"),
    });
    if (ok) await act(s.id, async () => { await snapshots.restore(inst.id, s.id); setDiffs({}); onRestored(); }, t("Откат не удался"));
  };
  const restoreWorld = async (w: WorldInfo, b: WorldBackup) => {
    const ok = await dialog.confirm({
      title: t("Вернуть «{0}» к бэкапу?", w.name),
      body: t("Мир вернётся к состоянию на {0}. Текущий мир не удаляется: сначала он сам сохранится в бэкап и отложится в папку .quarry-replaced внутри папки игры.", new Date(b.created_at * 1000).toLocaleString()),
      confirm: t("Восстановить"),
    });
    if (ok) await act(`r${b.file}`, async () => {
      await snapshots.restoreWorld(inst.id, b.file);
      await dialog.alert(t("Мир восстановлен"), t("«{0}» снова в состоянии бэкапа.", w.name));
    }, t("Мир не восстановлен"));
  };

  // «Последний рабочий» — состояние на последнем удачном запуске: первый снимок, сделанный после него.
  const ok = inst.last_launch?.outcome === "success" ? inst.last_launch.at : undefined;
  const working = ok === undefined ? undefined : [...(list ?? [])].filter((s) => s.created_at >= ok).sort((a, b) => a.created_at - b.created_at)[0]?.id;
  const when = (at: number) => new Date(at * 1000).toLocaleString(undefined, { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });

  return (
    <div className="ov">
      <div className="stack">
        <div className="pagehead">
          <p className="muted small" style={{ margin: 0, maxWidth: 560 }}>
            {t("Снимок создаётся сам перед установкой, обновлением и удалением модов, а также перед откатом. Хранятся 10 последних автоматических, ручные и закреплённые. В снимок входят моды, настройки модов и options.txt; миры защищают отдельные бэкапы ниже.")}
          </p>
          <button className="btn" disabled={running || busy !== ""} onClick={() => act("new", () => snapshots.create(inst.id, t("Создан вручную")), t("Снимок не создан"))}>{t("+ Создать снимок")}</button>
        </div>
        <section className="card" style={{ padding: 0 }}>
          {list === null ? <p className="muted pad"><span className="spinner" /> {t("Загружаем…")}</p> : list.length === 0 ? (
            <p className="muted pad">{t("Снимков пока нет. Первый появится сам перед первым изменением модов.")}</p>
          ) : (
            <ul className="snaps">
              {list.map((s) => (
                <li key={s.id}>
                  <span className="when muted">{when(s.created_at)}</span>
                  <div className="what">
                    <b>{tc(s.reason)}</b>
                    {s.id === working && <span className="tag ok">{t("последний рабочий")}</span>}
                    {s.auto && <span className="tag">{t("автоматически")}</span>}
                    {s.pinned && <span className="tag warn" title={t("● закреплён — ротация его не удалит")}>{t("закреплён")}</span>}
                    <div className="muted small">
                      {s.id === working ? t("С ним игра запускалась {0}", relTime(ok!)) : s.auto ? `${relTime(s.created_at)}` : t("Создан вручную")}
                      {" "}{t("· модов")} {s.mods}{t(", конфигов")} {s.configs}
                    </div>
                    {diffs[s.id] && <DiffView d={diffs[s.id]} />}
                  </div>
                  <div className="acts">
                    <button className="btn" onClick={() => snapshots.diff(inst.id, s.id).then((d) => setDiffs({ ...diffs, [s.id]: d })).catch((e) => dialog.alert(t("Не удалось сравнить"), errText(e)))}>{t("Сравнить с текущим")}</button>
                    <button className={`btn ${s.id === working ? "cta" : ""}`} disabled={running || busy !== ""} onClick={() => restore(s)}>{busy === s.id ? t("Откатываем…") : t("Откатить к этому снимку")}</button>
                    <button className="btn link small" onClick={() => act(`pin${s.id}`, () => snapshots.pin(inst.id, s.id, !s.pinned), t("Не получилось"))}>{s.pinned ? t("Открепить") : t("Закрепить")}</button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
        {journal.length > 0 && (
          <details className="card">
            <summary>{t("Журнал изменений (")}{journal.length})</summary>
            <ul className="changes">
              {[...journal].sort((a, b) => b.at - a.at).map((j, i) => (
                <li key={i}>
                  <span className="when muted small">{when(j.at)}</span>
                  <span className="grow"><b>{tc(j.action)}</b> {j.what} <span className="muted">— {tc(j.reason)}</span></span>
                </li>
              ))}
            </ul>
          </details>
        )}
      </div>

      <section className="card">
        <h3>{t("Миры и их бэкапы")}</h3>
        <p className="muted small" style={{ margin: "0 0 10px" }}>
          {t("Lime сам делает бэкап последнего мира, если моды менялись после прошлой игры, и перед удалением мода, который добавляет блоки или измерения. Хранятся 5 последних бэкапов каждого мира.")}
        </p>
        {worlds.length === 0 ? <p className="muted" style={{ margin: 0 }}>{t("Миров в профиле пока нет.")}</p> : (
          <div className="worlds">
            {worlds.map((w) => (
              <Fragment key={w.folder}>
                <div className="world">
                  <div className="grow">
                    <b>{w.name}</b>{w.hardcore && <span className="badge err" style={{ marginLeft: 6 }}>{t("хардкор")}</span>}
                    <div className="muted small">
                      {w.version_name ?? t("версия неизвестна")} · {tc(w.game_mode)} · {sizeText(w.size)}{w.last_played ? t(" · играли {0}", relTime(w.last_played)) : ""}
                      {w.folder !== w.name && <> {t("· папка")} <span className="mono">{w.folder}</span></>}
                    </div>
                  </div>
                  <div className="acts">
                    {w.backups.length > 0 && (
                      <button className="btn link small" aria-expanded={open === w.folder} onClick={() => setOpen(open === w.folder ? "" : w.folder)}>
                        {t("Бэкапы (")}{w.backups.length})
                      </button>
                    )}
                    <button className="btn" disabled={running || busy !== ""} onClick={() => act(`w${w.folder}`, async () => {
                      const b = await snapshots.backupWorld(inst.id, w.folder);
                      await dialog.alert(t("Бэкап готов"), `${w.name}: ${sizeText(b.size)}.`);
                    }, t("Бэкап не создан"))}>{busy === `w${w.folder}` ? t("Сохраняем…") : t("Сделать бэкап")}</button>
                  </div>
                </div>
                {open === w.folder && w.backups.map((b) => (
                  <div className="world sub" key={b.file}>
                    <div className="grow"><b>{when(b.created_at)}</b><div className="muted small">{sizeText(b.size)} · {relTime(b.created_at)}</div></div>
                    <button className="btn" disabled={running || busy !== ""} onClick={() => restoreWorld(w, b)}>{busy === `r${b.file}` ? t("Восстанавливаем…") : t("Восстановить")}</button>
                  </div>
                ))}
              </Fragment>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
