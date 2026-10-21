// Вкладка «Серверы»: список серверов игры (servers.dat), проверка онлайна и вход в один клик.
import { useCallback, useEffect, useState } from "react";
import { api, errText, type Instance, type Server, type ServerStatus } from "../api";
import { launches } from "../launches";
import { Modal, useDialog } from "../ui/Dialog";

type Ping = ServerStatus | { error: string } | "loading";

function ServerForm({ initial, onClose, onSave }: { initial: Server; onClose: () => void; onSave: (s: Server) => Promise<void> }) {
  const [s, setS] = useState(initial);
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={initial.ip ? "Изменить сервер" : "Добавить сервер"} onClose={onClose}>
      <label className="small muted" htmlFor="srv-name">Название</label>
      <input id="srv-name" className="field" maxLength={64} value={s.name} onChange={(e) => setS({ ...s, name: e.target.value })} placeholder="Сервер с друзьями" />
      <label className="small muted" htmlFor="srv-ip">Адрес</label>
      <input id="srv-ip" className="field mono" value={s.ip} onChange={(e) => setS({ ...s, ip: e.target.value.trim() })} placeholder="play.example.org или 1.2.3.4:25565" />
      <label className="small muted" htmlFor="srv-tex">Ресурспак сервера</label>
      <select id="srv-tex" className="field" value={s.accept_textures === undefined ? "ask" : s.accept_textures ? "yes" : "no"}
        onChange={(e) => setS({ ...s, accept_textures: e.target.value === "ask" ? undefined : e.target.value === "yes" })}>
        <option value="ask">Спрашивать</option>
        <option value="yes">Принимать</option>
        <option value="no">Отклонять</option>
      </select>
      <div className="acts">
        <button className="btn" onClick={onClose}>Отмена</button>
        <button className="btn pri" disabled={busy || !s.name.trim() || !s.ip} onClick={async () => { setBusy(true); try { await onSave(s); } finally { setBusy(false); } }}>
          {busy ? "Сохраняем…" : "Сохранить"}
        </button>
      </div>
    </Modal>
  );
}

export function ServersTab({ inst, running }: { inst: Instance; running: boolean }) {
  const dialog = useDialog();
  const [list, setList] = useState<Server[] | null>(null);
  const [pings, setPings] = useState<Record<string, Ping>>({});
  const [edit, setEdit] = useState<number | "new" | null>(null);

  const ping = useCallback((ip: string) => {
    setPings((p) => ({ ...p, [ip]: "loading" }));
    api.pingServer(ip)
      .then((s) => setPings((p) => ({ ...p, [ip]: s })))
      .catch((e) => setPings((p) => ({ ...p, [ip]: { error: errText(e) } })));
  }, []);
  const load = useCallback(() => {
    api.servers(inst.id).then((l) => { setList(l); l.forEach((s) => ping(s.ip)); }).catch((e) => { setList([]); dialog.alert("Список серверов не прочитан", errText(e)); });
  }, [inst.id, ping, dialog]);
  useEffect(load, [load]);

  const save = async (next: Server[]) => {
    try { await api.saveServers(inst.id, next); setList(next); } catch (e) { await dialog.alert("Не сохранено", errText(e)); throw e; }
  };
  const play = async (s: Server) => {
    launches.begin(inst.id);
    try { await api.launch(inst.id, { kind: "multiplayer", target: s.ip }); } catch (e) { await dialog.alert("Не удалось начать запуск", errText(e)); }
  };
  const remove = async (i: number) => {
    const s = list![i];
    const ok = await dialog.confirm({ title: `Убрать «${s.name}» из списка?`, body: "Прежний список сохранится в файле servers.dat_old в папке игры.", confirm: "Убрать", danger: true });
    if (ok) await save(list!.filter((_, j) => j !== i)).catch(() => {});
  };
  const move = (i: number, d: -1 | 1) => {
    const next = [...list!];
    [next[i], next[i + d]] = [next[i + d], next[i]];
    save(next).catch(() => {});
  };

  return (
    <>
      <div className="pagehead" style={{ marginBottom: 14 }}>
        <p className="muted small" style={{ margin: 0, maxWidth: 680 }}>
          Тот же список, что в игре в «Сетевой игре». «Играть» запускает игру и сразу подключает к серверу. Менять список можно, только пока игра закрыта.
        </p>
        <button className="btn" disabled={running} onClick={() => setEdit("new")}>+ Добавить сервер</button>
      </div>
      <div className="card" style={{ padding: 0 }}>
        {list === null ? <p className="muted" style={{ padding: 14 }}><span className="spinner" /> Загружаем…</p> : list.length === 0 ? (
          <p className="muted" style={{ padding: 14, margin: 0 }}>Серверов пока нет. Добавьте адрес здесь или в самой игре.</p>
        ) : (
          <table className="servers">
            <tbody>
              {list.map((s, i) => {
                const p = pings[s.ip];
                const st = p && p !== "loading" && !("error" in p) ? p : null;
                const icon = st?.favicon ?? (s.icon ? `data:image/png;base64,${s.icon}` : undefined);
                return (
                  <tr key={`${s.ip}-${i}`}>
                    <td style={{ width: 48 }}>{icon ? <img className="sicon" src={icon} alt="" /> : <div className="sicon" aria-hidden />}</td>
                    <td>
                      <b>{s.name}</b> <span className="muted small mono">{s.ip}</span>
                      <div className="small">
                        {p === "loading" || !p ? <span className="muted"><span className="spinner" /> Проверяем…</span>
                          : "error" in p ? <span className="muted" title={p.error}>Не отвечает</span>
                          : <><span className="ok-text">● онлайн {p.online}/{p.max}</span><span className="muted"> · {p.version}{p.latency_ms ? ` · ${p.latency_ms} мс` : ""}</span></>}
                      </div>
                      {st?.motd && <div className="muted small motd">{st.motd}</div>}
                      <div className="row sacts">
                        <button className="btn link small" aria-label={`Поднять «${s.name}» выше`} disabled={running || i === 0} onClick={() => move(i, -1)}>↑ выше</button>
                        <button className="btn link small" aria-label={`Опустить «${s.name}» ниже`} disabled={running || i === list.length - 1} onClick={() => move(i, 1)}>↓ ниже</button>
                        <button className="btn link small" disabled={running} onClick={() => setEdit(i)}>Изменить</button>
                        <button className="btn link small" disabled={running} onClick={() => remove(i)}>Убрать</button>
                      </div>
                    </td>
                    <td style={{ textAlign: "right", whiteSpace: "nowrap" }}>
                      <button className="btn" onClick={() => ping(s.ip)}>Проверить</button>
                      <button className="btn pri" disabled={running} onClick={() => play(s)}>Играть</button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
      <p className="muted small">Если сервер использует SRV-запись DNS, проверка здесь может не дойти до него, а в игре он откроется.</p>
      {edit !== null && (
        <ServerForm
          initial={edit === "new" ? { name: "", ip: "" } : list![edit]}
          onClose={() => setEdit(null)}
          onSave={async (s) => {
            const next = edit === "new" ? [...list!, s] : list!.map((x, j) => (j === edit ? s : x));
            await save(next);
            setEdit(null);
            ping(s.ip);
          }}
        />
      )}
    </>
  );
}
