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
use std::time::Duration;
use uuid::Uuid;

const PROVIDER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const PROVIDER_MAX_ATTEMPTS: usize = 3;

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
        let response_text = send_with_retries(|| {
            client
                .post(&endpoint)
                .bearer_auth(&api_key)
                .json(&request_body)
                .send()
        })?
        .text()?;
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
        let voice_prompt = request
            .style
            .as_deref()
            .map(str::trim)
            .filter(|style| !style.is_empty())
            .unwrap_or("自然、清晰、适合有声读物制作");
        let request_body = mimo_request_body(request, &model, voice_prompt)?;
        let client = provider_http_client()?;
        let response = send_with_retries(|| {
            client
                .post(&endpoint)
                .header("api-key", api_key)
                .json(&request_body)
                .send()
        })?;
        let status = response.status();
        let response_text = response.text()?;
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

pub fn ensure_default_voice_profiles(conn: &Connection, project_id: &str) -> StudioResult<()> {
    let narrator_profile_id = ensure_default_narrator_profile(conn, project_id)?;
    bind_voice_profile_to_matching_segments(conn, &narrator_profile_id, None, false)?;
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

pub fn preferred_voice_profile_for_character(
    conn: &Connection,
    character_id: &str,
) -> StudioResult<Option<String>> {
    let profile_id = conn
        .query_row(
            "SELECT id FROM voice_profiles
             WHERE character_id = ?1
             ORDER BY
               CASE WHEN tts_provider = 'mimo' THEN 0 ELSE 1 END,
               name
             LIMIT 1",
            params![character_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(profile_id)
}

fn ensure_default_narrator_profile(conn: &Connection, project_id: &str) -> StudioResult<String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM voice_profiles
             WHERE project_id = ?1 AND character_id IS NULL
             ORDER BY
               CASE
                 WHEN tts_provider = 'mimo' THEN 0
                 WHEN name LIKE '%旁白%' THEN 1
                 ELSE 2
               END,
               name
             LIMIT 1",
            params![project_id],
            |row| row.get(0),
        )
        .ok();
    if let Some(profile_id) = existing {
        return Ok(profile_id);
    }
    let profile_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO voice_profiles
         (id, project_id, character_id, name, age_stage, tts_provider, model, voice_id, speed, pitch, style)
         VALUES (?1, ?2, NULL, 'Mimo 旁白声音', 'adult', 'mimo', 'mimo-v2.5-tts-voicedesign', ?3, 1.0, 0.0, ?4)",
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
    let selected_segments = load_tts_requests(conn, project_root, segment_ids)?;
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

fn load_tts_requests(
    conn: &Connection,
    project_root: &Path,
    segment_ids: Vec<String>,
) -> StudioResult<Vec<TtsRequest>> {
    let ids = if segment_ids.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT s.id FROM segments s
             JOIN chapters c ON c.id = s.chapter_id
             ORDER BY c.order_index, s.order_index, s.id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    } else {
        segment_ids
    };
    let mut requests = Vec::new();
    for id in ids {
        let request = conn.query_row(
            "SELECT s.id, s.text, COALESCE(v.tts_provider, 'mock'), v.model,
                    COALESCE(v.voice_id, 'mock-female-narrator'), COALESCE(v.speed, 1.0),
                    COALESCE(v.pitch, 0.0), v.style, a.relative_path, a.mime_type
             FROM segments s
             LEFT JOIN voice_profiles v ON s.voice_profile_id = v.id
             LEFT JOIN voice_assets a ON v.voice_asset_id = a.id
             WHERE s.id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, f64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                ))
            },
        )?;
        let voice_sample = match (request.8, request.9) {
            (Some(relative_path), Some(mime_type)) => {
                Some(load_voice_sample(project_root, &relative_path, &mime_type)?)
            }
            _ => None,
        };
        requests.push(TtsRequest {
            segment_id: request.0,
            text: request.1,
            tts_provider: request.2,
            model: request.3,
            voice_id: request.4,
            speed: request.5,
            pitch: request.6,
            style: request.7,
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

fn normalize_mimo_model(model: Option<&str>) -> String {
    let model = model.unwrap_or("mimo-v2.5-tts").trim();
    if model.is_empty() {
        "mimo-v2.5-tts".to_string()
    } else {
        model.to_ascii_lowercase()
    }
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
        json!({
            "optimize_text_preview": true,
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

    #[test]
    fn mimo_voice_design_request_uses_official_audio_format() {
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
        assert_eq!(
            body.pointer("/audio/optimize_text_preview")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(body.pointer("/audio/response_format").is_none());
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
