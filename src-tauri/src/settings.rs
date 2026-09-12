use crate::error::{err, StudioResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub llm: LlmAppSettings,
    #[serde(default)]
    pub tts: TtsAppSettings,
    #[serde(default)]
    pub audio: AudioAppSettings,
    #[serde(default)]
    pub workspace: WorkspaceAppSettings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAppSettings {
    #[serde(default)]
    pub last_project_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmAppSettings {
    pub base_url: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TtsAppSettings {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub voice_id: String,
    pub style_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioAppSettings {
    #[serde(default)]
    pub ffmpeg_path: String,
    #[serde(default = "default_episode_format")]
    pub episode_format: String,
}

impl Default for AudioAppSettings {
    fn default() -> Self {
        Self {
            ffmpeg_path: String::new(),
            episode_format: default_episode_format(),
        }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: schema_version(),
            llm: LlmAppSettings::default(),
            tts: TtsAppSettings::default(),
            audio: AudioAppSettings::default(),
            workspace: WorkspaceAppSettings::default(),
        }
    }
}

impl Default for LlmAppSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4.1-mini".to_string(),
        }
    }
}

impl Default for TtsAppSettings {
    fn default() -> Self {
        Self {
            provider: "mimo".to_string(),
            endpoint: "https://api.xiaomimimo.com/v1".to_string(),
            model: "mimo-v2.5-tts-voicedesign".to_string(),
            voice_id: "温柔、清澈、适合长篇有声书旁白的成年女声".to_string(),
            style_prompt: "温柔、清澈、自然，适合长篇有声读物旁白；语速稳定，情绪克制但有画面感。"
                .to_string(),
        }
    }
}

fn schema_version() -> u32 {
    1
}

fn default_episode_format() -> String {
    "m4b".to_string()
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

pub fn load(config_dir: &Path) -> StudioResult<AppSettings> {
    let path = settings_path(config_dir);
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let bytes = fs::read(&path)
        .map_err(|error| err(format!("读取应用设置失败（{}）：{error}", path.display())))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        err(format!(
            "应用设置文件格式无效（{}）：{error}",
            path.display()
        ))
    })
}

pub fn save(config_dir: &Path, value: &AppSettings) -> StudioResult<()> {
    validate(value)?;
    fs::create_dir_all(config_dir)?;
    let path = settings_path(config_dir);
    let temporary = config_dir.join(format!("{SETTINGS_FILE}.tmp"));
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(&temporary, &path)
        .map_err(|error| err(format!("保存应用设置失败（{}）：{error}", path.display())))?;
    Ok(())
}

fn validate(value: &AppSettings) -> StudioResult<()> {
    if value.llm.base_url.trim().is_empty() || value.llm.model.trim().is_empty() {
        return Err(err("LLM 地址和模型不能为空"));
    }
    if value.tts.provider.trim().is_empty() || value.tts.model.trim().is_empty() {
        return Err(err("TTS 供应商和模型不能为空"));
    }
    if !["wav", "mp3", "m4b"].contains(&value.audio.episode_format.as_str()) {
        return Err(err("整集格式只能是 WAV、MP3 或 M4B"));
    }
    Ok(())
}
