// Stardew Chistositos - Launcher (Tauri backend)
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};

const BASE_URL: &str = "https://stardew.seguaz.online/";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Manifest {
    version: String,
    #[serde(default)]
    smapi: String,
    #[serde(default, rename = "gameVersion")]
    game_version: String,
    file: String,
    sha256: String,
    #[serde(default, rename = "sizeBytes")]
    size_bytes: u64,
    #[serde(default)]
    mods: Vec<String>,
    #[serde(default, rename = "smapiSha")]
    smapi_sha: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Clone)]
struct CheckResult {
    game_dir: Option<String>,
    local_version: String,
    remote_version: String,
    needs_update: bool,
    size_mb: u64,
    smapi: String,
    game_version: String,
    mods: Vec<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct ServerStatus {
    #[serde(default)]
    online: bool,
    #[serde(default)]
    players: u32,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    avatars: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChangeEntry {
    #[serde(default)]
    date: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    items: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
struct Progress {
    phase: String, // "download" | "verify" | "install" | "done" | "error"
    message: String,
    percent: f64, // 0..100, -1 = indeterminate
}

fn version_file(game_dir: &str) -> PathBuf {
    Path::new(game_dir).join(".chistositos_version")
}

fn home_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    } else {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}

/// The SMAPI launcher inside a game folder (`.exe` on Windows, no extension on macOS/Linux).
fn smapi_path(game_dir: &str) -> PathBuf {
    let exe = Path::new(game_dir).join("StardewModdingAPI.exe");
    if exe.is_file() {
        return exe;
    }
    Path::new(game_dir).join("StardewModdingAPI")
}

/// A valid Stardew folder — detected by the base game (so it works even before SMAPI is installed).
fn is_game_dir(p: &Path) -> bool {
    p.join("Stardew Valley.dll").is_file()
        || p.join("StardewModdingAPI.exe").is_file()
        || p.join("StardewModdingAPI").is_file()
}

fn os_key() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// Launcher's own persisted config (game dir + account session).
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct LauncherCfg {
    #[serde(default)]
    game_dir: Option<String>,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    smapi_installed: Option<String>,
    // game startup options the launcher controls (None = don't touch)
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    music: Option<bool>,
    #[serde(default)]
    fullscreen: Option<bool>,
}

fn config_base() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var("APPDATA").ok().map(PathBuf::from);
    }
    let home = home_dir()?;
    if cfg!(target_os = "macos") {
        Some(home.join("Library").join("Application Support"))
    } else {
        Some(
            std::env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| home.join(".config")),
        )
    }
}

fn launcher_config_path() -> Option<PathBuf> {
    Some(config_base()?.join("StardewChistositos").join("launcher.json"))
}

fn read_cfg() -> LauncherCfg {
    launcher_config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_cfg(cfg: &LauncherCfg) {
    if let Some(p) = launcher_config_path() {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(body) = serde_json::to_string_pretty(cfg) {
            let _ = std::fs::write(p, body);
        }
    }
}

fn read_saved_game_dir() -> Option<String> {
    let dir = read_cfg().game_dir?;
    if is_game_dir(Path::new(&dir)) {
        Some(dir)
    } else {
        None
    }
}

fn write_saved_game_dir(dir: &str) {
    let mut cfg = read_cfg();
    cfg.game_dir = Some(dir.to_string());
    write_cfg(&cfg);
}

#[derive(Debug, Serialize, Clone)]
struct Account {
    username: String,
    role: String,
    avatar: Option<String>,
}

/// Pull the avatar from a server `user` JSON object (None if missing/empty).
fn parse_avatar(v: &serde_json::Value) -> Option<String> {
    v["user"]["avatar"]
        .as_str()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

async fn auth_call(kind: &str, username: String, password: String) -> Result<Account, String> {
    let url = format!("{}api/{}", BASE_URL, kind);
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "username": username, "password": password }))
        .send()
        .await
        .map_err(|e| format!("No se pudo conectar al servidor: {e}"))?;
    let ok = resp.status().is_success();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Respuesta inválida: {e}"))?;
    if !ok {
        return Err(v["error"].as_str().unwrap_or("Error desconocido").to_string());
    }
    let token = v["token"].as_str().unwrap_or("").to_string();
    let uname = v["user"]["username"].as_str().unwrap_or("").to_string();
    let role = v["user"]["role"].as_str().unwrap_or("user").to_string();
    let avatar = parse_avatar(&v);
    let mut cfg = read_cfg();
    cfg.token = Some(token);
    cfg.username = Some(uname.clone());
    cfg.role = Some(role.clone());
    write_cfg(&cfg);
    Ok(Account { username: uname, role, avatar })
}

