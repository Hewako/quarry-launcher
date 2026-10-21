import { useEffect, useRef, useState } from "react";
import { accountKey, api, errText, type AccountRef, type AppInfo, type DeviceCode } from "../api";
import { Avatar } from "../ui/Avatar";
import { Modal, useDialog } from "../ui/Dialog";

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
    <Modal title="Вход через Microsoft" onClose={close}>
      {error ? (
        <div className="alert err"><span style={{ userSelect: "text" }}>{error}</span></div>
      ) : !code ? (
        <p className="muted"><span className="spinner" /> Получаем код входа…</p>
      ) : (
        <div className="form">
          <p style={{ margin: 0 }}>Откройте страницу Microsoft в браузере и введите код. Пароль вводится только на сайте Microsoft — лаунчер его не видит.</p>
          <div className="row">
            <span className="code">{code.user_code}</span>
            <button className="btn" onClick={async () => { await navigator.clipboard.writeText(code.user_code); setCopied(true); }}>{copied ? "Скопирован" : "Копировать код"}</button>
          </div>
          <button className="btn pri" onClick={() => api.openLink(code.verification_uri).catch((e) => setError(errText(e)))}>Открыть {code.verification_uri.replace("https://", "")}</button>
          <p className="muted small" style={{ margin: 0 }}><span className="spinner" /> Ждём подтверждения… Код действует {Math.round(code.expires_in / 60)} мин.</p>
        </div>
      )}
      <div className="acts"><button className="btn" onClick={close}>{error ? "Закрыть" : "Отмена"}</button></div>
    </Modal>
  );
}

export function Accounts({ info, accounts, defaultKey, onChanged }: { info: AppInfo; accounts: AccountRef[]; defaultKey?: string; onChanged: () => void }) {
  const dialog = useDialog();
  const [msLogin, setMsLogin] = useState(false);
  const [elyBusy, setElyBusy] = useState(false);
  const verified = accounts.some((a) => a.provider === "microsoft" && a.owns_java_edition);

  const remove = async (a: AccountRef) => {
    const ok = await dialog.confirm({
      title: `Выйти из ${a.name}?`,
      body: "Токены аккаунта будут удалены из хранилища секретов ОС. Профили и миры не затрагиваются.",
      confirm: "Выйти",
    });
    if (!ok) return;
    try { await api.removeAccount(accountKey(a)); onChanged(); }
    catch (e) { await dialog.alert("Не удалось выйти", errText(e)); }
  };

  const ely = async () => {
    setElyBusy(true);
    try {
      const a = await api.elyLogin();
      onChanged();
      await dialog.alert("Ely.by подключён", `${a.name}: доступен в профилях, переключённых на Ely.by. Официальные серверы с ним недоступны.`);
    } catch (e) {
      await dialog.alert("Вход через Ely.by не удался", errText(e));
    } finally {
      setElyBusy(false);
    }
  };

  return (
    <>
      <h2>Аккаунты</h2>
      <p className="sub">Провайдеры не смешиваются: у каждого аккаунта свой UUID и токены. Токены лежат только в хранилище секретов ОС.</p>
      {accounts.length === 0 && (
        <div className="alert info">
          <div className="h">Аккаунтов пока нет</div>
          <span className="small">Для игры нужен аккаунт Microsoft с купленной Minecraft: Java Edition. Лаунчер проверяет покупку при входе.</span>
        </div>
      )}
      {accounts.map((a) => (
        <div className="acc" key={accountKey(a)}>
          <Avatar account={a} size={40} />
          <div className="grow">
            <b>{a.name}</b>
            <div className="row" style={{ gap: 6, marginTop: 4 }}>
              {a.provider === "microsoft" ? (
                <>
                  <span className="badge ms">Microsoft</span>
                  {a.owns_java_edition ? <span className="badge ok">Java Edition куплена</span> : <span className="badge err">Java Edition не куплена</span>}
                  {(defaultKey === accountKey(a) || (!defaultKey && a === accounts.find((x) => x.provider === "microsoft"))) && <span className="badge">по умолчанию</span>}
                </>
              ) : (
                <>
                  <span className="badge ely">Ely.by</span>
                  <span className="badge warn">только совместимые серверы</span>
                </>
              )}
            </div>
            {a.provider === "elyby" && <p className="muted small" style={{ margin: "6px 0 0" }}>Не подтверждает покупку игры и не открывает официальные серверы. Используется в профилях с входом Ely.by через authlib-injector.</p>}
          </div>
          {a.provider === "microsoft" && defaultKey !== accountKey(a) && accounts.filter((x) => x.provider === "microsoft").length > 1 && (
            <button className="btn" onClick={() => api.setDefaultAccount(accountKey(a)).then(onChanged)}>Сделать основным</button>
          )}
          <button className="btn" onClick={() => remove(a)}>Выйти</button>
        </div>
      ))}
      <div className="row" style={{ marginTop: 6 }}>
        <button className="btn pri" disabled={!info.microsoft_configured} onClick={() => setMsLogin(true)}>Войти через Microsoft</button>
        <button className="btn" disabled={!verified || elyBusy} onClick={ely}>{elyBusy ? "Ждём браузер…" : "Войти через Ely.by"}</button>
      </div>
      {!info.microsoft_configured && (
        <div className="alert warn" style={{ marginTop: 14 }}>
          <div className="h">Вход Microsoft не настроен в этой сборке</div>
          <span className="small">Нужен client ID приложения в Microsoft Azure, одобренный Mojang. Пошаговая инструкция — docs/MICROSOFT_SETUP.md в репозитории. До одобрения Minecraft Services отвечает 403 — это ожидаемо.</span>
        </div>
      )}
      {!verified && <p className="muted small">Ely.by станет доступен после входа в Microsoft-аккаунт с купленной игрой.</p>}
      <p className="muted small">Вход открывается в системном браузере. Пароль в лаунчер не вводится.</p>
      {msLogin && <MicrosoftLogin onClose={() => setMsLogin(false)} onDone={() => { setMsLogin(false); onChanged(); }} />}
    </>
  );
}
