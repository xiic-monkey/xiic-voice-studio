mod ai;
mod audio;
mod domain;
mod error;
mod importer;
mod settings;
mod storage;
#[cfg(test)]
mod tests;
mod tts;

use crate::domain::*;
use crate::error::{err, StudioResult};
use crate::storage::now;
use keyring::{Entry, Error as KeyringError};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;
use uuid::Uuid;

#[derive(Debug, Default)]
struct AppState {
    project_root: std::sync::Mutex<Option<PathBuf>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectRequest {
    root_path: String,
    title: String,
    author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportSourceRequest {
    source_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSegmentRequest {
    segment_id: String,
    text: String,
    segment_type: SegmentType,
    speaker: Option<String>,
    emotion: Option<String>,
    sound_cue: Option<String>,
    anchor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssignVoiceProfileRequest {
    character_id: Option<String>,
    name: String,
    age_stage: String,
    tts_provider: String,
    model: Option<String>,
    voice_id: String,
    voice_asset_id: Option<String>,
    speed: f64,
    pitch: f64,
    style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCharacterRequest {
    character_id: String,
    canonical_name: String,
    aliases: Vec<String>,
    gender: Option<String>,
    age_timeline: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateVoiceProfileRequest {
    profile_id: String,
    name: String,
    age_stage: String,
    tts_provider: String,
    model: Option<String>,
    voice_id: String,
    voice_asset_id: Option<String>,
    speed: f64,
    pitch: f64,
    style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateVoiceCloneRequest {
    character_id: Option<String>,
    name: String,
    age_stage: String,
    source_path: String,
    style: Option<String>,
    consent_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TtsBatchRequest {
    segment_ids: Vec<String>,
    settings: Option<tts::ProviderSettings>,
    force_regenerate: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TtsTestRequest {
    settings: Option<tts::ProviderSettings>,
    voice_id: String,
    style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VoiceProfileTestRequest {
    profile_id: String,
    settings: Option<tts::ProviderSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewIssueRequest {
    segment_id: Option<String>,
    audio_id: Option<String>,
    issue_type: String,
    note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadSegmentAudioRequest {
    segment_id: String,
    source_path: String,
    ffmpeg_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentAudioStatusRequest {
    segment_id: String,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSecretStatus {
    provider: String,
    exists: bool,
}

const LLM_KEYCHAIN_ID: &str = "llm-openai-compatible";

#[tauri::command]
fn create_project(
    state: tauri::State<'_, AppState>,
    request: CreateProjectRequest,
) -> StudioResult<StudioSnapshot> {
    let root = PathBuf::from(request.root_path);
    let summary = storage::create_project(&root, &request.title, request.author)?;
    *state.project_root.lock().expect("project root lock") = Some(root.clone());
    let conn = storage::open_connection(&root)?;
    tts::ensure_default_voice_profiles(&conn, &summary.manifest.id)?;
    storage::snapshot(&root)
}

#[tauri::command]
fn open_project(
    state: tauri::State<'_, AppState>,
    root_path: String,
) -> StudioResult<StudioSnapshot> {
    let root = PathBuf::from(root_path);
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    storage::recover_incomplete_jobs(&conn)?;
    tts::ensure_default_voice_profiles(&conn, &manifest.id)?;
    *state.project_root.lock().expect("project root lock") = Some(root.clone());
    storage::snapshot(&root)
}

#[tauri::command]
fn import_source(
    state: tauri::State<'_, AppState>,
    request: ImportSourceRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    let source_path = PathBuf::from(&request.source_path);
    let imported = importer::read_source(&source_path)?;
    copy_source_asset(&source_path, &root)?;
    importer::import_source(&conn, &manifest.id, &imported)?;
    tts::ensure_default_voice_profiles(&conn, &manifest.id)?;
    storage::snapshot(&root)
}

#[tauri::command]
fn list_chapters(state: tauri::State<'_, AppState>) -> StudioResult<Vec<Chapter>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::list_chapters(&conn)
}

#[tauri::command]
fn delete_chapter(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::delete_chapter(&conn, &chapter_id)?;
    storage::snapshot(&root)
}

#[tauri::command]
async fn mark_chapter(
    state: tauri::State<'_, AppState>,
    chapter_id: String,
    settings: Option<ai::LlmSettings>,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let settings = llm_settings_with_stored_key(settings)?;
    let (project_id, prepared) = {
        let (manifest, conn) = storage::ensure_project_loaded(&root)?;
        let prepared = ai::prepare_mark_chapter(&conn, &manifest.id, &chapter_id, settings)?;
        (manifest.id, prepared)
    };
    let job_id = prepared.job_id.clone();
    let provider_payload = match (prepared.raw_text.as_deref(), prepared.settings) {
        (Some(raw_text), Some(settings)) => {
            match ai::call_openai_compatible(raw_text, settings).await {
                Ok(payload) => Some(payload),
                Err(error) => {
                    let conn = storage::open_connection(&root)?;
                    storage::mark_job(&conn, &job_id, "failed", 0.0, Some(&error.to_string()))?;
                    return Err(err(format!("章节标注失败：{error}")));
                }
            }
        }
        _ => None,
    };
    {
        let conn = storage::open_connection(&root)?;
        if let Err(error) =
            ai::finish_mark_chapter(&conn, &project_id, &chapter_id, &job_id, provider_payload)
        {
            storage::mark_job(&conn, &job_id, "failed", 0.0, Some(&error.to_string()))?;
            return Err(err(format!("章节标注失败：{error}")));
        }
        tts::ensure_default_voice_profiles(&conn, &project_id)?;
    }
    storage::snapshot(&root)
}

#[tauri::command]
fn list_segments(
    state: tauri::State<'_, AppState>,
    chapter_id: Option<String>,
) -> StudioResult<Vec<Segment>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::list_segments(&conn, chapter_id.as_deref())
}

#[tauri::command]
fn update_segment(
    state: tauri::State<'_, AppState>,
    request: UpdateSegmentRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let current: (String, String, Option<String>, Option<String>, Option<String>, Option<String>) =
        conn.query_row(
            "SELECT text, segment_type, speaker, emotion, sound_cue, anchor FROM segments WHERE id = ?1",
            params![request.segment_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )?;
    let script_changed = current.0 != request.text
        || current.1 != request.segment_type.as_str()
        || current.2 != request.speaker
        || current.3 != request.emotion
        || current.4 != request.sound_cue
        || current.5 != request.anchor;
    conn.execute(
        "UPDATE segments SET text = ?1, segment_type = ?2, speaker = ?3, emotion = ?4, sound_cue = ?5, anchor = ?6, is_manual_edit = 1, updated_at = ?7 WHERE id = ?8",
        params![
            request.text,
            request.segment_type.as_str(),
            request.speaker,
            request.emotion,
            request.sound_cue,
            request.anchor,
            now(),
            request.segment_id
        ],
    )?;
    if script_changed {
        audio::invalidate_segment_audio(
            &conn,
            &request.segment_id,
            "脚本内容已变更，请重新生成或上传并审听音频",
        )?;
    }
    storage::snapshot(&root)
}

#[tauri::command]
fn list_characters(state: tauri::State<'_, AppState>) -> StudioResult<Vec<Character>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::list_characters(&conn)
}

#[tauri::command]
fn update_character(
    state: tauri::State<'_, AppState>,
    request: UpdateCharacterRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, mut conn) = storage::ensure_project_loaded(&root)?;
    let name = request.canonical_name.trim();
    if name.is_empty() {
        return Err(err("角色名不能为空"));
    }
    let old_name: String = conn.query_row(
        "SELECT canonical_name FROM characters WHERE id = ?1 AND project_id = ?2",
        params![request.character_id, manifest.id],
        |row| row.get(0),
    )?;
    let mut aliases = Vec::new();
    for alias in request.aliases {
        let alias = alias.trim();
        if !alias.is_empty() && alias != name && !aliases.iter().any(|value| value == alias) {
            aliases.push(alias.to_string());
        }
    }
    if old_name != name && !aliases.iter().any(|alias| alias == &old_name) {
        aliases.push(old_name);
    }
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE characters
         SET canonical_name = ?1, gender = ?2, age_timeline = ?3, notes = ?4
         WHERE id = ?5 AND project_id = ?6",
        params![
            name,
            request.gender,
            request.age_timeline,
            request.notes,
            request.character_id,
            manifest.id
        ],
    )?;
    tx.execute(
        "DELETE FROM character_aliases WHERE character_id = ?1",
        params![request.character_id],
    )?;
    for alias in aliases {
        tx.execute(
            "INSERT INTO character_aliases (id, character_id, alias) VALUES (?1, ?2, ?3)",
            params![Uuid::new_v4().to_string(), request.character_id, alias],
        )?;
    }
    tx.commit()?;
    storage::snapshot(&root)
}

#[tauri::command]
fn merge_characters(
    state: tauri::State<'_, AppState>,
    source_character_id: String,
    target_character_id: String,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    merge_character_records(&conn, &source_character_id, &target_character_id)?;
    storage::snapshot(&root)
}

fn merge_character_records(
    conn: &rusqlite::Connection,
    source_character_id: &str,
    target_character_id: &str,
) -> StudioResult<()> {
    if source_character_id == target_character_id {
        return Err(err("不能合并同一个角色"));
    }
    let source_name: String = conn.query_row(
        "SELECT canonical_name FROM characters WHERE id = ?1",
        params![source_character_id],
        |row| row.get(0),
    )?;
    let segments_to_invalidate =
        tts::segment_ids_matching_voice_scope(conn, Some(source_character_id), true)?;
    let mut target_profile_id =
        tts::preferred_voice_profile_for_character(conn, target_character_id)?;
    if target_profile_id.is_none() {
        conn.execute(
            "UPDATE voice_profiles SET character_id = ?1 WHERE character_id = ?2",
            params![target_character_id, source_character_id],
        )?;
        target_profile_id = tts::preferred_voice_profile_for_character(conn, target_character_id)?;
    }
    conn.execute(
        "UPDATE segments SET character_id = ?1, voice_profile_id = ?2, updated_at = ?3 WHERE character_id = ?4",
        params![target_character_id, target_profile_id, now(), source_character_id],
    )?;
    if target_profile_id.is_some() {
        conn.execute(
            "DELETE FROM voice_profiles WHERE character_id = ?1",
            params![source_character_id],
        )?;
    }
    for segment_id in segments_to_invalidate {
        audio::invalidate_segment_audio(
            conn,
            &segment_id,
            "角色已合并并可能更换声音，请重新生成或上传并审听音频",
        )?;
    }
    conn.execute(
        "INSERT INTO character_aliases (id, character_id, alias) VALUES (?1, ?2, ?3)",
        params![Uuid::new_v4().to_string(), target_character_id, source_name],
    )?;
    conn.execute(
        "DELETE FROM characters WHERE id = ?1",
        params![source_character_id],
    )?;
    Ok(())
}

#[tauri::command]
fn assign_voice_profile(
    state: tauri::State<'_, AppState>,
    request: AssignVoiceProfileRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    let profile_id = Uuid::new_v4().to_string();
    let character_id = request.character_id.clone();
    let segments_to_invalidate =
        tts::segment_ids_matching_voice_scope(&conn, character_id.as_deref(), true)?;
    conn.execute(
        "INSERT INTO voice_profiles
         (id, project_id, character_id, name, age_stage, tts_provider, model, voice_id,
          voice_asset_id, speed, pitch, style)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            profile_id,
            manifest.id,
            character_id,
            request.name,
            request.age_stage,
            request.tts_provider,
            request.model,
            request.voice_id,
            request.voice_asset_id,
            request.speed,
            request.pitch,
            request.style
        ],
    )?;
    tts::bind_voice_profile_to_matching_segments(
        &conn,
        &profile_id,
        character_id.as_deref(),
        true,
    )?;
    for segment_id in segments_to_invalidate {
        audio::invalidate_segment_audio(
            &conn,
            &segment_id,
            "声音配置已变更，请重新生成或上传并审听音频",
        )?;
    }
    storage::snapshot(&root)
}

#[tauri::command]
fn update_voice_profile(
    state: tauri::State<'_, AppState>,
    request: UpdateVoiceProfileRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    if request.name.trim().is_empty() || request.voice_id.trim().is_empty() {
        return Err(err("声音名称和音色标识不能为空"));
    }
    let character_id: Option<String> = conn.query_row(
        "SELECT character_id FROM voice_profiles WHERE id = ?1 AND project_id = ?2",
        params![request.profile_id, manifest.id],
        |row| row.get(0),
    )?;
    let segments_to_invalidate: Vec<String> = conn
        .prepare(
            "SELECT s.id FROM segments s
             WHERE s.voice_profile_id = ?1
               AND EXISTS (
                 SELECT 1 FROM segment_audio a
                 WHERE a.segment_id = s.id
                   AND a.status IN ('approved', 'generated', 'uploaded')
                   AND a.source != 'manual_upload'
               )
             ORDER BY s.id",
        )?
        .query_map(params![request.profile_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    conn.execute(
        "UPDATE voice_profiles
         SET name = ?1, age_stage = ?2, tts_provider = ?3, model = ?4, voice_id = ?5,
             voice_asset_id = ?6, speed = ?7, pitch = ?8, style = ?9
         WHERE id = ?10 AND project_id = ?11",
        params![
            request.name.trim(),
            request.age_stage,
            request.tts_provider,
            request.model,
            request.voice_id.trim(),
            request.voice_asset_id,
            request.speed,
            request.pitch,
            request.style,
            request.profile_id,
            manifest.id
        ],
    )?;
    tts::bind_voice_profile_to_matching_segments(
        &conn,
        &request.profile_id,
        character_id.as_deref(),
        true,
    )?;
    for segment_id in segments_to_invalidate {
        audio::invalidate_segment_audio(
            &conn,
            &segment_id,
            "声音档案已变更，请重新生成并审听音频",
        )?;
    }
    storage::snapshot(&root)
}

#[tauri::command]
fn delete_voice_profile(
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM voice_profiles WHERE id = ?1 AND project_id = ?2",
        params![profile_id, manifest.id],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Err(err("声音档案不存在"));
    }
    let segments_to_invalidate: Vec<String> = conn
        .prepare(
            "SELECT s.id FROM segments s
             WHERE s.voice_profile_id = ?1
               AND EXISTS (
                 SELECT 1 FROM segment_audio a
                 WHERE a.segment_id = s.id
                   AND a.status IN ('approved', 'generated', 'uploaded')
                   AND a.source != 'manual_upload'
               )",
        )?
        .query_map(params![profile_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    conn.execute(
        "UPDATE segments SET voice_profile_id = NULL, updated_at = ?1 WHERE voice_profile_id = ?2",
        params![now(), profile_id],
    )?;
    conn.execute(
        "DELETE FROM voice_profiles WHERE id = ?1 AND project_id = ?2",
        params![profile_id, manifest.id],
    )?;
    for segment_id in segments_to_invalidate {
        audio::invalidate_segment_audio(
            &conn,
            &segment_id,
            "声音档案已删除，请重新分配声音并生成音频",
        )?;
    }
    storage::snapshot(&root)
}

#[tauri::command]
fn create_mimo_voice_clone(
    state: tauri::State<'_, AppState>,
    request: CreateVoiceCloneRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, mut conn) = storage::ensure_project_loaded(&root)?;
    tts::create_mimo_voice_clone_profile(
        &mut conn,
        &root,
        &manifest.id,
        request.character_id.as_deref(),
        &request.name,
        &request.age_stage,
        &PathBuf::from(request.source_path),
        request.style.as_deref(),
        request.consent_confirmed,
    )?;
    storage::snapshot(&root)
}

#[tauri::command]
async fn enqueue_tts_batch(
    state: tauri::State<'_, AppState>,
    request: TtsBatchRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    let settings = provider_settings_with_stored_key(request.settings)?;
    let force_regenerate = request.force_regenerate.unwrap_or(false);
    let options = tts::TtsSynthesisOptions { force_regenerate };
    let job_id = tts::create_tts_job(
        &conn,
        &manifest.id,
        &request.segment_ids,
        settings.as_ref(),
        &options,
    )?;
    spawn_tts_batch(
        root.clone(),
        manifest.id,
        job_id,
        request.segment_ids,
        settings,
        force_regenerate,
    );
    storage::snapshot(&root)
}

#[tauri::command]
async fn test_tts(app: tauri::AppHandle, request: TtsTestRequest) -> StudioResult<String> {
    let settings = provider_settings_with_stored_key(request.settings)?;
    let output_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| err(format!("无法确定应用缓存目录：{error}")))?
        .join("tts-tests");
    let output_path = tauri::async_runtime::spawn_blocking(move || {
        tts::synthesize_test_audio_to_dir(&output_dir, settings, request.voice_id, request.style)
    })
    .await
    .map_err(|error| err(format!("TTS 测试任务异常：{error}")))??;
    Ok(output_path.to_string_lossy().to_string())
}

#[tauri::command]
async fn test_voice_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: VoiceProfileTestRequest,
) -> StudioResult<String> {
    let root = current_root(&state)?;
    let settings = provider_settings_with_stored_key(request.settings)?;
    let output_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| err(format!("无法确定应用缓存目录：{error}")))?
        .join("voice-profile-tests");
    let profile_id = request.profile_id;
    tauri::async_runtime::spawn_blocking(move || {
        let conn = storage::open_connection(&root)?;
        tts::synthesize_voice_profile_test_audio(&conn, &root, &output_dir, &profile_id, settings)
    })
    .await
    .map_err(|error| err(format!("音色试听任务异常：{error}")))?
    .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
fn list_jobs(state: tauri::State<'_, AppState>) -> StudioResult<Vec<StudioJob>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::list_jobs(&conn)
}

#[tauri::command]
fn cancel_job(state: tauri::State<'_, AppState>, job_id: String) -> StudioResult<Vec<StudioJob>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let job = storage::list_jobs(&conn)?
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| err("任务不存在，无法取消"))?;
    if !matches!(job.status.as_str(), "pending" | "running") {
        return Err(err("任务已经结束，无法取消"));
    }
    storage::mark_job(&conn, &job_id, "canceled", 0.0, Some("用户已取消"))?;
    storage::list_jobs(&conn)
}

#[tauri::command]
async fn retry_job(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> StudioResult<Vec<StudioJob>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let job = storage::list_jobs(&conn)?
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| err("任务不存在，无法重试"))?;
    if job.job_type != "tts_batch" {
        return Err(err("当前只支持重试语音生成任务"));
    }
    if !matches!(job.status.as_str(), "failed" | "canceled") {
        return Err(err("只有失败或已取消的任务可以重试"));
    }
    let payload: serde_json::Value = serde_json::from_str(&job.payload_json)?;
    let segment_ids = payload
        .pointer("/segmentIds")
        .and_then(|value| value.as_array())
        .ok_or_else(|| err("任务缺少分段范围，无法重试"))?
        .iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let provider = payload
        .pointer("/provider")
        .and_then(|value| value.as_str())
        .unwrap_or("mock")
        .to_string();
    let settings = provider_settings_with_stored_key(Some(tts::ProviderSettings {
        provider,
        api_key: None,
        endpoint: payload
            .pointer("/endpoint")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        model: payload
            .pointer("/model")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    }))?;
    let force_regenerate = payload
        .pointer("/forceRegenerate")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    storage::mark_job(&conn, &job_id, "canceled", 0.0, Some("已创建重试任务"))?;
    let options = tts::TtsSynthesisOptions { force_regenerate };
    let new_job_id = tts::create_tts_job(
        &conn,
        &job.project_id,
        &segment_ids,
        settings.as_ref(),
        &options,
    )?;
    spawn_tts_batch(
        root.clone(),
        job.project_id,
        new_job_id,
        segment_ids,
        settings,
        force_regenerate,
    );
    storage::list_jobs(&conn)
}

#[tauri::command]
fn play_segment_audio(
    state: tauri::State<'_, AppState>,
    segment_id: String,
) -> StudioResult<Option<String>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    audio::segment_audio_path(&conn, &root, &segment_id)
}

#[tauri::command]
fn get_audio_output_directory(state: tauri::State<'_, AppState>) -> StudioResult<String> {
    let root = current_root(&state)?;
    let manifest = storage::load_manifest(&root)?;
    Ok(audio::audio_output_directory(&root, &manifest.id)?
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
fn review_segment_audio(
    state: tauri::State<'_, AppState>,
    request: ReviewIssueRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    audio::create_review_issue(
        &conn,
        request.segment_id,
        request.audio_id,
        request.issue_type,
        request.note,
    )?;
    storage::snapshot(&root)
}

#[tauri::command]
fn upload_segment_audio(
    state: tauri::State<'_, AppState>,
    request: UploadSegmentAudioRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    audio::upload_segment_audio(
        &conn,
        &root,
        &request.segment_id,
        &PathBuf::from(request.source_path),
        request.ffmpeg_path.as_deref(),
    )?;
    storage::snapshot(&root)
}

#[tauri::command]
fn set_segment_audio_status(
    state: tauri::State<'_, AppState>,
    request: SegmentAudioStatusRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    audio::set_latest_segment_audio_status(&conn, &request.segment_id, &request.status)?;
    storage::snapshot(&root)
}

#[tauri::command]
async fn regenerate_segment(
    state: tauri::State<'_, AppState>,
    segment_id: String,
    settings: Option<tts::ProviderSettings>,
) -> StudioResult<StudioSnapshot> {
    enqueue_tts_batch(
        state,
        TtsBatchRequest {
            segment_ids: vec![segment_id],
            settings,
            force_regenerate: Some(true),
        },
    )
    .await
}

#[tauri::command]
fn export_voice_script(
    state: tauri::State<'_, AppState>,
    chapter_ids: Option<Vec<String>>,
) -> StudioResult<String> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    Ok(
        audio::export_voice_script_for_chapters(&conn, &root, chapter_ids.as_deref())?
            .to_string_lossy()
            .to_string(),
    )
}

#[tauri::command]
fn export_character_script(state: tauri::State<'_, AppState>) -> StudioResult<String> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    Ok(audio::export_character_script(&conn, &root)?
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
fn export_segment_audio(
    state: tauri::State<'_, AppState>,
    chapter_ids: Option<Vec<String>>,
) -> StudioResult<String> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    Ok(
        audio::export_segment_audio_for_chapters(&conn, &root, chapter_ids.as_deref())?
            .to_string_lossy()
            .to_string(),
    )
}

#[tauri::command]
fn export_production_package(
    state: tauri::State<'_, AppState>,
    chapter_ids: Option<Vec<String>>,
) -> StudioResult<String> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    Ok(
        audio::export_production_package_for_chapters(&conn, &root, chapter_ids.as_deref())?
            .to_string_lossy()
            .to_string(),
    )
}

#[tauri::command]
fn check_production_readiness(
    state: tauri::State<'_, AppState>,
    chapter_ids: Option<Vec<String>>,
) -> StudioResult<ProductionCheckReport> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    audio::export_production_check_report_for_chapters(&conn, &root, chapter_ids.as_deref())
}

#[tauri::command]
async fn export_episode(
    state: tauri::State<'_, AppState>,
    ffmpeg_path: Option<String>,
    chapter_ids: Option<Vec<String>>,
    format: Option<String>,
) -> StudioResult<String> {
    let root = current_root(&state)?;
    let (selection, manifest) = {
        let conn = storage::open_connection(&root)?;
        (
            audio::collect_episode_audio_selection_for_chapters(
                &conn,
                &root,
                chapter_ids.as_deref(),
            )?,
            storage::load_manifest(&root)?,
        )
    };
    audio::export_episode_from_selection(
        &root,
        selection.paths,
        selection.chapters,
        ffmpeg_path,
        format.as_deref().unwrap_or("m4b"),
        Some(&manifest.title),
        manifest.author.as_deref(),
        audio::project_cover_path(&root).as_deref(),
    )
    .await
    .map(|path| path.to_string_lossy().to_string())
}

#[tauri::command]
fn set_project_cover(
    state: tauri::State<'_, AppState>,
    source_path: String,
) -> StudioResult<String> {
    let root = current_root(&state)?;
    Ok(
        audio::copy_project_cover(&root, &PathBuf::from(source_path))?
            .to_string_lossy()
            .to_string(),
    )
}

#[tauri::command]
fn get_project_cover(state: tauri::State<'_, AppState>) -> StudioResult<Option<String>> {
    let root = current_root(&state)?;
    Ok(audio::project_cover_path(&root).map(|path| path.to_string_lossy().to_string()))
}

#[tauri::command]
fn check_project_assets(state: tauri::State<'_, AppState>) -> StudioResult<AssetValidationReport> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    audio::validate_project_assets(&conn, &root)
}

#[tauri::command]
fn export_project_backup(state: tauri::State<'_, AppState>) -> StudioResult<String> {
    let root = current_root(&state)?;
    Ok(audio::export_project_backup(&root)?
        .to_string_lossy()
        .to_string())
}

#[tauri::command]
async fn check_ffmpeg(ffmpeg_path: Option<String>) -> StudioResult<String> {
    audio::ffmpeg_version(ffmpeg_path).await
}

#[tauri::command]
fn load_app_settings(app: tauri::AppHandle) -> StudioResult<settings::AppSettings> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| err(format!("无法确定应用配置目录：{error}")))?;
    settings::load(&config_dir)
}

#[tauri::command]
fn save_app_settings(
    app: tauri::AppHandle,
    value: settings::AppSettings,
) -> StudioResult<settings::AppSettings> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| err(format!("无法确定应用配置目录：{error}")))?;
    settings::save(&config_dir, &value)?;
    Ok(value)
}

#[tauri::command]
async fn test_llm(settings: ai::LlmSettings) -> StudioResult<String> {
    let settings =
        llm_settings_with_stored_key(Some(settings))?.ok_or_else(|| err("LLM 配置不能为空"))?;
    ai::test_openai_compatible(settings).await
}

#[tauri::command]
fn list_voices(settings: Option<tts::ProviderSettings>) -> StudioResult<Vec<VoiceInfo>> {
    tts::list_voices(settings)
}

#[tauri::command]
fn save_provider_api_key(provider: String, api_key: String) -> StudioResult<ProviderSecretStatus> {
    let provider = normalize_provider_id(&provider)?;
    let api_key = api_key.trim();
    if api_key.is_empty() {
        return Err(err("API Key 不能为空"));
    }
    provider_keychain_entry(&provider)?.set_password(api_key)?;
    Ok(ProviderSecretStatus {
        provider,
        exists: true,
    })
}

#[tauri::command]
fn get_provider_api_key(provider: String) -> StudioResult<Option<String>> {
    let provider = normalize_provider_id(&provider)?;
    match provider_keychain_entry(&provider)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[tauri::command]
fn delete_provider_api_key(provider: String) -> StudioResult<ProviderSecretStatus> {
    let provider = normalize_provider_id(&provider)?;
    match provider_keychain_entry(&provider)?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(ProviderSecretStatus {
            provider,
            exists: false,
        }),
        Err(error) => Err(error.into()),
    }
}

fn current_root(state: &tauri::State<'_, AppState>) -> StudioResult<PathBuf> {
    state
        .project_root
        .lock()
        .expect("project root lock")
        .clone()
        .ok_or_else(|| err("请先打开或创建项目"))
}

fn copy_source_asset(
    source_path: &std::path::Path,
    project_root: &std::path::Path,
) -> StudioResult<()> {
    let source_dir = project_root.join("assets/source");
    fs::create_dir_all(&source_dir)?;
    let source_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source.txt");
    let destination = source_dir.join(source_name);
    let is_same_file = fs::canonicalize(source_path).ok() == fs::canonicalize(&destination).ok();
    if is_same_file {
        return Ok(());
    }
    if destination.exists() {
        let stem = destination
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("source");
        let extension = destination
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{value}"))
            .unwrap_or_default();
        let destination = source_dir.join(format!("{stem}-{}{extension}", Uuid::new_v4()));
        fs::copy(source_path, destination)?;
    } else {
        fs::copy(source_path, destination)?;
    }
    Ok(())
}

fn provider_settings_with_stored_key(
    settings: Option<tts::ProviderSettings>,
) -> StudioResult<Option<tts::ProviderSettings>> {
    let Some(mut settings) = settings else {
        return Ok(None);
    };
    let provider = normalize_provider_id(&settings.provider)?;
    settings.provider = provider.clone();
    let has_key = settings
        .api_key
        .as_deref()
        .map(str::trim)
        .map(|key| !key.is_empty())
        .unwrap_or(false);
    if !has_key {
        settings.api_key = match provider_keychain_entry(&provider)?.get_password() {
            Ok(value) => Some(value),
            Err(KeyringError::NoEntry) => None,
            Err(error) => return Err(error.into()),
        };
    }
    Ok(Some(settings))
}

fn llm_settings_with_stored_key(
    settings: Option<ai::LlmSettings>,
) -> StudioResult<Option<ai::LlmSettings>> {
    let Some(mut settings) = settings else {
        return Ok(None);
    };
    let has_key = settings
        .api_key
        .as_deref()
        .map(str::trim)
        .map(|key| !key.is_empty())
        .unwrap_or(false);
    if !has_key {
        settings.api_key = match provider_keychain_entry(LLM_KEYCHAIN_ID)?.get_password() {
            Ok(value) => Some(value),
            Err(KeyringError::NoEntry) => None,
            Err(error) => return Err(error.into()),
        };
    }
    Ok(Some(settings))
}

fn normalize_provider_id(provider: &str) -> StudioResult<String> {
    let provider = provider.trim();
    if provider.is_empty() {
        return Err(err("供应商不能为空"));
    }
    Ok(provider.to_ascii_lowercase())
}

fn provider_keychain_entry(provider: &str) -> StudioResult<Entry> {
    Entry::new("xiic-voice-studio", provider).map_err(Into::into)
}

fn spawn_tts_batch(
    root: PathBuf,
    project_id: String,
    job_id: String,
    segment_ids: Vec<String>,
    settings: Option<tts::ProviderSettings>,
    force_regenerate: bool,
) {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = match storage::open_connection(&root) {
            Ok(conn) => conn,
            Err(error) => {
                if let Ok(fallback) = rusqlite::Connection::open(storage::database_path(&root)) {
                    let _ = storage::mark_job(
                        &fallback,
                        &job_id,
                        "failed",
                        0.0,
                        Some(&format!("任务启动失败：{error}")),
                    );
                }
                return;
            }
        };
        let result = tts::synthesize_segments_for_job(
            &conn,
            &root,
            &project_id,
            &job_id,
            segment_ids,
            settings,
            tts::TtsSynthesisOptions { force_regenerate },
        );
        if let Err(error) = result {
            let _ = storage::mark_job(&conn, &job_id, "failed", 0.0, Some(&error.to_string()));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("Xiic Voice Studio");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_project,
            open_project,
            import_source,
            list_chapters,
            delete_chapter,
            mark_chapter,
            list_segments,
            update_segment,
            list_characters,
            update_character,
            merge_characters,
            assign_voice_profile,
            update_voice_profile,
            delete_voice_profile,
            create_mimo_voice_clone,
            enqueue_tts_batch,
            test_tts,
            test_voice_profile,
            list_jobs,
            cancel_job,
            retry_job,
            play_segment_audio,
            get_audio_output_directory,
            review_segment_audio,
            upload_segment_audio,
            set_segment_audio_status,
            regenerate_segment,
            export_voice_script,
            export_character_script,
            export_segment_audio,
            export_production_package,
            check_production_readiness,
            export_episode,
            set_project_cover,
            get_project_cover,
            check_project_assets,
            export_project_backup,
            check_ffmpeg,
            load_app_settings,
            save_app_settings,
            test_llm,
            list_voices,
            save_provider_api_key,
            get_provider_api_key,
            delete_provider_api_key
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