/// Log in with an existing account.
#[tauri::command]
async fn account_login(username: String, password: String) -> Result<Account, String> {
    auth_call("login", username, password).await
}

/// Create a new account (first ever account becomes admin server-side).
#[tauri::command]
async fn account_register(username: String, password: String) -> Result<Account, String> {
    auth_call("register", username, password).await
}

/// Return the current session's account (validates the saved token), or null.
#[tauri::command]
async fn account_me() -> Option<Account> {
    let token = read_cfg().token?;
    let resp = reqwest::Client::new()
        .get(format!("{}api/me", BASE_URL))
        .bearer_auth(token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        account_logout();
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    Some(Account {
        username: v["user"]["username"].as_str()?.to_string(),
        role: v["user"]["role"].as_str().unwrap_or("user").to_string(),
        avatar: parse_avatar(&v),
    })
}

/// Upload a new profile photo (a data: URL, already resized client-side) and return the updated account.
#[tauri::command]
async fn account_set_avatar(data_url: String) -> Result<Account, String> {
    let token = read_cfg().token.ok_or("No has iniciado sesión")?;
    let resp = reqwest::Client::new()
        .put(format!("{}api/me/avatar", BASE_URL))
        .bearer_auth(token)
        .json(&serde_json::json!({ "avatar": data_url }))
        .send()
        .await
        .map_err(|e| format!("No se pudo conectar al servidor: {e}"))?;
    let ok = resp.status().is_success();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Respuesta inválida: {e}"))?;
    if !ok {
        return Err(v["error"].as_str().unwrap_or("Error desconocido").to_string());
    }
    Ok(Account {
        username: v["user"]["username"].as_str().unwrap_or("").to_string(),
        role: v["user"]["role"].as_str().unwrap_or("user").to_string(),
        avatar: parse_avatar(&v),
    })
}

/// Clear the saved session.
#[tauri::command]
fn account_logout() {
    let mut cfg = read_cfg();
    cfg.token = None;
    cfg.username = None;
    cfg.role = None;
    write_cfg(&cfg);
}

/// Open a URL in the default browser (used for the admin panel).
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", &url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };
    cmd.spawn()
        .map_err(|e| format!("No se pudo abrir el navegador: {e}"))?;
    Ok(())
}

// ---- game startup options (edit Stardew's startup_preferences) ----
fn startup_prefs_path() -> Option<PathBuf> {
    if cfg!(windows) {
        let appdata = std::env::var("APPDATA").ok()?;
        return Some(Path::new(&appdata).join("StardewValley").join("startup_preferences"));
    }
    // Stardew stores its config under ~/.config/StardewValley on macOS and Linux
    Some(home_dir()?.join(".config").join("StardewValley").join("startup_preferences"))
}

fn get_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let s = xml.find(&open)? + open.len();
    let e = xml[s..].find(&close)? + s;
    Some(xml[s..e].to_string())
}

fn set_tag(xml: &mut String, tag: &str, value: &str) {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    if let Some(s) = xml.find(&open) {
        let from = s + open.len();
        if let Some(rel) = xml[from..].find(&close) {
            let e = from + rel;
            xml.replace_range(from..e, value);
            return;
        }
    }
    // self-closing <tag /> -> expand
    let sc = format!("<{tag} />");
    if let Some(p) = xml.find(&sc) {
        xml.replace_range(p..p + sc.len(), &format!("{open}{value}{close}"));
    }
}

