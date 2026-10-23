import { useEffect, useRef, useState } from "react";
import { accountKey, api, errText, type AccountRef, type AppInfo, type DeviceCode } from "../api";
import { Avatar } from "../ui/Avatar";
import { Modal, useDialog } from "../ui/Dialog";
import { nick, useStreamer } from "../streamer";
import { t } from "../i18n";

function MicrosoftLogin({ onClose, onDone }: { onClose: () => void; onDone: (a: AccountRef) => void }) {
  const [code, setCode] = useState<DeviceCode | null>(null);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  // Вход запускается один раз за открытие диалога, даже если родитель перерисуется.
  const done = useRef(onDone);
  done.current = onDone;

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const c = await api.microsoftStart();
        if (!alive) return;
        setCode(c);
        const acc = await api.microsoftFinish(c);
        if (alive) done.current(acc);
      } catch (e) {
        if (alive) setError(errText(e));
      }
    })();
    return () => { alive = false; };
  }, []);

  const close = () => { api.cancelLogin().catch(() => {}); onClose(); };
  return (
    <Modal title={t("Вход через Microsoft")} onClose={close}>
      {error ? (
        <div className="alert err"><span style={{ userSelect: "text" }}>{error}</span></div>
      ) : !code ? (
        <p className="muted"><span className="spinner" /> {t("Получаем код входа…")}</p>
      ) : (
        <div className="form">
          <p style={{ margin: 0 }}>{t("Откройте страницу Microsoft в браузере и введите код. Пароль вводится только на сайте Microsoft — лаунчер его не видит.")}</p>
          <div className="row">
            <span className="code">{code.user_code}</span>
            <button className="btn" onClick={async () => { await navigator.clipboard.writeText(code.user_code); setCopied(true); }}>{copied ? t("Скопирован") : t("Копировать код")}</button>
          </div>
          <button className="btn pri" onClick={() => api.openLink(code.verification_uri).catch((e) => setError(errText(e)))}>{t("Открыть")} {code.verification_uri.replace("https://", "")}</button>
          <p className="muted small" style={{ margin: 0 }}><span className="spinner" /> {t("Ждём подтверждения… Код действует")} {Math.round(code.expires_in / 60)} {t("мин.")}</p>
        </div>
      )}
      <div className="acts"><button className="btn" onClick={close}>{error ? t("Закрыть") : t("Отмена")}</button></div>
    </Modal>
  );
}

