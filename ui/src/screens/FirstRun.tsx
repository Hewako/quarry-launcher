// Мастер первого запуска: язык, папка данных и место, Java, вход, первый профиль, приватность.
// По одной мысли на шаг; любой шаг можно пропустить — всё меняется потом в настройках.
import { useEffect, useState } from "react";
import { api, sizeText } from "../api";
import { currentLang, switchLang, t } from "../i18n";

type Info = Awaited<ReturnType<typeof api.firstRunInfo>>;
const STEPS = [t("Язык"), t("Папка"), "Java", t("Вход"), t("Профиль"), t("Приватность")];

export function FirstRun({ onDone }: { onDone: (next: "accounts" | "create" | "import" | "home") => void }) {
  const [step, setStep] = useState(0);
  const [lang, setLang] = useState("ru");
  const [info, setInfo] = useState<Info | null>(null);
  const [next, setNext] = useState<"accounts" | "create" | "import" | "home">("home");
  useEffect(() => { api.firstRunInfo().then(setInfo).catch(() => setInfo({ data_dir: "", javas: [] })); }, []);

  const finish = async () => { await api.firstRunDone(lang).catch(() => {}); onDone(next); if (lang !== currentLang()) switchLang(lang); };
  const forward = () => (step < STEPS.length - 1 ? setStep(step + 1) : finish());
  const lowSpace = info?.free_bytes !== undefined && info.free_bytes < 5 * 1024 ** 3;

  return (
    <div className="overlay">
      <div className="dialog wizard" role="dialog" aria-modal="true" aria-labelledby="wiz-title">
        <ol className="steps" aria-label={t("Шаги")}>
          {STEPS.map((s, i) => <li key={s} className={i === step ? "on" : i < step ? "done" : ""} aria-current={i === step ? "step" : undefined}>{s}</li>)}
        </ol>

        {step === 0 && (
          <>
            <h3 id="wiz-title">{t("Добро пожаловать в Axel'")}</h3>
            <p>{t("Пара коротких шагов — и можно играть. Всё это потом меняется в настройках.")}</p>
            <label className="small muted" htmlFor="wiz-lang">{t("Язык интерфейса")}</label>
            <select id="wiz-lang" className="field" value={lang} onChange={(e) => setLang(e.target.value)} data-autofocus>
              <option value="ru">{t("Русский")}</option>
              <option value="en">English</option>
            </select>
          </>
        )}
        {step === 1 && (
          <>
            <h3 id="wiz-title">{t("Где Axel' хранит игру")}</h3>
            <p>{t("Версии игры, моды, миры и настройки лежат в одной папке:")}</p>
            <p className="mono small" style={{ userSelect: "text" }}>{info?.data_dir || "…"}</p>
            {info?.free_bytes !== undefined && <p>{t("Свободно на диске:")} <b>{sizeText(info.free_bytes)}</b>.</p>}
            {lowSpace && <div className="alert warn"><span>{t("Места мало. Одна версия игры с модами занимает 1–3 ГБ — освободите место заранее.")}</span></div>}
            <p className="muted small">{t("Папку любого профиля потом можно перенести на другой диск (вкладка «Профиль»).")}</p>
          </>
        )}
        {step === 2 && (
          <>
            <h3 id="wiz-title">Java</h3>
            {info === null ? <p className="muted"><span className="spinner" /> {t("Ищем Java на компьютере…")}</p> : info.javas.length > 0 ? (
              <>
                <p>{t("Нашли на компьютере:")}</p>
                <ul className="small">{info.javas.slice(0, 5).map((j) => <li key={j.path}>Java {j.major} ({j.arch})</li>)}</ul>
                <p className="muted small">{t("Для каждой версии игры Axel' сам выберет подходящую Java, а если нужной нет — скачает её.")}</p>
              </>
            ) : <p>{t("Java не найдена — это не страшно. Axel' сам скачает нужную версию при первом запуске игры.")}</p>}
          </>
        )}
        {step === 3 && (
          <>
            <h3 id="wiz-title">{t("Вход")}</h3>
            <p>{t("Чтобы играть, нужен аккаунт Microsoft, на котором куплена Minecraft: Java Edition. Пароль вводится на сайте Microsoft — Axel' его не видит.")}</p>
            <div className="row">
              <button className={`btn ${next === "accounts" ? "pri" : ""}`} onClick={() => { setNext("accounts"); setStep(4); }}>{t("Войти после мастера")}</button>
              <button className="btn" onClick={() => setStep(4)}>{t("Войти позже")}</button>
            </div>
          </>
        )}
        {step === 4 && (
          <>
            <h3 id="wiz-title">{t("Первый профиль")}</h3>
            <p>{t("Профиль — это отдельная игра со своей версией, модами и мирами. Профилей может быть сколько угодно, и они не мешают друг другу.")}</p>
            <div className="row">
              <button className={`btn ${next === "create" ? "pri" : ""}`} onClick={() => { setNext("create"); setStep(5); }}>{t("Создать профиль")}</button>
              <button className={`btn ${next === "import" ? "pri" : ""}`} onClick={() => { setNext("import"); setStep(5); }}>{t("Импортировать сборку")}</button>
              <button className="btn" onClick={() => setStep(5)}>{t("Пропустить")}</button>
            </div>
          </>
        )}
        {step === 5 && (
          <>
            <h3 id="wiz-title">{t("Ваши данные")}</h3>
            <ul className="small privacy">
              <li>{t("Axel' не собирает статистику и ничего не отправляет разработчикам.")}</li>
              <li>{t("Лаунчер обращается только к нужным сервисам: Mojang и Microsoft (игра и вход), Modrinth и CurseForge (моды), Adoptium (Java), GitHub (проверка новой версии Axel').")}</li>
              <li>{t("Данные входа хранятся в защищённом хранилище системы, а не в файлах.")}</li>
              <li>{t("Из логов и отчётов скрываются токены, путь к домашней папке и IP-адреса.")}</li>
            </ul>
            <button className="btn link small" onClick={() => api.openLink("https://github.com/Hewako/quarry-launcher/blob/main/docs/PRIVACY.md").catch(() => {})}>{t("Подробнее о приватности")}</button>
          </>
        )}

        <div className="acts">
          {step > 0 && <button className="btn" onClick={() => setStep(step - 1)}>{t("Назад")}</button>}
          <button className="btn link" onClick={finish}>{t("Пропустить мастер")}</button>
          <button className="btn pri" onClick={forward}>{step === STEPS.length - 1 ? t("Начать") : t("Дальше")}</button>
        </div>
      </div>
    </div>
  );
}
