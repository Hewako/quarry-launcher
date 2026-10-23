// Вкладка «Обзор» (вариант F, макет 01): вердикт Mod Doctor, что изменилось, последний запуск; справа быстрый старт и сводка.
import { useEffect, useState } from "react";
import { api, errText, relTime, snapshots, statusOf, type AccountRef, type DoctorReport, type Instance, type JournalEntry, type QuickPlay, type ResolvedLaunch, type Server, type SnapshotInfo, type WorldInfo } from "../api";
import { isActive, launches, type LaunchState } from "../launches";
import { useDialog } from "../ui/Dialog";
import { address, nick, useStreamer } from "../streamer";
import { t, tc } from "../i18n";

/** Метка строки журнала: «Установлено» → зелёная, «Обновлено» → янтарная, «Удалено»/«Выключено» → красная. */
const TONE: Record<string, string> = { "Установлено": "ok", "Добавлено": "ok", "Включено": "ok", "Обновлено": "warn", "Удалено": "err", "Выключено": "err" };

export function Overview({ inst, state, doctor, accounts, fix, onGo, onLogs }: {
  inst: Instance;
  state?: LaunchState;
  doctor: { report: DoctorReport; journal: JournalEntry[] } | null;
  accounts: AccountRef[];
  /** Есть, когда все блокирующие замечания Mod Doctor чинятся выключением мода. */
  fix?: () => Promise<void>;
  onGo: (tab: "mods" | "snapshots") => void;
  onLogs: () => void;
}) {
  const dialog = useDialog();
  const streamer = useStreamer();
  const [worlds, setWorlds] = useState<WorldInfo[]>([]);
  const [servers, setServers] = useState<Server[]>([]);
  const [snaps, setSnaps] = useState<SnapshotInfo[]>([]);
  const [launch, setLaunch] = useState<{ resolved: ResolvedLaunch; total_memory_mb?: number } | null>(null);
  const phase = state?.phase;
  useEffect(() => {
    snapshots.worlds(inst.id).then(setWorlds).catch(() => setWorlds([]));
    api.servers(inst.id).then(setServers).catch(() => setServers([]));
    snapshots.list(inst.id).then(setSnaps).catch(() => setSnaps([]));
    api.launchSettings(inst.id).then(setLaunch).catch(() => setLaunch(null));
  }, [inst.id, phase]);

  const active = isActive(state);
  const play = async (quickPlay?: QuickPlay, safeMode = false, before?: () => Promise<void>) => {
    try {
      await before?.();
      launches.begin(inst.id);
      await api.launch(inst.id, quickPlay, false, safeMode);
    } catch (e) {
      await dialog.alert(t("Не удалось начать запуск"), errText(e));
    }
  };

  const r = doctor?.report;
  const blocking = r?.findings.filter((f) => f.severity === "blocking") ?? [];
  const warnings = r?.findings.filter((f) => f.severity === "warning") ?? [];
  const last = inst.last_launch;
  const worked = last?.outcome === "success";
  const journal = doctor?.journal ?? [];
  const changes = (worked ? journal.filter((j) => j.at > last.at) : journal).slice().sort((a, b) => b.at - a.at);
  const world = [...worlds].sort((a, b) => b.last_played - a.last_played)[0];
  const server = servers[0];
  const acc = accounts.find((a) => `${a.provider}:${a.uuid}` === inst.account) ?? accounts.find((a) => a.provider === "microsoft");
  const gb = (mb: number) => (mb / 1024).toLocaleString(undefined, { maximumFractionDigits: 1 });
  const java = launch?.resolved.java_path.value;
  const mem = launch?.resolved.max_memory_mb.value;

  return (
    <div className="ov">
      <div className="stack">
        {r && blocking.length > 0 && (
          <section className="verdict err" aria-label={t("Вердикт Mod Doctor")}>
            <h3>{t("Мешает запуску: {0}", tc(blocking[0].title))}</h3>
            <p>{tc(blocking[0].detail)}</p>
            {blocking.length > 1 && <p className="muted small">{t("И ещё замечаний, мешающих запуску: {0}", blocking.length - 1)}</p>}
            <div className="acts">
              {fix && <button className="btn cta" disabled={active} onClick={() => play(undefined, false, fix)}>{t("Починить и играть")}</button>}
              {inst.loader !== "vanilla" && <button className="btn" disabled={active} onClick={() => play(undefined, true)}>{t("Безопасный запуск")}</button>}
              <button className="btn" onClick={() => onGo("mods")}>{t("Открыть в «Моды»")}</button>
            </div>
          </section>
        )}
        {r && blocking.length === 0 && warnings.length > 0 && (
          <section className="verdict warn" aria-label={t("Вердикт Mod Doctor")}>
            <h3>{t("Можно играть, есть замечания: {0}", warnings.length)}</h3>
            <p>{tc(warnings[0].title)}</p>
            <div className="acts"><button className="btn" onClick={() => onGo("mods")}>{t("Открыть в «Моды»")}</button></div>
          </section>
        )}
        {r && r.findings.length === 0 && (
          <section className="verdict ok" aria-label={t("Вердикт Mod Doctor")}>
            <h3>{t("Всё готово к запуску")}</h3>
            <p>{inst.loader === "vanilla" ? t("Профиль без модов.") : t("Mod Doctor проверил моды: {0}, проблем нет.", r.total)}</p>
          </section>
        )}

        {(changes.length > 0 || worked) && (
          <section className="card">
            <h3>{worked ? t("Что изменилось с последнего рабочего запуска") : t("Последние изменения модов")}</h3>
            {changes.length === 0 && <p className="muted">{t("С тех пор ничего не менялось.")}</p>}
            {changes.length > 0 && (
              <ul className="changes">
                {changes.slice(0, 6).map((j, n) => (
                  <li key={n}>
                    <span className={`tag ${TONE[j.action] ?? ""}`}>{tc(j.action).toLowerCase()}</span>
                    <span className="grow">{j.what}</span>
                    <span className="muted small">{relTime(j.at)}</span>
                  </li>
                ))}
              </ul>
            )}
            {changes.length > 6 && <p className="muted small">{t("И ещё изменений: {0}", changes.length - 6)}</p>}
            <div className="row foot">
              {worked && <span className="muted small grow">{t("Игра запускалась без ошибок {0}.", relTime(last.at))}</span>}
              <button className="btn link" onClick={() => onGo("snapshots")}>{t("Сравнить со снимком")}</button>
            </div>
          </section>
        )}

        <section className="card">
          <h3>{t("Последний запуск")}</h3>
          {last ? (
            <p>
              {statusOf(inst).text}{last.duration_secs > 0 ? `, ${t("играли {0} мин", Math.max(1, Math.round(last.duration_secs / 60)))}` : ""}.
              {last.message ? ` ${tc(last.message)}` : ""}{" "}
              <button className="btn link" onClick={onLogs}>{t("Открыть лог")}</button>
            </p>
          ) : <p className="muted">{t("Профиль ещё не запускали.")}</p>}
        </section>
      </div>

      <div className="stack">
        {(world || server) && (
          <section className="card">
            <h3>{t("Быстрый старт")}</h3>
            <div className="qs">
              {world && (
                <button disabled={active} onClick={() => play({ kind: "singleplayer", target: world.folder })} aria-label={t("Играть в мир «{0}»", world.name)}>
                  <span className="grow"><b>{t("Мир «{0}»", world.name)}</b><small>{world.last_played ? t("играли {0}", relTime(world.last_played)) : tc(world.game_mode)}</small></span>▶
                </button>
              )}
              {server && (
                <button disabled={active} onClick={() => play({ kind: "multiplayer", target: server.ip })} aria-label={t("Играть на сервере «{0}»", server.name)}>
                  <span className="grow"><b>{server.name}</b><small className="mono">{address(server.ip, streamer)}</small></span>▶
                </button>
              )}
            </div>
          </section>
        )}
        <section className="card">
          <h3>{t("Сводка")}</h3>
          <dl className="kv">
            <dt>{t("Аккаунт")}</dt><dd>{acc ? nick(acc.name, streamer) : t("нет аккаунта")}</dd>
            <dt>Java</dt><dd>{java ? t("указана вручную") : t("подбирается автоматически")}</dd>
            <dt>{t("Память")}</dt><dd>{mem ? (launch?.total_memory_mb ? t("{0} ГБ из {1} ГБ", gb(mem), gb(launch.total_memory_mb)) : t("{0} ГБ", gb(mem))) : "—"}</dd>
            <dt>{t("Снимков")}</dt><dd>{snaps.length ? t("{0}, последний {1}", snaps.length, relTime(Math.max(...snaps.map((s) => s.created_at)))) : "0"}</dd>
            <dt>{t("Миры")}</dt><dd>{worlds.length}</dd>
          </dl>
        </section>
      </div>
    </div>
  );
}