function ElyLogin({ onClose, onDone }: { onClose: () => void; onDone: (a: AccountRef) => void }) {
  const [login, setLogin] = useState("");
  const [password, setPassword] = useState("");
  const [totp, setTotp] = useState("");
  const [needTotp, setNeedTotp] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async () => {
    setBusy(true);
    setError("");
    try {
      const r = await api.elyLoginPassword(login, password, needTotp ? totp : undefined);
      if (r.need_totp) { setNeedTotp(true); return; }
      setPassword("");
      if (r.account) onDone(r.account);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title={t("Вход через Ely.by")} onClose={onClose}>
      <form className="form" onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <label><span>{t("Ник или e-mail")}</span><input className="field" value={login} onChange={(e) => setLogin(e.target.value)} autoComplete="username" data-autofocus /></label>
        <label><span>{t("Пароль")}</span><input className="field" type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" /></label>
        {needTotp && (
          <label><span>{t("Код двухфакторной защиты")}</span><input className="field" inputMode="numeric" value={totp} onChange={(e) => setTotp(e.target.value)} autoFocus /></label>
        )}
        <span className="muted small">{t("Пароль отправляется только на authserver.ely.by и не сохраняется: в хранилище секретов ОС попадает только токен сессии. Ely.by-аккаунт работает лишь на совместимых серверах.")}</span>
        {error && <div className="alert err"><span style={{ userSelect: "text" }}>{error}</span></div>}
        <div className="acts">
          <button type="button" className="btn" onClick={onClose}>{t("Отмена")}</button>
          <button className="btn pri" disabled={busy || !login || !password || (needTotp && !totp)}>{busy ? t("Входим…") : t("Войти")}</button>
        </div>
      </form>
    </Modal>
  );
}

export function Accounts({ info, accounts, defaultKey, onChanged }: { info: AppInfo; accounts: AccountRef[]; defaultKey?: string; onChanged: () => void }) {
  const streamer = useStreamer();
  const [skin, setSkin] = useState<Record<string, string | null>>({});
  const showSkin = (key: string) => {
    if (key in skin) return;
    api.accountSkin(key).then((url) => setSkin((s) => ({ ...s, [key]: url }))).catch(() => setSkin((s) => ({ ...s, [key]: null })));
  };
  const dialog = useDialog();
  const [msLogin, setMsLogin] = useState(false);
  const [elyBusy, setElyBusy] = useState(false);
  const [elyPassword, setElyPassword] = useState(false);
  // Справа — скин выбранного аккаунта (сначала основного).
  const [sel, setSel] = useState<string | null>(null);
  const selAcc = accounts.find((a) => accountKey(a) === sel) ?? accounts.find((a) => accountKey(a) === defaultKey) ?? accounts[0];
  const selKey = selAcc ? accountKey(selAcc) : undefined;
  useEffect(() => { if (selKey) showSkin(selKey); }, [selKey]);
  const verified = accounts.some((a) => a.provider === "microsoft" && a.owns_java_edition);

  const remove = async (a: AccountRef) => {
    const ok = await dialog.confirm({
      title: t("Выйти из {0}?", a.name),
      body: t("Токены аккаунта будут удалены из хранилища секретов ОС. Профили и миры не затрагиваются."),
      confirm: t("Выйти"),
    });
    if (!ok) return;
    try { await api.removeAccount(accountKey(a)); onChanged(); }
    catch (e) { await dialog.alert(t("Не удалось выйти"), errText(e)); }
  };

  const ely = async () => {
    setElyBusy(true);
    try {
      const a = await api.elyLogin();
      onChanged();
      await dialog.alert(t("Ely.by подключён"), t("{0}: доступен в профилях, переключённых на Ely.by. Официальные серверы с ним недоступны.", a.name));
    } catch (e) {
      await dialog.alert(t("Вход через Ely.by не удался"), errText(e));
    } finally {
      setElyBusy(false);
    }
  };

  return (
    <>
      <h2>{t("Аккаунты и скины")}</h2>
      <p className="sub">{t("Провайдеры не смешиваются: у каждого аккаунта свой UUID и токены. Токены лежат только в хранилище секретов ОС.")}</p>
      <div className="ov">
      <section className="card">
      <h3>{t("Аккаунты")}</h3>
      {accounts.length === 0 && (
        <div className="alert info">
          <div className="h">{t("Аккаунтов пока нет")}</div>
          <span className="small">{t("Для игры нужен аккаунт Microsoft с купленной Minecraft: Java Edition. Лаунчер проверяет покупку при входе.")}</span>
        </div>
      )}
      {accounts.map((a) => (
        <div className="acc" key={accountKey(a)}>
          <Avatar account={a} size={40} />
          <div className="grow">
            <b>{nick(a.name, streamer)}</b>{" "}
            <button className="btn link small" aria-pressed={selKey === accountKey(a)} onClick={() => setSel(accountKey(a))}>{t("Скин")}</button>
            <div className="row" style={{ gap: 6, marginTop: 4 }}>
              {a.provider === "microsoft" ? (
                <>
                  <span className="badge ms">Microsoft</span>
                  {a.owns_java_edition ? <span className="badge ok">{t("Java Edition куплена")}</span> : <span className="badge err">{t("Java Edition не куплена")}</span>}
                  {(defaultKey === accountKey(a) || (!defaultKey && a === accounts.find((x) => x.provider === "microsoft"))) && <span className="badge">{t("по умолчанию")}</span>}
                </>
              ) : (
                <>
                  <span className="badge ely">Ely.by</span>
                  <span className="badge warn">{t("только совместимые серверы")}</span>
                </>
              )}
            </div>
            {a.provider === "elyby" && <p className="muted small" style={{ margin: "6px 0 0" }}>{t("Не подтверждает покупку игры и не открывает официальные серверы. Используется в профилях с входом Ely.by через authlib-injector.")}</p>}
          </div>
          {a.provider === "microsoft" && defaultKey !== accountKey(a) && accounts.filter((x) => x.provider === "microsoft").length > 1 && (
            <button className="btn" onClick={() => api.setDefaultAccount(accountKey(a)).then(onChanged)}>{t("Сделать основным")}</button>
          )}
          <button className="btn" onClick={() => remove(a)}>{t("Выйти")}</button>
        </div>
      ))}
      <div className="row" style={{ marginTop: 12 }}>
        <button className="btn cta" disabled={!info.microsoft_configured} onClick={() => setMsLogin(true)}>{t("Войти через Microsoft")}</button>
        <button className="btn" disabled={!verified} onClick={() => setElyPassword(true)}>{t("Войти через Ely.by")}</button>
        {verified && <button className="btn link small" disabled={elyBusy} onClick={ely}>{elyBusy ? t("Ждём браузер…") : t("или через браузер (OAuth)")}</button>}
      </div>
      {!info.microsoft_configured && (
        <div className="alert warn" style={{ marginTop: 14 }}>
          <div className="h">{t("Вход Microsoft не настроен в этой сборке")}</div>
          <span className="small">{t("Нужен client ID приложения в Microsoft Azure, одобренный Mojang. Пошаговая инструкция — docs/MICROSOFT_SETUP.md в репозитории. До одобрения Minecraft Services отвечает 403 — это ожидаемо.")}</span>
        </div>
      )}
      {!verified && <p className="muted small">{t("Ely.by станет доступен после входа в Microsoft-аккаунт с купленной игрой.")}</p>}
      <p className="muted small">{t("Вход открывается в системном браузере. Пароль в лаунчер не вводится.")}</p>
      </section>
      {selAcc && (
        <section className="card skincard" aria-label={t("Скин")}>
          <h3>{t("Скин")} {nick(selAcc.name, streamer)}</h3>
          {skin[selKey!] === undefined ? <p className="muted"><span className="spinner" /> {t("Загружаем…")}</p>
            : skin[selKey!] ? <img className="skinfig" src={skin[selKey!]!} alt={t("Скин спереди и сзади")} /> : <p className="muted small">{t("Скина нет — в игре будет стандартный Steve или Alex.")}</p>}
          <p className="muted small">{selAcc.provider === "elyby" ? t("Скин Ely.by виден только на серверах, которые используют Ely.by. Сменить его можно на ely.by.") : t("Сменить скин можно на minecraft.net.")}</p>
        </section>
      )}
      </div>
      {elyPassword && (
        <ElyLogin onClose={() => setElyPassword(false)} onDone={async (a) => {
          setElyPassword(false);
          onChanged();
          await dialog.alert(t("Ely.by подключён"), t("{0}: доступен в профилях, переключённых на Ely.by. Официальные серверы с ним недоступны.", a.name));
        }} />
      )}
      {msLogin && <MicrosoftLogin onClose={() => setMsLogin(false)} onDone={() => { setMsLogin(false); onChanged(); }} />}
    </>
  );
}