#[derive(Debug, Serialize, Clone)]
struct GamePrefs {
    lang: String,
    music: bool,
    fullscreen: bool,
}

/// Read the effective game startup options (file + launcher overrides) for the UI.
#[tauri::command]
fn get_prefs() -> GamePrefs {
    let cfg = read_cfg();
    let xml = startup_prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let mut lang = get_tag(&xml, "languageCode").filter(|s| !s.is_empty()).unwrap_or_else(|| "es".into());
    let mut music = get_tag(&xml, "musicVolumeLevel")
        .and_then(|v| v.parse::<f32>().ok())
        .map(|v| v > 0.0)
        .unwrap_or(true);
    let mut fullscreen = get_tag(&xml, "windowMode")
        .and_then(|v| v.parse::<i32>().ok())
        .map(|v| v == 2)
        .unwrap_or(false);
    // launcher overrides win
    if let Some(l) = cfg.lang { lang = l; }
    if let Some(m) = cfg.music { music = m; }
    if let Some(f) = cfg.fullscreen { fullscreen = f; }
    GamePrefs { lang, music, fullscreen }
}

/// Save the launcher-controlled game options (applied at launch).
#[tauri::command]
fn set_prefs(lang: String, music: bool, fullscreen: bool) {
    let mut cfg = read_cfg();
    cfg.lang = Some(lang);
    cfg.music = Some(music);
    cfg.fullscreen = Some(fullscreen);
    write_cfg(&cfg);
}

/// Apply launcher options to Stardew's startup_preferences (called before launch).
fn apply_game_prefs() {
    let cfg = read_cfg();
    if cfg.lang.is_none() && cfg.music.is_none() && cfg.fullscreen.is_none() {
        return; // nothing chosen, leave the game's settings untouched
    }
    let path = match startup_prefs_path() {
        Some(p) if p.is_file() => p,
        _ => return,
    };
    let mut xml = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Some(lang) = &cfg.lang {
        set_tag(&mut xml, "languageCode", lang);
    }
    if let Some(music) = cfg.music {
        set_tag(&mut xml, "musicVolumeLevel", if music { "1" } else { "0" });
    }
    if let Some(fs) = cfg.fullscreen {
        // windowMode: 2 = fullscreen, 1 = windowed
        set_tag(&mut xml, "windowMode", if fs { "2" } else { "1" });
        set_tag(&mut xml, "fullscreen", if fs { "true" } else { "false" });
        set_tag(&mut xml, "windowedBorderlessFullscreen", "false");
    }
    let _ = std::fs::write(&path, xml);
}

