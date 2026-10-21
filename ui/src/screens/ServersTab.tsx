// Вкладка «Серверы»: список серверов игры (servers.dat), проверка онлайна и вход в один клик.
import { useCallback, useEffect, useState } from "react";
import { api, errText, type Instance, type Server, type ServerStatus } from "../api";
import { launches } from "../launches";
import { Modal, useDialog } from "../ui/Dialog";
import { address, useStreamer } from "../streamer";
import { t } from "../i18n";

type Ping = ServerStatus | { error: string } | "loading";

function ServerForm({ initial, initialTags, onClose, onSave }: { initial: Server; initialTags: string[]; onClose: () => void; onSave: (s: Server, tags: string[]) => Promise<void> }) {
  const [s, setS] = useState(initial);
  const [tags, setTags] = useState(initialTags.join(", "));
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={initial.ip ? t("Изменить сервер") : t("Добавить сервер")} onClose={onClose}>
      <label className="small muted" htmlFor="srv-name">{t("Название")}</label>
      <input id="srv-name" className="field" maxLength={64} value={s.name} onChange={(e) => setS({ ...s, name: e.target.value })} placeholder={t("Сервер с друзьями")} />
      <label className="small muted" htmlFor="srv-ip">{t("Адрес")}</label>
      <input id="srv-ip" className="field mono" value={s.ip} onChange={(e) => setS({ ...s, ip: e.target.value.trim() })} placeholder={t("play.example.org или 1.2.3.4:25565")} />
      <label className="small muted" htmlFor="srv-tex">{t("Ресурспак сервера")}</label>
      <select id="srv-tex" className="field" value={s.accept_textures === undefined ? "ask" : s.accept_textures ? "yes" : "no"}
        onChange={(e) => setS({ ...s, accept_textures: e.target.value === "ask" ? undefined : e.target.value === "yes" })}>
        <option value="ask">{t("Спрашивать")}</option>
        <option value="yes">{t("Принимать")}</option>
        <option value="no">{t("Отклонять")}</option>
      </select>
      <label className="small muted" htmlFor="srv-tags">{t("Теги через запятую")}</label>
      <input id="srv-tags" className="field" value={tags} onChange={(e) => setTags(e.target.value)} placeholder={t("друзья, выживание")} />
      <div className="acts">
        <button className="btn" onClick={onClose}>{t("Отмена")}</button>
        <button className="btn pri" disabled={busy || !s.name.trim() || !s.ip} onClick={async () => { setBusy(true); try { await onSave(s, tags.split(",").map((x) => x.trim()).filter(Boolean)); } finally { setBusy(false); } }}>
          {busy ? t("Сохраняем…") : t("Сохранить")}
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
  const [tags, setTagsMap] = useState<Record<string, string[]>>({});
  const [tagFilter, setTagFilter] = useState("");
  useEffect(() => { api.serverTags(inst.id).then(setTagsMap).catch(() => setTagsMap({})); }, [inst.id]);
  const allTags = [...new Set(Object.values(tags).flat())].sort();
  const streamer = useStreamer();

  const ping = useCallback((ip: string) => {
    setPings((p) => ({ ...p, [ip]: "loading" }));
    api.pingServer(ip)
      .then((s) => setPings((p) => ({ ...p, [ip]: s })))
      .catch((e) => setPings((p) => ({ ...p, [ip]: { error: errText(e) } })));
  }, []);
  const load = useCallback(() => {
    api.servers(inst.id).then((l) => { setList(l); l.forEach((s) => ping(s.ip)); }).catch((e) => { setList([]); dialog.alert(t("Список серверов не прочитан"), errText(e)); });
  }, [inst.id, ping, dialog]);
  useEffect(load, [load]);

  const save = async (next: Server[]) => {
    try { await api.saveServers(inst.id, next); setList(next); } catch (e) { await dialog.alert(t("Не сохранено"), errText(e)); throw e; }
  };
  const play = async (s: Server) => {
    launches.begin(inst.id);
    try { await api.launch(inst.id, { kind: "multiplayer", target: s.ip }); } catch (e) { await dialog.alert(t("Не удалось начать запуск"), errText(e)); }
  };
  const remove = async (i: number) => {
    const s = list![i];
    const ok = await dialog.confirm({ title: t("Убрать «{0}» из списка?", s.name), body: t("Прежний список сохранится в файле servers.dat_old в папке игры."), confirm: t("Убрать"), danger: true });
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
          {t("Тот же список, что в игре в «Сетевой игре». «Играть» запускает игру и сразу подключает к серверу. Менять список можно, только пока игра закрыта.")}
        </p>
        {allTags.length > 0 && (
          <select className="field auto" aria-label={t("Показывать серверы с тегом")} value={tagFilter} onChange={(e) => setTagFilter(e.target.value)}>
            <option value="">{t("Все серверы")}</option>
            {allTags.map((tg) => <option key={tg} value={tg}>{tg}</option>)}
          </select>
        )}
        <button className="btn" disabled={running} onClick={() => setEdit("new")}>{t("+ Добавить сервер")}</button>
      </div>
      <div className="card" style={{ padding: 0 }}>
        {list === null ? <p className="muted" style={{ padding: 14 }}><span className="spinner" /> {t("Загружаем…")}</p> : list.length === 0 ? (
          <p className="muted" style={{ padding: 14, margin: 0 }}>{t("Серверов пока нет. Добавьте адрес здесь или в самой игре.")}</p>
        ) : (
          <table className="servers">
            <tbody>
              {list.map((s, i) => {
                if (tagFilter && !(tags[s.ip] ?? []).includes(tagFilter)) return null;
                const p = pings[s.ip];
                const st = p && p !== "loading" && !("error" in p) ? p : null;
                const icon = st?.favicon ?? (s.icon ? `data:image/png;base64,${s.icon}` : undefined);
                return (
                  <tr key={`${s.ip}-${i}`}>
                    <td style={{ width: 48 }}>{icon ? <img className="sicon" src={icon} alt="" /> : <div className="sicon" aria-hidden />}</td>
                    <td>
                      <b>{s.name}</b> <span className="muted small mono">{address(s.ip, streamer)}</span>
                      {(tags[s.ip] ?? []).map((tg) => <span key={tg} className="badge" style={{ marginLeft: 6 }}>{tg}</span>)}
                      <div className="small">
                        {p === "loading" || !p ? <span className="muted"><span className="spinner" /> {t("Проверяем…")}</span>
                          : "error" in p ? <span className="muted" title={p.error}>{t("Не отвечает")}</span>
                          : <><span className="ok-text">{t("● онлайн")} {p.online}/{p.max}</span><span className="muted"> · {p.version}{p.latency_ms ? t(" · {0} мс", p.latency_ms) : ""}</span></>}
                      </div>
                      {st?.motd && <div className="muted small motd">{st.motd}</div>}
                      <div className="row sacts">
                        <button className="btn link small" aria-label={t("Поднять «{0}» выше", s.name)} disabled={running || i === 0} onClick={() => move(i, -1)}>{t("↑ выше")}</button>
                        <button className="btn link small" aria-label={t("Опустить «{0}» ниже", s.name)} disabled={running || i === list.length - 1} onClick={() => move(i, 1)}>{t("↓ ниже")}</button>
                        <button className="btn link small" disabled={running} onClick={() => setEdit(i)}>{t("Изменить")}</button>
                        <button className="btn link small" disabled={running} onClick={() => remove(i)}>{t("Убрать")}</button>
                      </div>
                    </td>
                    <td style={{ textAlign: "right", whiteSpace: "nowrap" }}>
                      <button className="btn" onClick={() => ping(s.ip)}>{t("Проверить")}</button>
                      <button className="btn pri" disabled={running} onClick={() => play(s)}>{t("Играть")}</button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
      {edit !== null && (
        <ServerForm
          initial={edit === "new" ? { name: "", ip: "" } : list![edit]}
          initialTags={edit === "new" ? [] : tags[list![edit].ip] ?? []}
          onClose={() => setEdit(null)}
          onSave={async (s, newTags) => {
            const next = edit === "new" ? [...list!, s] : list!.map((x, j) => (j === edit ? s : x));
            await save(next);
            await api.setServerTags(inst.id, s.ip, newTags).catch(() => {});
            api.serverTags(inst.id).then(setTagsMap).catch(() => {});
            setEdit(null);
            ping(s.ip);
          }}
        />
      )}
    </>
  );
}
