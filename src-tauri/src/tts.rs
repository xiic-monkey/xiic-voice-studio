use crate::audio;
use crate::domain::VoiceInfo;
use crate::error::{err, StudioResult};
use crate::storage::{
    insert_job, latest_audio_for_segment, mark_job, next_audio_version, now, start_job,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const PROVIDER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const PROVIDER_MAX_ATTEMPTS: usize = 3;
/// 响应体读取失败时的整体重发次数（含首次）。见 `send_and_read_body`。
const PROVIDER_BODY_MAX_ATTEMPTS: usize = 2;
/// 错误信息里给用户看的供应商名（与 `provider_id()` 的机器名区分开）。
const PROVIDER_LABEL_MIMO: &str = "Mimo";
const PROVIDER_LABEL_ALIYUN: &str = "阿里百炼";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettings {
    pub provider: String,
    pub api_key: Option<String>,
    pub endpoint: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsRequest {
    pub segment_id: String,
    pub text: String,
    pub tts_provider: String,
    pub model: Option<String>,
    pub voice_id: String,
    pub voice_sample: Option<VoiceSample>,
    pub speed: f64,
    pub pitch: f64,
    pub style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSample {
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsResult {
    pub audio_bytes: Vec<u8>,
    pub extension: String,
    pub provider: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsSynthesisOptions {
    pub force_regenerate: bool,
}

pub trait TtsProvider {
    fn provider_id(&self) -> &'static str;
    fn list_voices(&self) -> StudioResult<Vec<VoiceInfo>>;
    fn synthesize_blocking(&self, request: &TtsRequest) -> StudioResult<TtsResult>;
    #[allow(dead_code)]
    fn clone_voice(&self) -> StudioResult<()>;
    #[allow(dead_code)]
    fn transcribe(&self) -> StudioResult<()>;
}

pub struct MockTtsProvider;

impl TtsProvider for MockTtsProvider {
    fn provider_id(&self) -> &'static str {
        "mock"
    }

    fn list_voices(&self) -> StudioResult<Vec<VoiceInfo>> {
        Ok(vec![
            VoiceInfo {
                provider: "mock".to_string(),
                voice_id: "mock-female-narrator".to_string(),
                name: "模拟旁白女声".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["narration".to_string()],
            },
            VoiceInfo {
                provider: "mock".to_string(),
                voice_id: "mock-male-character".to_string(),
                name: "模拟角色男声".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["dialogue".to_string()],
            },
        ])
    }

    fn synthesize_blocking(&self, request: &TtsRequest) -> StudioResult<TtsResult> {
        let wav = silent_wav(800 + request.text.chars().count() as u32 * 35);
        Ok(TtsResult {
            audio_bytes: wav,
            extension: "wav".to_string(),
            provider: "mock".to_string(),
        })
    }

    fn clone_voice(&self) -> StudioResult<()> {
        Ok(())
    }

    fn transcribe(&self) -> StudioResult<()> {
        Ok(())
    }
}

pub struct AliyunBailianProvider {
    pub settings: ProviderSettings,
}

impl TtsProvider for AliyunBailianProvider {
    fn provider_id(&self) -> &'static str {
        "aliyun-bailian"
    }

    fn list_voices(&self) -> StudioResult<Vec<VoiceInfo>> {
        Ok(vec![
            VoiceInfo {
                provider: "aliyun-bailian".to_string(),
                voice_id: "longxiaochun".to_string(),
                name: "龙小淳".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["cosyvoice".to_string(), "mandarin".to_string()],
            },
            VoiceInfo {
                provider: "aliyun-bailian".to_string(),
                voice_id: "longwan".to_string(),
                name: "龙婉".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["cosyvoice".to_string(), "female".to_string()],
            },
        ])
    }

    fn synthesize_blocking(&self, request: &TtsRequest) -> StudioResult<TtsResult> {
        if self
            .settings
            .api_key
            .as_ref()
            .map(|key| key.trim().is_empty())
            .unwrap_or(true)
        {
            return MockTtsProvider.synthesize_blocking(request);
        }
        let endpoint = self.settings.endpoint.clone().unwrap_or_else(|| {
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
                .to_string()
        });
        let api_key = self.settings.api_key.clone().unwrap_or_default();
        let model = self
            .settings
            .model
            .clone()
            .unwrap_or_else(|| "cosyvoice-v2".to_string());
        let request_body = json!({
            "model": model,
            "input": {
                "text": request.text,
                "voice": request.voice_id
            },
            "parameters": {
                "speed": request.speed,
                "pitch": request.pitch,
                "format": "wav"
            }
        });
        let client = provider_http_client()?;
        let (_, response_text) = send_and_read_body(PROVIDER_LABEL_ALIYUN, &endpoint, &model, || {
            client
                .post(&endpoint)
                .bearer_auth(&api_key)
                .json(&request_body)
                .send()
        })?;
        let payload: serde_json::Value =
            serde_json::from_str(&response_text).unwrap_or_else(|_| json!({}));
        if let Some(audio_base64) = payload.pointer("/output/audio").and_then(|v| v.as_str()) {
            let bytes = STANDARD
                .decode(audio_base64)
                .map_err(|error| err(format!("解码阿里百炼音频内容失败：{error}")))?;
            return Ok(TtsResult {
                audio_bytes: bytes,
                extension: "wav".to_string(),
                provider: "aliyun-bailian".to_string(),
            });
        }
        Err(err(format!(
            "阿里百炼响应中没有内联音频。原始响应已保留用于调试：{response_text}"
        )))
    }

    fn clone_voice(&self) -> StudioResult<()> {
        Err(err("声音克隆将在后续供应商专用流程中实现"))
    }

    fn transcribe(&self) -> StudioResult<()> {
        Err(err("当前 MVP 尚未实现阿里百炼 ASR"))
    }
}

pub struct MimoTtsProvider {
    pub settings: ProviderSettings,
}

impl TtsProvider for MimoTtsProvider {
    fn provider_id(&self) -> &'static str {
        "mimo"
    }

    fn list_voices(&self) -> StudioResult<Vec<VoiceInfo>> {
        Ok(vec![
            VoiceInfo {
                provider: "mimo".to_string(),
                voice_id: "mimo_default".to_string(),
                name: "Mimo 默认声音".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["preset".to_string(), "mandarin".to_string()],
            },
            VoiceInfo {
                provider: "mimo".to_string(),
                voice_id: "Mia".to_string(),
                name: "Mia".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["preset".to_string(), "female".to_string()],
            },
            VoiceInfo {
                provider: "mimo".to_string(),
                voice_id: "Chloe".to_string(),
                name: "Chloe".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["preset".to_string(), "female".to_string()],
            },
            VoiceInfo {
                provider: "mimo".to_string(),
                voice_id: "Milo".to_string(),
                name: "Milo".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["preset".to_string(), "male".to_string()],
            },
            VoiceInfo {
                provider: "mimo".to_string(),
                voice_id: "Dean".to_string(),
                name: "Dean".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["preset".to_string(), "male".to_string()],
            },
            VoiceInfo {
                provider: "mimo".to_string(),
                voice_id: "温柔、清澈、适合长篇有声书旁白的成年女声".to_string(),
                name: "VoiceDesign：温柔旁白".to_string(),
                language: "zh-CN".to_string(),
                tags: vec!["voice-design".to_string(), "narration".to_string()],
            },
        ])
    }

    fn synthesize_blocking(&self, request: &TtsRequest) -> StudioResult<TtsResult> {
        let api_key = self
            .settings
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| err("请先在设置中保存 Mimo API Key"))?;
        let model =
            normalize_mimo_model(request.model.as_deref().or(self.settings.model.as_deref()));
        let endpoint = mimo_chat_completions_endpoint(self.settings.endpoint.as_deref());
        // 闸门：带克隆样本却走非 voiceclone 模型时，`mimo_request_body` 的 voicedesign 分支
        // 根本不会读 `voice_sample` —— 参考音频被**静默丢弃**，音色会换成"按描述重新设计"的
        // 另一个声音，而且不报错。这里宁可失败也不出声。
        ensure_model_matches_voice_sample(request, &model)?;
        let voice_prompt = build_voice_prompt(request, &model);
        let request_body = mimo_request_body(request, &model, &voice_prompt)?;
        let client = provider_http_client()?;
        let (status, response_text) =
            send_and_read_body(PROVIDER_LABEL_MIMO, &endpoint, &model, || {
                client
                    .post(&endpoint)
                    .header("api-key", api_key)
                    .json(&request_body)
                    .send()
            })?;
        if !status.is_success() {
            return Err(err(format!(
                "Mimo TTS 请求失败（HTTP {status}）：{response_text}"
            )));
        }
        let payload: Value = serde_json::from_str(&response_text)
            .map_err(|error| err(format!("解析 Mimo TTS 响应失败：{error}")))?;
        let audio_base64 = extract_mimo_audio_base64(&payload).ok_or_else(|| {
            err(format!(
                "Mimo TTS 响应中没有音频数据，请检查模型、音色和文本。原始响应：{response_text}"
            ))
        })?;
        let bytes = STANDARD
            .decode(audio_base64)
            .map_err(|error| err(format!("解码 Mimo 音频内容失败：{error}")))?;
        Ok(TtsResult {
            audio_bytes: bytes,
            extension: "wav".to_string(),
            provider: "mimo".to_string(),
        })
    }

    fn clone_voice(&self) -> StudioResult<()> {
        Ok(())
    }

    fn transcribe(&self) -> StudioResult<()> {
        Err(err("当前版本尚未实现 Mimo ASR"))
    }
}

pub fn provider_from_settings(
    settings: Option<ProviderSettings>,
) -> Box<dyn TtsProvider + Send + Sync> {
    match settings {
        Some(settings) if settings.provider == "aliyun-bailian" => {
            Box::new(AliyunBailianProvider { settings })
        }
        Some(settings) if settings.provider == "mimo" => Box::new(MimoTtsProvider { settings }),
        _ => Box::new(MockTtsProvider),
    }
}

#[cfg(test)]
pub fn synthesize_test_audio(
    project_root: &Path,
    project_id: &str,
    settings: Option<ProviderSettings>,
    voice_id: String,
    style: Option<String>,
) -> StudioResult<std::path::PathBuf> {
    let provider = provider_from_settings(settings);
    let result = provider.synthesize_blocking(&TtsRequest {
        segment_id: "tts-test".to_string(),
        text: "这是 Xiic Voice Studio 的声音测试。".to_string(),
        tts_provider: provider.provider_id().to_string(),
        model: None,
        voice_id,
        voice_sample: None,
        speed: 1.0,
        pitch: 0.0,
        style,
    })?;
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let file_name = format!(
        "tts-test-{timestamp}-{}.{}",
        Uuid::new_v4(),
        result.extension
    );
    let (_, output_path) = audio::new_audio_asset(project_root, project_id, &file_name)?;
    fs::write(&output_path, result.audio_bytes)?;
    Ok(output_path)
}

pub fn synthesize_test_audio_to_dir(
    output_dir: &Path,
    settings: Option<ProviderSettings>,
    voice_id: String,
    style: Option<String>,
) -> StudioResult<std::path::PathBuf> {
    let provider = provider_from_settings(settings);
    let result = provider.synthesize_blocking(&TtsRequest {
        segment_id: "tts-test".to_string(),
        text: "这是 Xiic Voice Studio 的声音测试。".to_string(),
        tts_provider: provider.provider_id().to_string(),
        model: None,
        voice_id,
        voice_sample: None,
        speed: 1.0,
        pitch: 0.0,
        style,
    })?;
    fs::create_dir_all(output_dir)?;
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let output_path = output_dir.join(format!(
        "tts-test-{timestamp}-{}.{}",
        Uuid::new_v4(),
        result.extension
    ));
    fs::write(&output_path, result.audio_bytes)?;
    Ok(output_path)
}

pub fn synthesize_voice_profile_test_audio(
    conn: &Connection,
    project_root: &Path,
    output_dir: &Path,
    profile_id: &str,
    settings: Option<ProviderSettings>,
) -> StudioResult<std::path::PathBuf> {
    let profile = conn.query_row(
        "SELECT v.tts_provider, v.model, v.voice_id, v.speed, v.pitch, v.style,
                a.relative_path, a.mime_type
         FROM voice_profiles v
         LEFT JOIN voice_assets a ON v.voice_asset_id = a.id
         WHERE v.id = ?1",
        params![profile_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        },
    )?;
    let provider = provider_from_settings(settings);
    if provider.provider_id() != "mock" && profile.0 != provider.provider_id() {
        return Err(err(format!(
            "声音档案使用 {}，当前生成服务是 {}",
            profile.0,
            provider.provider_id()
        )));
    }
    let voice_sample = match (profile.6, profile.7) {
        (Some(relative_path), Some(mime_type)) => {
            Some(load_voice_sample(project_root, &relative_path, &mime_type)?)
        }
        _ => None,
    };
    let result = provider.synthesize_blocking(&TtsRequest {
        segment_id: "voice-profile-test".to_string(),
        text: "这是 Xiic Voice Studio 的项目音色试听。".to_string(),
        tts_provider: profile.0,
        model: profile.1,
        voice_id: profile.2,
        voice_sample,
        speed: profile.3,
        pitch: profile.4,
        style: profile.5,
    })?;
    fs::create_dir_all(output_dir)?;
    let output_path = output_dir.join(format!(
        "voice-profile-{}-{}.{}",
        profile_id,
        Uuid::new_v4(),
        result.extension
    ));
    fs::write(&output_path, result.audio_bytes)?;
    Ok(output_path)
}

pub fn list_voices(settings: Option<ProviderSettings>) -> StudioResult<Vec<VoiceInfo>> {
    provider_from_settings(settings).list_voices()
}

/// 默认音色档案的兜底：确保旁白档案存在，并为每个还没有音色的角色补一条。
///
/// 「每个角色有自己的固定音色」应当是结构上成立的，而不是留给用户填空：
/// 角色一被标注出来就带上一条可用的默认音色（用户随后换成预置 / 描述设计 / 参考克隆），
/// 生成前的音色校验因此不会在正常流程里被触发——它只在真正的破损状态下兜底。
pub fn ensure_default_voice_profiles(conn: &Connection, project_id: &str) -> StudioResult<()> {
    let narrator_profile_id = ensure_default_narrator_profile(conn, project_id)?;
    bind_voice_profile_to_matching_segments(conn, &narrator_profile_id, None, false)?;
    let characters: Vec<(String, String, Option<String>)> = conn
        .prepare("SELECT id, canonical_name, gender FROM characters WHERE project_id = ?1 ORDER BY canonical_name")?
        .query_map(params![project_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (character_id, name, gender) in characters {
        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM voice_profiles WHERE character_id = ?1",
            params![character_id],
            |row| row.get(0),
        )?;
        if existing > 0 {
            continue;
        }
        let voice_id = match gender.as_deref() {
            Some("male") | Some("男") | Some("男性") => "沉稳自然的中年男声，适合长篇有声书",
            Some("female") | Some("女") | Some("女性") => "温和清晰的成年女声，适合长篇有声书",
            _ => "自然清晰、中性的成年声音，适合长篇有声书",
        };
        conn.execute(
            "INSERT INTO voice_profiles
             (id, project_id, character_id, name, age_stage, tts_provider, model, voice_id, speed, pitch, style, is_default)
             VALUES (?1, ?2, ?3, ?4, 'adult', 'mimo', 'mimo-v2.5-tts-voicedesign', ?5, 1.0, 0.0, NULL, 1)",
            params![
                Uuid::new_v4().to_string(),
                project_id,
                character_id,
                format!("{name} 声音"),
                voice_id
            ],
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn create_mimo_voice_clone_profile(
    conn: &mut Connection,
    project_root: &Path,
    project_id: &str,
    character_id: Option<&str>,
    name: &str,
    age_stage: &str,
    source_path: &Path,
    style: Option<&str>,
    consent_confirmed: bool,
) -> StudioResult<String> {
    if !consent_confirmed {
        return Err(err("创建模仿音色前必须确认已获得声音使用授权"));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(err("音色名称不能为空"));
    }
    if !source_path.is_file() {
        return Err(err("参考音频文件不存在"));
    }
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| err("参考音频必须是 MP3 或 WAV 文件"))?;
    let mime_type = match extension.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        _ => return Err(err("Mimo 音色模仿仅支持 MP3 或 WAV 参考音频")),
    };
    let source_size = fs::metadata(source_path)?.len() as usize;
    let encoded_size = source_size.div_ceil(3) * 4 + mime_type.len() + 13;
    if encoded_size > 10 * 1024 * 1024 {
        return Err(err("参考音频 Base64 编码后超过 Mimo 10 MB 限制"));
    }

    let asset_id = Uuid::new_v4().to_string();
    let profile_id = Uuid::new_v4().to_string();
    let relative_path = format!("assets/source/voices/{asset_id}.{extension}");
    let destination = project_root.join(&relative_path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source_path, &destination)?;

    let source_file_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("voice-sample")
        .to_string();
    let affected_segments = segment_ids_matching_voice_scope(conn, character_id, true)?;
    let timestamp = now();
    let transaction_result = (|| -> StudioResult<()> {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO voice_assets
             (id, project_id, name, asset_type, provider, model, relative_path, mime_type,
              source_file_name, consent_confirmed, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'clone_sample', 'mimo', 'mimo-v2.5-tts-voiceclone', ?4, ?5, ?6, 1, 'ready', ?7, ?8)",
            params![
                asset_id,
                project_id,
                name,
                relative_path,
                mime_type,
                source_file_name,
                timestamp,
                timestamp
            ],
        )?;
        tx.execute(
            "INSERT INTO voice_profiles
             (id, project_id, character_id, name, age_stage, tts_provider, model, voice_id,
              voice_asset_id, speed, pitch, style)
             VALUES (?1, ?2, ?3, ?4, ?5, 'mimo', 'mimo-v2.5-tts-voiceclone', ?6, ?7, 1.0, 0.0, ?8)",
            params![
                profile_id,
                project_id,
                character_id,
                name,
                age_stage,
                format!("clone:{asset_id}"),
                asset_id,
                style
            ],
        )?;
        bind_voice_profile_to_matching_segments(&tx, &profile_id, character_id, true)?;
        tx.commit()?;
        Ok(())
    })();
    if let Err(error) = transaction_result {
        let _ = fs::remove_file(&destination);
        return Err(error);
    }
    for segment_id in affected_segments {
        audio::invalidate_segment_audio(
            conn,
            &segment_id,
            "角色已切换为 Mimo 模仿音色，请重新生成并审听音频",
        )?;
    }
    Ok(profile_id)
}

pub fn bind_voice_profile_to_matching_segments(
    conn: &Connection,
    profile_id: &str,
    character_id: Option<&str>,
    replace_existing: bool,
) -> StudioResult<usize> {
    let replace_existing = if replace_existing { 1 } else { 0 };
    let updated = if let Some(character_id) = character_id {
        conn.execute(
            "UPDATE segments
             SET voice_profile_id = ?1, updated_at = ?2
             WHERE character_id = ?3 AND (voice_profile_id IS NULL OR ?4 = 1)",
            params![profile_id, now(), character_id, replace_existing],
        )?
    } else {
        conn.execute(
            "UPDATE segments
             SET voice_profile_id = ?1, updated_at = ?2
             WHERE character_id IS NULL
               AND segment_type IN ('narration', 'inner_monologue', 'unknown')
               AND (voice_profile_id IS NULL OR ?3 = 1)",
            params![profile_id, now(), replace_existing],
        )?
    };
    Ok(updated)
}

pub fn segment_ids_matching_voice_scope(
    conn: &Connection,
    character_id: Option<&str>,
    only_with_usable_audio: bool,
) -> StudioResult<Vec<String>> {
    let audio_filter = if only_with_usable_audio {
        "AND EXISTS (
            SELECT 1 FROM segment_audio a
            WHERE a.segment_id = s.id
              AND a.status IN ('approved', 'generated', 'uploaded')
              AND a.source != 'manual_upload'
        )"
    } else {
        ""
    };
    let sql = if character_id.is_some() {
        format!("SELECT s.id FROM segments s WHERE s.character_id = ?1 {audio_filter} ORDER BY s.order_index")
    } else {
        format!(
            "SELECT s.id FROM segments s
             WHERE s.character_id IS NULL
               AND s.segment_type IN ('narration', 'inner_monologue', 'unknown')
               {audio_filter}
             ORDER BY s.order_index"
        )
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(character_id) = character_id {
        stmt.query_map(params![character_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(rows)
}

/// product-brief 第 6 节的音色时间轴阶段，按年龄从早到晚排列。
/// 顺序即语义，`segments.age_progress`（0.0~1.0）按此顺序映射。
pub const AGE_STAGES: [&str; 6] = [
    "childhood",
    "teenager",
    "young_adult",
    "adult",
    "middle_aged",
    "elderly",
];

/// 把分段的 `age_progress`（0.0 ~ 1.0）映射为音色时间轴上的阶段。
pub fn stage_for_age_progress(progress: f64) -> &'static str {
    let clamped = progress.clamp(0.0, 1.0);
    let index = (clamped * (AGE_STAGES.len() - 1) as f64).round() as usize;
    AGE_STAGES[index.min(AGE_STAGES.len() - 1)]
}

/// 角色在指定年龄阶段下应使用的音色档案。
/// 优先级：精确命中该阶段 → 默认档 `adult` → 任意一条（mimo 优先）。
/// 音色的唯一真源是角色，因此这里只按 `character_id` 查，不看分段副本。
pub fn voice_profile_for_character_at_stage(
    conn: &Connection,
    character_id: &str,
    age_progress: Option<f64>,
) -> StudioResult<Option<String>> {
    let wanted = age_progress.map(stage_for_age_progress);
    let profile_id = conn
        .query_row(
            "SELECT id FROM voice_profiles
             WHERE character_id = ?1
             ORDER BY
               CASE
                 WHEN ?2 IS NOT NULL AND age_stage = ?2 THEN 0
                 WHEN age_stage = 'adult' THEN 1
                 ELSE 2
               END,
               is_default ASC,
               CASE WHEN tts_provider = 'mimo' THEN 0 ELSE 1 END,
               name
             LIMIT 1",
            params![character_id, wanted],
            |row| row.get(0),
        )
        .optional()?;
    Ok(profile_id)
}

/// 角色身上**由人定过**的音色，排除系统自动兜底的默认档。
/// 合并角色时用它回答「目标角色是不是已经有真正的音色了」——
/// 光看"有没有档案"会被自动默认档骗过去。
pub fn user_voice_profile_for_character(
    conn: &Connection,
    character_id: &str,
) -> StudioResult<Option<String>> {
    let profile_id = conn
        .query_row(
            "SELECT id FROM voice_profiles
             WHERE character_id = ?1 AND is_default = 0
             ORDER BY
               CASE WHEN age_stage = 'adult' THEN 0 ELSE 1 END,
               CASE WHEN tts_provider = 'mimo' THEN 0 ELSE 1 END,
               updated_at DESC,
               name
             LIMIT 1",
            params![character_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(profile_id)
}

/// 兼容旧调用点：不指定年龄阶段时的角色音色。
pub fn preferred_voice_profile_for_character(
    conn: &Connection,
    character_id: &str,
) -> StudioResult<Option<String>> {
    voice_profile_for_character_at_stage(conn, character_id, None)
}

/// 项目内的旁白音色档案（`character_id` 为空的那条）。
/// 只读，不会创建；`set_narrator_voice` 与合成解析共用同一个口径，避免"改了旁白音色界面不变"。
pub fn narrator_profile_for_project(
    conn: &Connection,
    project_id: &str,
) -> StudioResult<Option<String>> {
    let profile_id = conn
        .query_row(
            "SELECT id FROM voice_profiles
             WHERE project_id = ?1 AND character_id IS NULL
             ORDER BY
               CASE
                 WHEN tts_provider = 'mimo' THEN 0
                 WHEN name LIKE '%旁白%' THEN 1
                 ELSE 2
               END,
               is_default ASC,
               name
             LIMIT 1",
            params![project_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(profile_id)
}

fn voice_profile_exists(conn: &Connection, profile_id: &str) -> StudioResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM voice_profiles WHERE id = ?1",
        params![profile_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// 解析一个分段最终使用的音色档案，是合成链路上音色归属的唯一权威。
///
/// 1. 分段已定稿（审听通过 / 人工上传）且冻结档案仍在 → 用冻结的那条，
///    这样单段返修继承的是**生成时**的音色阶段，不会跟相邻段落脱节（product-brief 第 6 节）。
/// 2. 其余情况 → 按 `character_id` + `age_progress` 解析角色的当前音色。
/// 3. 旁白（`character_id` 为空）→ 项目内的旁白档案。
/// 4. 都解析不到 → 返回 `None`，由调用方 fail-closed 报错，不再静默兜底成 mock。
pub fn resolve_voice_profile_for_segment(
    conn: &Connection,
    project_id: &str,
    frozen_profile_id: Option<&str>,
    character_id: Option<&str>,
    age_progress: Option<f64>,
    finalized: bool,
) -> StudioResult<Option<String>> {
    if finalized {
        if let Some(profile_id) = frozen_profile_id {
            if voice_profile_exists(conn, profile_id)? {
                return Ok(Some(profile_id.to_string()));
            }
        }
    }
    match character_id {
        Some(character_id) => voice_profile_for_character_at_stage(conn, character_id, age_progress),
        None => narrator_profile_for_project(conn, project_id),
    }
}

pub(crate) fn ensure_default_narrator_profile(conn: &Connection, project_id: &str) -> StudioResult<String> {
    if let Some(profile_id) = narrator_profile_for_project(conn, project_id)? {
        return Ok(profile_id);
    }
    let profile_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO voice_profiles
         (id, project_id, character_id, name, age_stage, tts_provider, model, voice_id, speed, pitch, style, is_default)
         VALUES (?1, ?2, NULL, 'Mimo 旁白声音', 'adult', 'mimo', 'mimo-v2.5-tts-voicedesign', ?3, 1.0, 0.0, ?4, 1)",
        params![
            profile_id,
            project_id,
            "温柔、清澈、适合长篇有声书旁白的成年女声",
            "温柔、清澈、自然，适合长篇有声读物旁白；语速稳定，情绪克制但有画面感。"
        ],
    )?;
    Ok(profile_id)
}

#[allow(dead_code)]
pub fn synthesize_segments(
    conn: &Connection,
    project_root: &Path,
    project_id: &str,
    segment_ids: Vec<String>,
    settings: Option<ProviderSettings>,
) -> StudioResult<()> {
    synthesize_segments_with_options(
        conn,
        project_root,
        project_id,
        segment_ids,
        settings,
        TtsSynthesisOptions::default(),
    )
}

pub fn synthesize_segments_with_options(
    conn: &Connection,
    project_root: &Path,
    project_id: &str,
    segment_ids: Vec<String>,
    settings: Option<ProviderSettings>,
    options: TtsSynthesisOptions,
) -> StudioResult<()> {
    let job_id = create_tts_job(conn, project_id, &segment_ids, settings.as_ref(), &options)?;
    synthesize_segments_for_job(
        conn,
        project_root,
        project_id,
        &job_id,
        segment_ids,
        settings,
        options,
    )
}

pub fn create_tts_job(
    conn: &Connection,
    project_id: &str,
    segment_ids: &[String],
    settings: Option<&ProviderSettings>,
    options: &TtsSynthesisOptions,
) -> StudioResult<String> {
    let payload = json!({
        "segmentIds": segment_ids,
        "provider": settings.as_ref().map(|s| s.provider.as_str()).unwrap_or("mock"),
        "endpoint": settings.as_ref().and_then(|s| s.endpoint.as_deref()),
        "model": settings.as_ref().and_then(|s| s.model.as_deref()),
        "forceRegenerate": options.force_regenerate,
    })
    .to_string();
    Ok(insert_job(conn, project_id, "tts_batch", &payload)?.id)
}

pub fn synthesize_segments_for_job(
    conn: &Connection,
    project_root: &Path,
    project_id: &str,
    job_id: &str,
    segment_ids: Vec<String>,
    settings: Option<ProviderSettings>,
    options: TtsSynthesisOptions,
) -> StudioResult<()> {
    if is_job_canceled(conn, job_id)? {
        return Ok(());
    }
    if !start_job(conn, job_id)? {
        return Ok(());
    }
    let provider = provider_from_settings(settings);
    let selected_segments = load_tts_requests(conn, project_root, project_id, segment_ids)?;
    let protected_skipped = count_protected_segments(conn, &selected_segments, &options)?;
    let selected_segments = filter_tts_requests(conn, selected_segments, &options)?;
    let total = selected_segments.len().max(1);
    let result = synthesize_selected_segments(
        conn,
        project_root,
        project_id,
        provider.as_ref(),
        job_id,
        selected_segments,
        total,
    );
    if is_job_canceled(conn, job_id)? {
        return Ok(());
    }
    if let Err(error) = &result {
        mark_job(conn, job_id, "failed", 0.0, Some(&error.to_string()))?;
        return Err(err(format!("语音生成失败：{error}")));
    }
    let message = if protected_skipped > 0 {
        Some(format!("已跳过 {protected_skipped} 个受保护分段"))
    } else {
        None
    };
    mark_job(conn, job_id, "succeeded", 1.0, message.as_deref())?;
    Ok(())
}

fn provider_http_client() -> StudioResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(PROVIDER_CONNECT_TIMEOUT)
        .timeout(PROVIDER_REQUEST_TIMEOUT)
        .build()
        .map_err(Into::into)
}

fn send_with_retries<F>(mut send: F) -> StudioResult<reqwest::blocking::Response>
where
    F: FnMut() -> Result<reqwest::blocking::Response, reqwest::Error>,
{
    let mut last_error = None;
    for attempt in 0..PROVIDER_MAX_ATTEMPTS {
        match send() {
            Ok(response)
                if retryable_status(response.status()) && attempt + 1 < PROVIDER_MAX_ATTEMPTS =>
            {
                thread::sleep(retry_delay(attempt));
            }
            Ok(response) => return Ok(response),
            Err(error) if attempt + 1 < PROVIDER_MAX_ATTEMPTS => {
                last_error = Some(error);
                thread::sleep(retry_delay(attempt));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(last_error
        .map(Into::into)
        .unwrap_or_else(|| err("网络请求失败，重试后仍未收到响应")))
}

fn retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

/// 读取响应体失败的两种情形。reqwest 把**两者**都压成同一个
/// `Error{kind: Decode}`，Display 只有一句 "error decoding response body"，
/// 既看不出是网络问题还是 JSON 问题，也无法据以行动。这里靠 `is_timeout()`
/// 把它重新分开，并翻成能直接定位问题的中文。
enum ProviderBodyFailure {
    /// 请求已被接受，但响应体没在时限内读完。
    TimedOut(String),
    /// 连接/HTTP 流在传输途中被提前关闭，响应体不完整。
    Truncated(String),
}

fn read_provider_body(
    response: reqwest::blocking::Response,
    provider: &str,
    endpoint: &str,
    model: &str,
    elapsed: Duration,
) -> Result<(reqwest::StatusCode, String), ProviderBodyFailure> {
    let status = response.status();
    match response.text() {
        Ok(text) => Ok((status, text)),
        Err(error) => {
            let seconds = elapsed.as_secs_f64();
            if error.is_timeout() {
                Err(ProviderBodyFailure::TimedOut(format!(
                    "{provider} 读取响应超时：请求已发出（HTTP {status}），但 {seconds:.1} 秒内没读完响应体。\
                     模型 {model}，地址 {endpoint}。（原始错误：{error}）"
                )))
            } else {
                Err(ProviderBodyFailure::Truncated(format!(
                    "{provider} 响应在传输途中被中断：HTTP {status}，{seconds:.1} 秒后连接被提前关闭，响应体不完整。\
                     模型 {model}，地址 {endpoint}。多数情况下重试即可恢复。（原始错误：{error}）"
                )))
            }
        }
    }
}

/// 发送请求并读取响应体。
///
/// `send_with_retries` 只兜住了 **send 阶段**的错误；而"响应头已返回 200、响应体却在
/// 传输途中被掐断"这类故障会穿过去，直接以 `error decoding response body` 抛到界面上
/// —— 2026-09-25「生成声音试听」报的正是这一类。所以这里把整次请求（send + 读体）
/// 一起重发一次；已超时的情形不重发，因为那说明整个时间预算已经耗尽。
fn send_and_read_body<F>(
    provider: &str,
    endpoint: &str,
    model: &str,
    mut send: F,
) -> StudioResult<(reqwest::StatusCode, String)>
where
    F: FnMut() -> Result<reqwest::blocking::Response, reqwest::Error>,
{
    let mut last_truncation = None;
    for attempt in 0..PROVIDER_BODY_MAX_ATTEMPTS {
        let started = Instant::now();
        let response = send_with_retries(&mut send)?;
        match read_provider_body(response, provider, endpoint, model, started.elapsed()) {
            Ok(outcome) => return Ok(outcome),
            Err(ProviderBodyFailure::TimedOut(message)) => return Err(err(message)),
            Err(ProviderBodyFailure::Truncated(message)) => {
                last_truncation = Some(message);
                if attempt + 1 < PROVIDER_BODY_MAX_ATTEMPTS {
                    thread::sleep(retry_delay(attempt));
                }
            }
        }
    }
    Err(err(last_truncation
        .unwrap_or_else(|| format!("{provider} 响应体读取失败"))))
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(500 * 2u64.pow(attempt.min(4) as u32))
}

fn synthesize_selected_segments(
    conn: &Connection,
    project_root: &Path,
    project_id: &str,
    provider: &(dyn TtsProvider + Send + Sync),
    job_id: &str,
    selected_segments: Vec<TtsRequest>,
    total: usize,
) -> StudioResult<()> {
    for (index, request) in selected_segments.iter().enumerate() {
        if is_job_canceled(conn, job_id)? {
            return Ok(());
        }
        if provider.provider_id() != "mock" && request.tts_provider != provider.provider_id() {
            return Err(err(format!(
                "分段声音档案使用 {}，当前生成服务是 {}。请切换服务或重新分配声音",
                request.tts_provider,
                provider.provider_id()
            )));
        }
        let result = provider.synthesize_blocking(request)?;
        if is_job_canceled(conn, job_id)? {
            return Ok(());
        }
        let version = next_audio_version(conn, &request.segment_id)?;
        let file_name = format!("{}_v{}.{}", request.segment_id, version, result.extension);
        let (relative_path, output_path) =
            audio::new_audio_asset(project_root, project_id, &file_name)?;
        fs::write(&output_path, result.audio_bytes)?;
        let duration_ms = audio::detect_audio_duration_ms(&output_path, None).ok();
        conn.execute(
            "INSERT INTO segment_audio (id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, 'generated', ?7)",
            params![
                Uuid::new_v4().to_string(),
                request.segment_id,
                relative_path,
                duration_ms,
                version,
                result.provider,
                now()
            ],
        )?;
        conn.execute(
            "UPDATE segments SET audio_status = 'generated', review_status = 'unreviewed', updated_at = ?1 WHERE id = ?2",
            params![now(), request.segment_id],
        )?;
        // 重新生成后，「脚本已变更 / 音色已固化 / 声音配置已变更」这些自动提示的原因就消除了。
        // 不关的话它们会永远挂在界面上（导出闸门不看这张表，但用户会反复看到"请重新生成"）。
        audio::resolve_auto_invalidation_issues(conn, &request.segment_id)?;
        mark_job(
            conn,
            job_id,
            "running",
            (index + 1) as f64 / total as f64,
            None,
        )?;
    }
    Ok(())
}

fn is_job_canceled(conn: &Connection, job_id: &str) -> StudioResult<bool> {
    let status: String = conn.query_row(
        "SELECT status FROM jobs WHERE id = ?1",
        params![job_id],
        |row| row.get(0),
    )?;
    Ok(status == "canceled")
}

/// 需要合成的分段 id：空列表表示全书（按章节与分段顺序展开）。
fn tts_segment_ids(conn: &Connection, segment_ids: Vec<String>) -> StudioResult<Vec<String>> {
    if !segment_ids.is_empty() {
        return Ok(segment_ids);
    }
    let mut stmt = conn.prepare(
        "SELECT s.id FROM segments s
         JOIN chapters c ON c.id = s.chapter_id
         ORDER BY c.order_index, s.order_index, s.id",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

struct SegmentVoiceSource {
    id: String,
    text: String,
    character_id: Option<String>,
    frozen_profile_id: Option<String>,
    age_progress: Option<f64>,
    emotion: Option<String>,
    /// 角色名；旁白时为「旁白」。仅用于拼报错信息。
    speaker_name: String,
    /// 是否已定稿（审听通过 / 人工上传），决定音色是否走冻结副本。
    finalized: bool,
}

fn load_segment_voice_sources(
    conn: &Connection,
    ids: &[String],
) -> StudioResult<Vec<SegmentVoiceSource>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.text, s.character_id, s.voice_profile_id, s.age_progress, s.emotion,
                COALESCE(c.canonical_name, '旁白'),
                CASE WHEN s.review_status = 'approved'
                       OR EXISTS (
                         SELECT 1 FROM segment_audio a
                         WHERE a.segment_id = s.id
                           AND (a.status = 'approved' OR a.source = 'manual_upload')
                       )
                     THEN 1 ELSE 0 END
         FROM segments s
         LEFT JOIN characters c ON c.id = s.character_id
         WHERE s.id = ?1",
    )?;
    let mut sources = Vec::new();
    for id in ids {
        let source = stmt.query_row(params![id], |row| {
            Ok(SegmentVoiceSource {
                id: row.get(0)?,
                text: row.get(1)?,
                character_id: row.get(2)?,
                frozen_profile_id: row.get(3)?,
                age_progress: row.get(4)?,
                emotion: row.get(5)?,
                speaker_name: row.get(6)?,
                finalized: row.get::<_, i64>(7)? == 1,
            })
        })?;
        sources.push(source);
    }
    Ok(sources)
}

/// 生成前的音色校验：任何说话人解析不到音色都直接报错，并列出是谁、涉及多少段。
/// 这是「没有音色就不许生成」的闸门，取代原来的 `COALESCE(v.tts_provider,'mock')` 静默兜底
/// ——后者正是「一部分分段用 mimo、一部分悄悄变成 mock」的机制性根因。
pub fn validate_voice_resolution(
    conn: &Connection,
    project_id: &str,
    segment_ids: Vec<String>,
) -> StudioResult<()> {
    let ids = tts_segment_ids(conn, segment_ids)?;
    let sources = load_segment_voice_sources(conn, &ids)?;
    let mut missing: Vec<(String, usize)> = Vec::new();
    for source in &sources {
        let resolved = resolve_voice_profile_for_segment(
            conn,
            project_id,
            source.frozen_profile_id.as_deref(),
            source.character_id.as_deref(),
            source.age_progress,
            source.finalized,
        )?;
        if resolved.is_none() {
            match missing
                .iter_mut()
                .find(|(name, _)| name == &source.speaker_name)
            {
                Some((_, count)) => *count += 1,
                None => missing.push((source.speaker_name.clone(), 1)),
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let detail = missing
        .iter()
        .map(|(name, count)| format!("{name}（{count} 段）"))
        .collect::<Vec<_>>()
        .join("、");
    Err(err(format!(
        "以下说话人还没有音色，请先定音色再生成：{detail}"
    )))
}

fn load_tts_requests(
    conn: &Connection,
    project_root: &Path,
    project_id: &str,
    segment_ids: Vec<String>,
) -> StudioResult<Vec<TtsRequest>> {
    let ids = tts_segment_ids(conn, segment_ids)?;
    let sources = load_segment_voice_sources(conn, &ids)?;
    let mut requests = Vec::new();
    for source in sources {
        let profile_id = resolve_voice_profile_for_segment(
            conn,
            project_id,
            source.frozen_profile_id.as_deref(),
            source.character_id.as_deref(),
            source.age_progress,
            source.finalized,
        )?
        .ok_or_else(|| {
            err(format!(
                "「{}」还没有音色，请先为它定音色再生成",
                source.speaker_name
            ))
        })?;
        let voice = conn.query_row(
            "SELECT v.tts_provider, v.model, v.voice_id, v.speed, v.pitch, v.style,
                    a.relative_path, a.mime_type
             FROM voice_profiles v
             LEFT JOIN voice_assets a ON v.voice_asset_id = a.id
             WHERE v.id = ?1",
            params![profile_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )?;
        let voice_sample = match (voice.6, voice.7) {
            (Some(relative_path), Some(mime_type)) => {
                Some(load_voice_sample(project_root, &relative_path, &mime_type)?)
            }
            _ => None,
        };
        // 台词情绪并入 style：与角色表演提示拼接，供应商按自然语言理解
        let parts: Vec<String> = [voice.5.as_deref(), source.emotion.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .collect();
        let style = if parts.is_empty() {
            None
        } else {
            Some(parts.join("，"))
        };
        requests.push(TtsRequest {
            segment_id: source.id,
            text: source.text,
            tts_provider: voice.0,
            model: voice.1,
            voice_id: voice.2,
            speed: voice.3,
            pitch: voice.4,
            style,
            voice_sample,
        });
    }
    Ok(requests)
}

fn load_voice_sample(
    project_root: &Path,
    relative_path: &str,
    mime_type: &str,
) -> StudioResult<VoiceSample> {
    let sample_path = safe_project_asset_path(project_root, relative_path)?;
    let data_base64 = STANDARD.encode(fs::read(&sample_path)?);
    let data_uri_size = data_base64.len() + mime_type.len() + 13;
    if data_uri_size > 10 * 1024 * 1024 {
        return Err(err(format!(
            "音色样本编码后超过 Mimo 10 MB 限制：{}",
            sample_path.display()
        )));
    }
    Ok(VoiceSample {
        mime_type: mime_type.to_string(),
        data_base64,
    })
}

fn safe_project_asset_path(
    project_root: &Path,
    relative_path: &str,
) -> StudioResult<std::path::PathBuf> {
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        || !relative.starts_with("assets/source/voices")
    {
        return Err(err("音色样本路径无效"));
    }
    Ok(project_root.join(relative))
}

fn count_protected_segments(
    conn: &Connection,
    requests: &[TtsRequest],
    options: &TtsSynthesisOptions,
) -> StudioResult<usize> {
    if options.force_regenerate {
        return Ok(0);
    }
    let mut count = 0;
    for request in requests {
        if is_segment_protected(conn, &request.segment_id)? {
            count += 1;
        }
    }
    Ok(count)
}

fn filter_tts_requests(
    conn: &Connection,
    requests: Vec<TtsRequest>,
    options: &TtsSynthesisOptions,
) -> StudioResult<Vec<TtsRequest>> {
    if options.force_regenerate {
        return Ok(requests);
    }
    let mut out = Vec::new();
    for request in requests {
        if !is_segment_protected(conn, &request.segment_id)? {
            out.push(request);
        }
    }
    Ok(out)
}

fn is_segment_protected(conn: &Connection, segment_id: &str) -> StudioResult<bool> {
    let review_status: String = conn.query_row(
        "SELECT review_status FROM segments WHERE id = ?1",
        params![segment_id],
        |row| row.get(0),
    )?;
    if review_status == "approved" {
        return Ok(true);
    }
    let Some(audio) = latest_audio_for_segment(conn, segment_id)? else {
        return Ok(false);
    };
    Ok(audio.status == "approved" || audio.source == "manual_upload")
}

/// 闸门：音色档案里带了克隆参考音频，但解析出的模型不是 voiceclone 时直接失败。
///
/// 为什么必须失败而不是警告：`mimo_request_body` 的 voicedesign 分支构造 `audio` 时
/// **不会读 `voice_sample`**，参考音频会被静默丢弃，音色变成"按文字描述重新设计"的
/// 另一个人；请求仍然 200 成功，界面上看不出任何异常。
/// 触发场景：档案 `model` 为 NULL 时回退到设置里的默认模型，而设置默认是 voicedesign。
fn ensure_model_matches_voice_sample(request: &TtsRequest, model: &str) -> StudioResult<()> {
    if request.voice_sample.is_some() && !model.contains("voiceclone") {
        return Err(err(format!(
            "「{}」的音色档案带克隆样本，但模型是「{model}」，克隆参考音频会被忽略。\
             请为这个角色重新固化音色（走 voiceclone）后再合成。",
            request.segment_id
        )));
    }
    Ok(())
}

fn normalize_mimo_model(model: Option<&str>) -> String {
    let model = model.unwrap_or("mimo-v2.5-tts").trim();
    if model.is_empty() {
        "mimo-v2.5-tts".to_string()
    } else {
        model.to_ascii_lowercase()
    }
}

/// VoiceDesign 的音色**完全由这段提示文字决定**，所以试听和成片必须发同一段文字。
///
/// 2026-09-25 实测（真实接口，同一句台词、同一模型，三组提示互不相同）：
/// - 发「音色描述」（试听走的口径）        → 2.72s / 130,604 B / RMS 3424
/// - 发「默认兜底提示」（批量走的口径）    → 2.08s /  99,884 B / RMS 2839 ← 明显换人
/// - 发「音色描述 + 表演提示」（本函数口径）→ 2.88s / 138,284 B / RMS 3835
///
/// 旧实现只把 `style`（表演提示）发出去，**`voice_id`（音色描述）根本没进请求体**，
/// 于是「试听听中的那个声音」在批量生成时不会复现；角色档案若 `style` 为空，
/// 提示还会退化成一句与角色无关的默认值。
fn build_voice_prompt(request: &TtsRequest, model: &str) -> String {
    let style = request
        .style
        .as_deref()
        .map(str::trim)
        .filter(|style| !style.is_empty());
    if model.contains("voicedesign") {
        // 音色描述在前（决定"是谁"），表演提示在后（决定"怎么演"）。
        // 试听路径把描述放在 style、voice_id 留空，两者拼接后与旧行为一致。
        let description = request.voice_id.trim();
        let description = if description.is_empty() || description.starts_with("clone:") {
            None
        } else {
            Some(description)
        };
        let parts: Vec<&str> = [description, style]
            .into_iter()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .collect();
        if !parts.is_empty() {
            return parts.join("，");
        }
    }
    style.unwrap_or("自然、清晰、适合有声读物制作").to_string()
}

fn mimo_chat_completions_endpoint(endpoint: Option<&str>) -> String {
    let base = endpoint
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .unwrap_or("https://api.xiaomimimo.com/v1")
        .trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

fn mimo_request_body(request: &TtsRequest, model: &str, voice_prompt: &str) -> StudioResult<Value> {
    let audio = if model.contains("voicedesign") {
        // ⚠️ 这里**不要**下发 `optimize_text_preview`（无论 true/false 之外的取值都不要）。
        // 2026-09-25 实测：带 `optimize_text_preview: true` 时 api.xiaomimimo.com 会在约
        // 10 秒后把响应流掐断——状态码仍是 200，但响应体只有 1 个换行，reqwest 侧表现为
        // 一句无从下手的 "error decoding response body"（工坊「生成声音试听」报的就是它）。
        // 去掉之后同一次请求 0.8~1.7 秒正常返回 70~370KB 音频；只给 `{"format":"wav"}`
        // 或不给 audio 字段都正常。
        // 回归口径见 `mimo_voicedesign_live_returns_audio`（联网用例，需 --ignored 手动跑）。
        json!({
            "format": "wav"
        })
    } else if model.contains("voiceclone") {
        let sample = request
            .voice_sample
            .as_ref()
            .ok_or_else(|| err("Mimo 音色模仿缺少参考音频"))?;
        json!({
            "voice": format!("data:{};base64,{}", sample.mime_type, sample.data_base64),
            "format": "wav"
        })
    } else {
        let voice = if request.voice_id.trim().is_empty() || request.voice_id == "mimo_default" {
            "Mia"
        } else {
            request.voice_id.as_str()
        };
        json!({
            "voice": voice,
            "format": "wav"
        })
    };
    Ok(json!({
        "model": model,
        "modalities": ["audio", "text"],
        "stream": false,
        "audio": audio,
        "messages": [
            {
                "role": "user",
                "content": voice_prompt
            },
            {
                "role": "assistant",
                "content": request.text
            }
        ]
    }))
}

fn extract_mimo_audio_base64(payload: &Value) -> Option<&str> {
    payload
        .pointer("/choices/0/message/audio/data")
        .and_then(|value| value.as_str())
        .or_else(|| {
            payload
                .pointer("/choices/0/message/audio")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            payload
                .pointer("/audio/data")
                .and_then(|value| value.as_str())
        })
        .or_else(|| {
            payload
                .pointer("/output/audio")
                .and_then(|value| value.as_str())
        })
}

fn silent_wav(duration_ms: u32) -> Vec<u8> {
    let sample_rate = 16_000u32;
    let channels = 1u16;
    let bits_per_sample = 16u16;
    let samples = sample_rate * duration_ms / 1000;
    let data_size = samples * channels as u32 * (bits_per_sample as u32 / 8);
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(
        &(sample_rate * channels as u32 * bits_per_sample as u32 / 8).to_le_bytes(),
    );
    out.extend_from_slice(&(channels * bits_per_sample / 8).to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    out.resize(out.len() + data_size as usize, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mimo_endpoint_accepts_base_url_or_full_chat_url() {
        assert_eq!(
            mimo_chat_completions_endpoint(Some("https://api.xiaomimimo.com/v1")),
            "https://api.xiaomimimo.com/v1/chat/completions"
        );
        assert_eq!(
            mimo_chat_completions_endpoint(Some("https://api.xiaomimimo.com/v1/chat/completions")),
            "https://api.xiaomimimo.com/v1/chat/completions"
        );
    }

    #[test]
    fn mimo_audio_parser_reads_openai_compatible_shape() {
        let payload = json!({
            "choices": [
                {
                    "message": {
                        "audio": {
                            "data": "YWJj"
                        }
                    }
                }
            ]
        });
        assert_eq!(extract_mimo_audio_base64(&payload), Some("YWJj"));
    }

    /// 回归：voicedesign 请求体里不得出现 `optimize_text_preview`。
    /// 这个字段曾把工坊「生成声音试听」打成 200 + 空响应体（详见 `mimo_request_body` 注释）。
    #[test]
    fn mimo_voice_design_request_omits_optimize_text_preview() {
        let request = TtsRequest {
            segment_id: "seg-1".to_string(),
            text: "这是一段旁白。".to_string(),
            tts_provider: "mimo".to_string(),
            model: Some("mimo-v2.5-tts-voicedesign".to_string()),
            voice_id: "温柔、清澈、适合长篇有声书旁白的成年女声".to_string(),
            voice_sample: None,
            speed: 1.0,
            pitch: 0.0,
            style: Some("温柔旁白".to_string()),
        };

        let body = mimo_request_body(&request, "mimo-v2.5-tts-voicedesign", "温柔旁白").unwrap();

        assert_eq!(
            body.pointer("/audio/format").and_then(|v| v.as_str()),
            Some("wav")
        );
        assert!(
            body.pointer("/audio/optimize_text_preview").is_none(),
            "optimize_text_preview 会让 mimo 网关在 ~10s 后掐断响应流"
        );
        assert!(body.pointer("/audio/response_format").is_none());
        assert!(body.pointer("/audio/voice").is_none());
    }

    fn voicedesign_request(voice_id: &str, style: Option<&str>) -> TtsRequest {
        TtsRequest {
            segment_id: "seg-voice".to_string(),
            text: "萧炎哥哥，你终于来了。".to_string(),
            tts_provider: "mimo".to_string(),
            model: Some("mimo-v2.5-tts-voicedesign".to_string()),
            voice_id: voice_id.to_string(),
            voice_sample: None,
            speed: 1.0,
            pitch: 0.0,
            style: style.map(str::to_string),
        }
    }

    /// 回归：voicedesign 的提示必须同时带上「音色描述」和「表演提示」。
    ///
    /// 旧实现只把 `style` 发出去，`voice_id` 根本没进请求体 —— 试听听中的那个声音
    /// 在批量合成时不会复现（实测同一句话换成另一段音频，见 `build_voice_prompt`）。
    #[test]
    fn voicedesign_prompt_carries_description_and_style() {
        let request = voicedesign_request("清冷温润的少女音色", Some("克制而疏离"));
        assert_eq!(
            build_voice_prompt(&request, "mimo-v2.5-tts-voicedesign"),
            "清冷温润的少女音色，克制而疏离"
        );
    }

    /// 角色档案 `style` 为空（萧薰儿那条就是 NULL）时，不能退化成与角色无关的默认值。
    #[test]
    fn voicedesign_prompt_survives_missing_style() {
        let request = voicedesign_request("清冷温润的少女音色", None);
        assert_eq!(
            build_voice_prompt(&request, "mimo-v2.5-tts-voicedesign"),
            "清冷温润的少女音色"
        );

        // 试听口径：描述放在 style、voice_id 留空，改动前后行为一致。
        let audition = voicedesign_request("", Some("温柔旁白"));
        assert_eq!(
            build_voice_prompt(&audition, "mimo-v2.5-tts-voicedesign"),
            "温柔旁白"
        );

        // clone 档案的 voice_id 是 `clone:<asset>`，不是音色描述，不能塞进提示。
        let clone = voicedesign_request("clone:336bd712", Some("自然"));
        assert_eq!(
            build_voice_prompt(&clone, "mimo-v2.5-tts-voicedesign"),
            "自然"
        );
    }

    /// 克隆样本配非克隆模型 = 参考音频被静默丢弃，必须直接报错而不是出声。
    #[test]
    fn clone_sample_with_non_clone_model_is_rejected() {
        let mut request = voicedesign_request("clone:336bd712", Some("自然"));
        request.voice_sample = Some(VoiceSample {
            mime_type: "audio/wav".to_string(),
            data_base64: "AAAA".to_string(),
        });

        let error = ensure_model_matches_voice_sample(&request, "mimo-v2.5-tts-voicedesign")
            .expect_err("带克隆样本却不是 voiceclone 模型时必须失败");
        assert!(
            error.to_string().contains("voiceclone"),
            "错误信息要给出修法：{error}"
        );

        assert!(ensure_model_matches_voice_sample(&request, "mimo-v2.5-tts-voiceclone").is_ok());

        request.voice_sample = None;
        assert!(ensure_model_matches_voice_sample(&request, "mimo-v2.5-tts").is_ok());
    }

    /// 联网契约测试：对真实 Mimo 接口跑一次 voicedesign，要求拿回完整 WAV。
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored mimo_voicedesign_live_returns_audio --nocapture
    /// ```
    /// API Key 优先取环境变量 `MIMO_API_KEY`，缺省时读本机 app 的密钥文件。
    ///
    /// 为什么必须有这一条：单元测试断言的是我们自己拼的 JSON，永远发现不了"服务端不收这个
    /// 字段"。2026-09-25 的事故（`error decoding response body`）根因正是请求体字段，
    /// 而当时那条单测还把错字段断言成"必须存在"。
    #[test]
    #[ignore = "联网 + 需要 MIMO_API_KEY，手动运行"]
    fn mimo_voicedesign_live_returns_audio() {
        let Some(api_key) = live_mimo_api_key() else {
            panic!("未找到 Mimo API Key：设 MIMO_API_KEY，或让本机 app 已保存过 key");
        };
        let provider = MimoTtsProvider {
            settings: ProviderSettings {
                provider: "mimo".to_string(),
                api_key: Some(api_key),
                endpoint: None,
                model: None,
            },
        };
        let request = TtsRequest {
            segment_id: "live-voicedesign".to_string(),
            text: "萧炎，斗之力，三段！级别：低级！".to_string(),
            tts_provider: "mimo".to_string(),
            model: Some("mimo-v2.5-tts-voicedesign".to_string()),
            voice_id: String::new(),
            voice_sample: None,
            speed: 1.0,
            pitch: 0.0,
            style: Some("中年男性，音色偏低沉浑厚，略带一丝沧桑的沙哑质感，中气十足。语速中等偏快，吐字清晰。".to_string()),
        };

        let result = provider
            .synthesize_blocking(&request)
            .expect("voicedesign 合成应成功");
        assert_eq!(result.extension, "wav");
        assert!(
            result.audio_bytes.len() > 10_000,
            "音频过短：{} 字节",
            result.audio_bytes.len()
        );
        assert_eq!(&result.audio_bytes[..4], b"RIFF", "不是合法 WAV");
    }

    fn live_mimo_api_key() -> Option<String> {
        std::env::var("MIMO_API_KEY")
            .ok()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
            .or_else(|| crate::read_provider_secret("mimo"))
    }

    /// 回归：分段上的人工情绪必须进合成请求的 `style`（供应商把它当表演提示解释）。
    ///
    /// 用户报「固化了音色、情绪不起作用」时这条链两端都可能断：
    /// ① 前端没把情绪存进库（见 `useStudioActions::saveSegmentDraft`）；
    /// ② 存了却没拼进请求 —— 本用例钉住②，并且要求它与角色档案的 style **拼接**而非互相覆盖。
    #[test]
    fn segment_emotion_reaches_the_tts_request_style() {
        let root = std::env::temp_dir().join(format!("xiic-tts-emotion-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三: 斗之力，三段！").unwrap();
        let source = crate::importer::read_source(&source_path).unwrap();
        let summary = crate::storage::create_project(&root, "情绪链路测试", None).unwrap();
        let project_id = summary.manifest.id.clone();
        let conn = crate::storage::open_connection(&root).unwrap();
        let chapter_id = crate::importer::import_source(&conn, &project_id, &source)
            .unwrap()
            .remove(0);
        crate::importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();

        let character_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO characters (id, project_id, canonical_name, gender, age_timeline, notes, default_color)
             VALUES (?1, ?2, '中年男子', 'male', 'adult', NULL, '#8a8f98')",
            params![character_id, project_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE segments SET character_id = ?1, speaker = '中年男子', emotion = '大声宣布'",
            params![character_id],
        )
        .unwrap();
        ensure_default_voice_profiles(&conn, &project_id).unwrap();
        conn.execute(
            "UPDATE voice_profiles SET style = '语气公事公办' WHERE character_id = ?1",
            params![character_id],
        )
        .unwrap();

        let requests = load_tts_requests(&conn, &root, &project_id, Vec::new()).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].style.as_deref(),
            Some("语气公事公办，大声宣布"),
            "分段情绪必须与角色档案 style 一起进请求"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mimo_preset_tts_request_uses_voice_and_wav_format() {
        let request = TtsRequest {
            segment_id: "seg-2".to_string(),
            text: "这是一句角色对白。".to_string(),
            tts_provider: "mimo".to_string(),
            model: Some("mimo-v2.5-tts".to_string()),
            voice_id: "Milo".to_string(),
            voice_sample: None,
            speed: 1.0,
            pitch: 0.0,
            style: None,
        };

        let body = mimo_request_body(&request, "mimo-v2.5-tts", "角色对白").unwrap();

        assert_eq!(
            body.pointer("/audio/voice").and_then(|v| v.as_str()),
            Some("Milo")
        );
        assert_eq!(
            body.pointer("/audio/format").and_then(|v| v.as_str()),
            Some("wav")
        );
        assert!(body.pointer("/audio/optimize_text_preview").is_none());
    }

    #[test]
    fn mimo_voice_clone_request_embeds_the_reference_sample() {
        let request = TtsRequest {
            segment_id: "seg-clone".to_string(),
            text: "这是模仿音色测试。".to_string(),
            tts_provider: "mimo".to_string(),
            model: Some("mimo-v2.5-tts-voiceclone".to_string()),
            voice_id: "clone:sample".to_string(),
            voice_sample: Some(VoiceSample {
                mime_type: "audio/wav".to_string(),
                data_base64: "YWJj".to_string(),
            }),
            speed: 1.0,
            pitch: 0.0,
            style: Some("自然旁白".to_string()),
        };

        let body = mimo_request_body(&request, "mimo-v2.5-tts-voiceclone", "自然旁白").unwrap();

        assert_eq!(
            body.pointer("/audio/voice").and_then(Value::as_str),
            Some("data:audio/wav;base64,YWJj")
        );
        assert_eq!(
            body.pointer("/model").and_then(Value::as_str),
            Some("mimo-v2.5-tts-voiceclone")
        );
    }
}