/// Auto-detect the Stardew Valley folder, or None.
#[tauri::command]
fn find_game_dir() -> Option<String> {
    // 0) previously chosen folder
    if let Some(dir) = read_saved_game_dir() {
        return Some(dir);
    }
    // 1) next to the launcher exe
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if is_game_dir(dir) {
                return Some(dir.to_string_lossy().to_string());
            }
        }
    }
    // 2) common install locations (per OS)
    let guesses: Vec<PathBuf> = if cfg!(windows) {
        [
            r"C:\Program Files (x86)\Steam\steamapps\common\Stardew Valley",
            r"C:\Program Files\Steam\steamapps\common\Stardew Valley",
            r"D:\Games\Stardew Valley\Stardew Valley",
            r"D:\SteamLibrary\steamapps\common\Stardew Valley",
            r"E:\SteamLibrary\steamapps\common\Stardew Valley",
            r"C:\GOG Games\Stardew Valley",
            r"C:\Program Files\GalaxyClient\Games\Stardew Valley",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    } else if cfg!(target_os = "macos") {
        match home_dir() {
            Some(h) => vec![
                h.join("Library/Application Support/Steam/steamapps/common/Stardew Valley/Contents/MacOS"),
                h.join("Library/Application Support/Steam/steamapps/common/Stardew Valley"),
            ],
            None => vec![],
        }
    } else {
        // Linux: Steam (native, flatpak), GOG
        match home_dir() {
            Some(h) => vec![
                h.join(".steam/steam/steamapps/common/Stardew Valley"),
                h.join(".local/share/Steam/steamapps/common/Stardew Valley"),
                h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/Stardew Valley"),
                h.join("GOG Games/Stardew Valley/game"),
                PathBuf::from("/usr/lib/StardewValley"),
            ],
            None => vec![],
        }
    };
    for p in guesses {
        if is_game_dir(&p) {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

/// Validate a user-supplied folder (trims quotes/whitespace) and remember it.
#[tauri::command]
fn validate_game_dir(path: String) -> Option<String> {
    let cleaned = path.trim().trim_matches('"').to_string();
    let p = Path::new(&cleaned);
    if is_game_dir(p) {
        write_saved_game_dir(&cleaned);
        Some(cleaned)
    } else {
        None
    }
}

/// Live server status from the hosting endpoint (online + player count + connected names),
/// enriched with each connected player's profile photo (resolved via the admin API).
#[tauri::command]
async fn get_server_status() -> ServerStatus {
    let url = format!("{}status.json", BASE_URL);
    let mut st = match reqwest::get(&url).await {
        Ok(resp) if resp.status().is_success() => {
            let text = resp.text().await.unwrap_or_default();
            let clean = text.trim_start_matches('\u{feff}').trim();
            serde_json::from_str::<ServerStatus>(clean).unwrap_or_default()
        }
        _ => ServerStatus::default(),
    };
    // resolve the connected players' profile photos (needs a logged-in token)
    if !st.names.is_empty() {
        if let Some(token) = read_cfg().token {
            if let Ok(resp) = reqwest::Client::new()
                .post(format!("{}api/avatars", BASE_URL))
                .bearer_auth(token)
                .json(&serde_json::json!({ "users": st.names }))
                .send()
                .await
            {
                if resp.status().is_success() {
                    if let Ok(v) = resp.json::<serde_json::Value>().await {
                        if let Some(obj) = v["avatars"].as_object() {
                            for (k, val) in obj {
                                if let Some(s) = val.as_str() {
                                    st.avatars.insert(k.to_lowercase(), s.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    st
}

/// Patch notes / news shown in the launcher.
#[tauri::command]
async fn get_changelog() -> Vec<ChangeEntry> {
    let url = format!("{}changelog.json", BASE_URL);
    match reqwest::get(&url).await {
        Ok(r) if r.status().is_success() => {
            let t = r.text().await.unwrap_or_default();
            let c = t.trim_start_matches('\u{feff}').trim();
            serde_json::from_str::<Vec<ChangeEntry>>(c).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

async fn fetch_manifest() -> Result<Manifest, String> {
    let url = format!("{}manifest.json", BASE_URL);
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("No se pudo conectar al servidor: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("El servidor respondió {}", resp.status()));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| format!("Error leyendo manifest: {e}"))?;
    // Strip a UTF-8/UTF-16 BOM if the server wrote one (PowerShell does).
    let clean = text.trim_start_matches('\u{feff}').trim();
    serde_json::from_str::<Manifest>(clean).map_err(|e| format!("Manifest inválido: {e}"))
}

/// Check for updates given a known game dir (may be empty/None to just probe).
#[tauri::command]
async fn check_updates(game_dir: Option<String>) -> CheckResult {
    let resolved = match &game_dir {
        Some(g) if is_game_dir(Path::new(g)) => Some(g.clone()),
        _ => find_game_dir(),
    };

    let manifest = match fetch_manifest().await {
        Ok(m) => m,
        Err(e) => {
            return CheckResult {
                game_dir: resolved,
                local_version: String::new(),
                remote_version: String::new(),
                needs_update: false,
                size_mb: 0,
                smapi: String::new(),
                game_version: String::new(),
                mods: Vec::new(),
                error: Some(e),
            };
        }
    };

    let local = resolved
        .as_ref()
        .map(|g| {
            std::fs::read_to_string(version_file(g))
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .unwrap_or_default();

    let mods_exist = resolved
        .as_ref()
        .map(|g| Path::new(g).join("Mods").is_dir())
        .unwrap_or(false);

    let needs_update = resolved.is_some() && (local != manifest.version || !mods_exist);

    CheckResult {
        game_dir: resolved,
        local_version: local,
        remote_version: manifest.version.clone(),
        needs_update,
        size_mb: manifest.size_bytes / 1024 / 1024,
        smapi: manifest.smapi.clone(),
        game_version: manifest.game_version.clone(),
        mods: manifest.mods.clone(),
        error: None,
    }
}

fn emit(app: &AppHandle, phase: &str, message: &str, percent: f64) {
    let _ = app.emit(
        "progress",
        Progress {
            phase: phase.to_string(),
            message: message.to_string(),
            percent,
        },
    );
}

/// Download + verify + extract the mod pack, then write the version file.
#[tauri::command]
async fn update_mods(app: AppHandle, game_dir: String) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let manifest = fetch_manifest().await?;
    let mods_dir = Path::new(&game_dir).join("Mods");
    let tmp_zip = std::env::temp_dir().join("chistositos_mods.zip");

    // ---- download (streamed, with progress) ----
    emit(&app, "download", "Conectando…", -1.0);
    let url = format!("{}{}", BASE_URL, manifest.file);
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Error de descarga: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Descarga falló: {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(manifest.size_bytes);
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(&tmp_zip)
        .await
        .map_err(|e| format!("No se pudo crear archivo temporal: {e}"))?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Conexión interrumpida: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Error escribiendo: {e}"))?;
        downloaded += chunk.len() as u64;
        let pct = if total > 0 {
            (downloaded as f64 / total as f64) * 100.0
        } else {
            -1.0
        };
        let msg = format!(
            "Descargando… {} / {} MB",
            downloaded / 1024 / 1024,
            total / 1024 / 1024
        );
        emit(&app, "download", &msg, pct);
    }
    file.flush().await.ok();
    drop(file);

    // ---- verify sha256 ----
    emit(&app, "verify", "Verificando integridad…", -1.0);
    let computed = sha256_file(&tmp_zip).map_err(|e| format!("Error verificando: {e}"))?;
    if !computed.eq_ignore_ascii_case(&manifest.sha256) {
        let _ = std::fs::remove_file(&tmp_zip);
        return Err("El hash no coincide (descarga corrupta). Vuelve a intentarlo.".into());
    }

    // ---- extract over Mods/ (blocking, on a worker thread) ----
    emit(&app, "install", "Instalando mods…", -1.0);
    let app2 = app.clone();
    let zip_path = tmp_zip.clone();
    let mods_dir2 = mods_dir.clone();
    tokio::task::spawn_blocking(move || extract_zip(&app2, &zip_path, &mods_dir2))
        .await
        .map_err(|e| format!("Error interno: {e}"))??;

    let _ = std::fs::remove_file(&tmp_zip);
    std::fs::write(version_file(&game_dir), &manifest.version)
        .map_err(|e| format!("No se pudo guardar la versión: {e}"))?;

    emit(&app, "done", "¡Mods actualizados!", 100.0);
    Ok(())
}

fn extract_zip(app: &AppHandle, zip_path: &Path, mods_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(mods_dir).map_err(|e| e.to_string())?;
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let count = archive.len();
    for i in 0..count {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = match entry.enclosed_name() {
            Some(p) => p,
            None => continue,
        };
        let dest = mods_dir.join(&name);
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).ok();
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        drop(out);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = entry.unix_mode() {
                let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(mode));
            }
        }
        if i % 15 == 0 || i + 1 == count {
            let pct = ((i + 1) as f64 / count as f64) * 100.0;
            emit(app, "install", &format!("Instalando… {}/{}", i + 1, count), pct);
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Ensure SMAPI is installed and matches the required version. Installs/updates it
/// by extracting the OS-specific install.dat into the game folder. Safe no-op if current.
#[tauri::command]
async fn ensure_smapi(app: AppHandle, game_dir: String) -> Result<String, String> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let manifest = fetch_manifest().await?;
    let want = manifest.smapi.clone();
    if want.is_empty() {
        return Ok("skip".into());
    }
    let cfg = read_cfg();
    let present = smapi_path(&game_dir).is_file();
    if present && cfg.smapi_installed.as_deref() == Some(want.as_str()) {
        return Ok("ok".into());
    }

    emit(&app, "smapi", &format!("Instalando SMAPI {}…", want), -1.0);
    let os = os_key();
    let url = format!("{}smapi-{}.dat", BASE_URL, os);
    let tmp = std::env::temp_dir().join("chistositos_smapi.dat");

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("No se pudo descargar SMAPI: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("SMAPI no disponible ({})", resp.status()));
    }
    let mut f = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("No se pudo crear archivo temporal: {e}"))?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Conexión interrumpida: {e}"))?;
        f.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    f.flush().await.ok();
    drop(f);

    if let Some(expected) = manifest.smapi_sha.get(os) {
        let got = sha256_file(&tmp).map_err(|e| e.to_string())?;
        if !got.eq_ignore_ascii_case(expected) {
            let _ = std::fs::remove_file(&tmp);
            return Err("SMAPI: el hash no coincide (descarga corrupta).".into());
        }
    }

    emit(&app, "smapi", "Instalando SMAPI…", -1.0);
    let app2 = app.clone();
    let tmp2 = tmp.clone();
    let gd = PathBuf::from(&game_dir);
    tokio::task::spawn_blocking(move || extract_zip(&app2, &tmp2, &gd))
        .await
        .map_err(|e| format!("Error interno: {e}"))??;
    let _ = std::fs::remove_file(&tmp);

    // make sure the SMAPI launcher is executable on macOS/Linux
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = Path::new(&game_dir).join("StardewModdingAPI");
        if bin.is_file() {
            let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755));
        }
    }

    let mut c = read_cfg();
    c.smapi_installed = Some(want);
    write_cfg(&c);
    emit(&app, "smapi", "SMAPI listo.", -1.0);
    Ok("installed".into())
}

/// Launch SMAPI and return immediately.
#[tauri::command]
fn launch_game(game_dir: String) -> Result<(), String> {
    let smapi = smapi_path(&game_dir);
    if !smapi.is_file() {
        return Err("No encuentro StardewModdingAPI.exe en esa carpeta.".into());
    }
    // tell the AutoConnect mod who is playing so it joins straight into their cabin
    let cfg = read_cfg();
    if let Some(uname) = cfg.username {
        let ac_dir = Path::new(&game_dir).join("Mods").join("AutoConnect");
        let _ = std::fs::create_dir_all(&ac_dir);
        let body = serde_json::json!({
            "ServerIP": "84.235.232.238",
            "FarmhandName": uname,
            "AutoConnect": true
        })
        .to_string();
        let _ = std::fs::write(ac_dir.join("config.json"), body);
    }
    // apply launcher game options (language / music / fullscreen) to startup_preferences
    apply_game_prefs();
    std::process::Command::new(&smapi)
        .current_dir(&game_dir)
        .spawn()
        .map_err(|e| format!("No se pudo lanzar el juego: {e}"))?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            find_game_dir,
            validate_game_dir,
            get_server_status,
            get_changelog,
            account_login,
            account_register,
            account_me,
            account_logout,
            account_set_avatar,
            open_url,
            get_prefs,
            set_prefs,
            ensure_smapi,
            check_updates,
            update_mods,
            launch_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
