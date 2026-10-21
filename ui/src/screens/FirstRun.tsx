// Мастер первого запуска: язык, папка данных и место, Java, вход, первый профиль, приватность.
// По одной мысли на шаг; любой шаг можно пропустить — всё меняется потом в настройках.
import { useEffect, useState } from "react";
import { api, sizeText } from "../api";

type Info = Awaited<ReturnType<typeof api.firstRunInfo>>;
const STEPS = ["Язык", "Папка", "Java", "Вход", "Профиль", "Приватность"];

export function FirstRun({ onDone }: { onDone: (next: "accounts" | "create" | "import" | "home") => void }) {
  const [step, setStep] = useState(0);
  const [lang, setLang] = useState("ru");
  const [info, setInfo] = useState<Info | null>(null);
  const [next, setNext] = useState<"accounts" | "create" | "import" | "home">("home");
  useEffect(() => { api.firstRunInfo().then(setInfo).catch(() => setInfo({ data_dir: "", javas: [] })); }, []);

  const finish = async () => { await api.firstRunDone(lang).catch(() => {}); onDone(next); };
  const forward = () => (step < STEPS.length - 1 ? setStep(step + 1) : finish());
  const lowSpace = info?.free_bytes !== undefined && info.free_bytes < 5 * 1024 ** 3;

  return (
    <div className="overlay">
      <div className="dialog wizard" role="dialog" aria-modal="true" aria-labelledby="wiz-title">
        <ol className="steps" aria-label="Шаги">
          {STEPS.map((s, i) => <li key={s} className={i === step ? "on" : i < step ? "done" : ""} aria-current={i === step ? "step" : undefined}>{s}</li>)}
        </ol>

        {step === 0 && (
          <>
            <h3 id="wiz-title">Добро пожаловать в Quarry</h3>
            <p>Пара коротких шагов — и можно играть. Всё это потом меняется в настройках.</p>
            <label className="small muted" htmlFor="wiz-lang">Язык интерфейса</label>
            <select id="wiz-lang" className="field" value={lang} onChange={(e) => setLang(e.target.value)} data-autofocus>
              <option value="ru">Русский</option>
              <option value="en">English</option>
            </select>
          </>
        )}
        {step === 1 && (
          <>
            <h3 id="wiz-title">Где Quarry хранит игру</h3>
            <p>Версии игры, моды, миры и настройки лежат в одной папке:</p>
            <p className="mono small" style={{ userSelect: "text" }}>{info?.data_dir || "…"}</p>
            {info?.free_bytes !== undefined && <p>Свободно на диске: <b>{sizeText(info.free_bytes)}</b>.</p>}
            {lowSpace && <div className="alert warn"><span>Места мало. Одна версия игры с модами занимает 1–3 ГБ — освободите место заранее.</span></div>}
            <p className="muted small">Папку любого профиля потом можно перенести на другой диск (вкладка «Профиль»).</p>
          </>
        )}
        {step === 2 && (
          <>
            <h3 id="wiz-title">Java</h3>
            {info === null ? <p className="muted"><span className="spinner" /> Ищем Java на компьютере…</p> : info.javas.length > 0 ? (
              <>
                <p>Нашли на компьютере:</p>
                <ul className="small">{info.javas.slice(0, 5).map((j) => <li key={j.path}>Java {j.major} ({j.arch})</li>)}</ul>
                <p className="muted small">Для каждой версии игры Quarry сам выберет подходящую Java, а если нужной нет — скачает её.</p>
              </>
            ) : <p>Java не найдена — это не страшно. Quarry сам скачает нужную версию при первом запуске игры.</p>}
          </>
        )}
        {step === 3 && (
          <>
            <h3 id="wiz-title">Вход</h3>
            <p>Чтобы играть, нужен аккаунт Microsoft, на котором куплена Minecraft: Java Edition. Пароль вводится на сайте Microsoft — Quarry его не видит.</p>
            <div className="row">
              <button className={`btn ${next === "accounts" ? "pri" : ""}`} onClick={() => { setNext("accounts"); setStep(4); }}>Войти после мастера</button>
              <button className="btn" onClick={() => setStep(4)}>Войти позже</button>
            </div>
          </>
        )}
        {step === 4 && (
          <>
            <h3 id="wiz-title">Первый профиль</h3>
            <p>Профиль — это отдельная игра со своей версией, модами и мирами. Профилей может быть сколько угодно, и они не мешают друг другу.</p>
            <div className="row">
              <button className={`btn ${next === "create" ? "pri" : ""}`} onClick={() => { setNext("create"); setStep(5); }}>Создать профиль</button>
              <button className={`btn ${next === "import" ? "pri" : ""}`} onClick={() => { setNext("import"); setStep(5); }}>Импортировать сборку</button>
              <button className="btn" onClick={() => setStep(5)}>Пропустить</button>
            </div>
          </>
        )}
        {step === 5 && (
          <>
            <h3 id="wiz-title">Ваши данные</h3>
            <ul className="small privacy">
              <li>Quarry не собирает статистику и ничего не отправляет разработчикам.</li>
              <li>Лаунчер обращается только к нужным сервисам: Mojang и Microsoft (игра и вход), Modrinth и CurseForge (моды), Adoptium (Java), GitHub (проверка новой версии Quarry).</li>
              <li>Данные входа хранятся в защищённом хранилище системы, а не в файлах.</li>
              <li>Из логов и отчётов скрываются токены, путь к домашней папке и IP-адреса.</li>
            </ul>
          </>
        )}

        <div className="acts">
          {step > 0 && <button className="btn" onClick={() => setStep(step - 1)}>Назад</button>}
          <button className="btn link" onClick={finish}>Пропустить мастер</button>
          <button className="btn pri" onClick={forward}>{step === STEPS.length - 1 ? "Начать" : "Дальше"}</button>
        </div>
      </div>
    </div>
  );
}
