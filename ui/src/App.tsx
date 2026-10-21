import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

interface Profile {
  id: string;
  name: string;
  mc_version: string;
  loader: string | null;
}

const NAV = ["Главная", "Профили", "Mod Doctor", "Снимки и откат", "Логи и краши", "Аккаунты и скины", "Настройки"];
const VERSIONS = ["26.3", "1.21.11", "1.20.1", "1.16.5"];

function App() {
  const [active, setActive] = useState("Главная");
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [launchingId, setLaunchingId] = useState<string | null>(null);
  
  // Состояние для модального окна
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [newName, setNewName] = useState("");
  const [newVersion, setNewVersion] = useState(VERSIONS[0]);
  const [newLoader, setNewLoader] = useState("vanilla");

  const refresh = () => {
    invoke<Profile[]>("get_profiles").then(setProfiles).catch(console.error);
  };

  useEffect(() => { refresh(); }, []);

  const handleDelete = async (id: string) => {
    if (!confirm(`Удалить профиль «${id}»? Это действие необратимо.`)) return;
    try {
      await invoke("delete_profile", { id });
      refresh();
    } catch (e) {
      alert(`Ошибка: ${e}`);
    }
  };

  const handleLaunch = async (id: string) => {
    try {
      setLaunchingId(id);
      // Запускаем и не ждем окончания загрузки (для UX лучше вынести в события, но пока так)
      invoke("launch_profile", { id }).catch(e => alert(`Ошибка фонового запуска: ${e}`));
      
      // Возвращаем кнопку в исходное состояние через 3 секунды, 
      // чтобы не висела вечно (так как мы не ждем полной загрузки)
      setTimeout(() => setLaunchingId(null), 3000);
    } catch (e) {
      alert(`Ошибка: ${e}`);
      setLaunchingId(null);
    }
  };

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newName.trim()) return;
    try {
      await invoke("create_profile", { 
        name: newName.trim(), 
        version: newVersion, 
        loader: newLoader === "vanilla" ? null : newLoader 
      });
      setIsModalOpen(false);
      setNewName("");
      refresh();
    } catch (err) {
      alert(`Ошибка создания: ${err}`);
    }
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="logo">
          <div className="logo-icon" />
          <div className="logo-text">
            Quarry
            <small>рабочее название · v0.3</small>
          </div>
        </div>
        <nav className="nav">
          {NAV.map((item) => (
            <button
              key={item}
              className={`nav-item ${active === item ? "active" : ""}`}
              onClick={() => setActive(item)}
            >
              {item}
            </button>
          ))}
        </nav>
      </aside>

      <main className="main">
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "20px" }}>
          <h1 style={{ margin: 0 }}>{active}</h1>
          {active === "Главная" && (
            <button className="btn-primary" style={{ padding: "8px 16px", flex: "none" }} onClick={() => setIsModalOpen(true)}>
              + Новый профиль
            </button>
          )}
        </div>

        {active === "Главная" && (
          <>
            {profiles.length === 0 ? (
              <div className="card"><p>Нет профилей. Создайте первый!</p></div>
            ) : (
              <div className="profile-grid">
                {profiles.map(p => (
                  <div key={p.id} className="profile-card">
                    <div className="profile-head">
                      <div className="profile-icon" />
                      <div>
                        <strong>{p.name}</strong>
                        <div className="profile-meta">
                          {p.mc_version} {p.loader ? `· ${p.loader}` : "· Vanilla"}
                        </div>
                      </div>
                    </div>
                    <div className="profile-actions">
                      <button
                        className="btn-primary"
                        onClick={() => handleLaunch(p.id)}
                        disabled={launchingId === p.id}
                      >
                        {launchingId === p.id ? "Подготовка..." : "▶ Играть"}
                      </button>
                      <button className="btn-ghost" onClick={() => handleDelete(p.id)} title="Удалить">🗑</button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </>
        )}

        {active !== "Главная" && (
          <div className="card">
            <p>Экран «{active}» появится в следующей итерации.</p>
          </div>
        )}
      </main>

      {/* Модальное окно создания профиля */}
      {isModalOpen && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>Новый профиль</h2>
            <form onSubmit={handleCreate} style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
              <div>
                <label style={{ display: "block", marginBottom: "4px", fontSize: "13px", color: "var(--muted)" }}>Название</label>
                <input 
                  className="input" 
                  type="text" 
                  value={newName} 
                  onChange={e => setNewName(e.target.value)} 
                  placeholder="Например: Технология+" 
                  autoFocus
                />
              </div>
              <div>
                <label style={{ display: "block", marginBottom: "4px", fontSize: "13px", color: "var(--muted)" }}>Версия Minecraft</label>
                <select className="input" value={newVersion} onChange={e => setNewVersion(e.target.value)}>
                  {VERSIONS.map(v => <option key={v} value={v}>{v}</option>)}
                </select>
              </div>
              <div>
                <label style={{ display: "block", marginBottom: "4px", fontSize: "13px", color: "var(--muted)" }}>Загрузчик</label>
                <select className="input" value={newLoader} onChange={e => setNewLoader(e.target.value)}>
                  <option value="vanilla">Vanilla</option>
                  <option value="fabric">Fabric</option>
                </select>
              </div>
              <div style={{ display: "flex", gap: "8px", marginTop: "8px" }}>
                <button type="submit" className="btn-primary" style={{ padding: "10px" }}>Создать</button>
                <button type="button" className="btn-ghost" style={{ padding: "10px" }} onClick={() => setIsModalOpen(false)}>Отмена</button>
              </div>
            </form>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
