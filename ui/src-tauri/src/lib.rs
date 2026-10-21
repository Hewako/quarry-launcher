use quarry_core::profile::{Profile, ProfileStore};
use tauri::Manager;
use std::path::PathBuf;

fn data_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let root = app.path().home_dir().map_err(|e| e.to_string())?.join(".quarry");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    Ok(root)
}

#[tauri::command]
async fn get_profiles(app: tauri::AppHandle) -> Result<Vec<Profile>, String> {
    let store = ProfileStore::new(data_root(&app)?);
    store.list().map_err(|e| e.to_string())
}

#[tauri::command]
async fn create_profile(
    app: tauri::AppHandle,
    name: String,
    version: String,
    loader: Option<String>,
) -> Result<Profile, String> {
    let store = ProfileStore::new(data_root(&app)?);
    store.create(&name, &version, loader.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_profile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let store = ProfileStore::new(data_root(&app)?);
    store.delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn launch_profile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let root = data_root(&app)?;
    let store = ProfileStore::new(root.clone());
    let profile = store.get(&id).map_err(|e| e.to_string())?;
    let game_dir = store.game_dir(&id);

    let inst = quarry_core::instance::prepare(
        &profile.mc_version,
        &root,
        &game_dir,
        profile.loader.as_deref(),
    ).await.map_err(|e| format!("Ошибка подготовки: {}", e))?;

    let cfg = quarry_core::launch::LaunchConfig {
        java_path: inst.java_path.clone(),
        main_class: inst.main_class.clone(),
        classpath: inst.classpath.clone(),
        game_dir: inst.game_dir.clone(),
        assets_dir: inst.assets_dir.clone(),
        natives_dir: inst.natives_dir.clone(),
        version_id: inst.version_id.clone(),
        username: "Player".into(),
        uuid: "00000000000000000000000000000001".into(),
        access_token: "0".into(),
        version_type: "release".into(),
        asset_index: inst.asset_index.clone(),
        min_memory_mb: 2048,
        max_memory_mb: 4096,
        authlib_injector_jar: None,
        required_java: inst.required_java,
    };
    let cmd = quarry_core::launch::build_command(&cfg);

    std::process::Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(&inst.game_dir)
        .spawn()
        .map_err(|e| format!("Не удалось запустить игру: {}", e))?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_profiles,
            create_profile,
            delete_profile,
            launch_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
