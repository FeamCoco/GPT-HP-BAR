//! 设置持久化与应用：config 存 %APPDATA%/GPT-HP-BAR/settings.json；自启写 HKCU Run 注册表

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub skin: String,     // 皮肤 id（skins/*.js 注册名）
    pub font_scale: u32,  // 80-150，100=原始
    pub opacity: u32,     // 40-100
    pub accent: String,   // auto | green | amber | cyan
    pub show_week: bool,
    pub show_credits: bool,
    pub show_countdown: bool,
    pub show_email: bool,
    pub poll_secs: u32,   // 30-600
    pub autostart: bool,
    pub mini_enabled: bool,    // 任务栏挂件
    pub mini_pos: String,      // left | center | right
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            skin: "card".into(),
            font_scale: 100,
            opacity: 96,
            accent: "auto".into(),
            show_week: true,
            show_credits: true,
            show_countdown: true,
            show_email: true,
            poll_secs: 60,
            autostart: false,
            mini_enabled: true,
            mini_pos: "right".into(),
        }
    }
}

pub fn config_path() -> PathBuf {
    let mut p = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    p.push("GPT-HP-BAR");
    let _ = std::fs::create_dir_all(&p);
    p.join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(s: &Settings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), json).map_err(|e| e.to_string())
}

/// 开机自启：HKCU\...\Run（用户级，无需管理员）
pub fn apply_autostart(enable: bool) -> Result<(), String> {
    use winreg::{enums::*, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_SET_VALUE | KEY_QUERY_VALUE)
        .or_else(|_| hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run").map(|(k, _)| k))
        .map_err(|e| e.to_string())?;
    if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        run.set_value("GPT-HP-BAR", &exe.to_string_lossy().to_string())
            .map_err(|e| e.to_string())
    } else {
        match run.delete_value("GPT-HP-BAR") {
            Ok(()) => Ok(()),
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub fn read_autostart() -> bool {
    use winreg::{enums::*, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .and_then(|k| k.get_value::<String, _>("GPT-HP-BAR"))
        .is_ok()
}
