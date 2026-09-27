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
mod voice_context;

use crate::domain::*;
use crate::error::{err, StudioResult};
use crate::storage::now;
use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use base64::Engine as _;
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
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
    /// None：默认启发式自动识别；Some：用户选择/输入的正则
    chapter_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSegmentRequest {
    segment_id: String,
    text: String,
    segment_type: SegmentType,
    speaker: Option<String>,
    /// 绑定到的角色；空字符串或 None 表示旁白（落到旁白声音档案）。
    character_id: Option<String>,
    emotion: Option<String>,
    sound_cue: Option<String>,
    anchor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SplitSegmentRequest {
    segment_id: String,
    /// 在文本的第几个字符处拆开（左半沿用原段，右半新建）。
    offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergeSegmentsRequest {
    segment_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InsertSegmentRequest {
    chapter_id: String,
    /// 省略 = 插到章首（空章节添加第一句时没有"上一段"可指）。
    #[serde(default)]
    after_segment_id: Option<String>,
    text: String,
    segment_type: SegmentType,
    character_id: Option<String>,
    speaker: Option<String>,
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
struct CreateCharacterRequest {
    canonical_name: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    gender: Option<String>,
    #[serde(default)]
    age_timeline: Option<String>,
    #[serde(default)]
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
struct SaveSegmentRecordingRequest {
    segment_id: String,
    /// MediaRecorder 产出的音频，base64 编码
    data_base64: String,
    /// 如 audio/webm;codecs=opus、audio/mp4、audio/wav
    mime_type: String,
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
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: CreateProjectRequest,
) -> StudioResult<StudioSnapshot> {
    let root = PathBuf::from(request.root_path);
    let summary = storage::create_project(&root, &request.title, request.author)?;
    *state.project_root.lock().expect("project root lock") = Some(root.clone());
    persist_last_project_root(&app, &root);
    let conn = storage::open_connection(&root)?;
    tts::ensure_default_voice_profiles(&conn, &summary.manifest.id)?;
    storage::snapshot(&root)
}

#[tauri::command]
fn open_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    root_path: String,
) -> StudioResult<StudioSnapshot> {
    let root = PathBuf::from(root_path);
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    storage::recover_incomplete_jobs(&conn)?;
    tts::ensure_default_voice_profiles(&conn, &manifest.id)?;
    *state.project_root.lock().expect("project root lock") = Some(root.clone());
    persist_last_project_root(&app, &root);
    storage::snapshot(&root)
}

/// 记住最近打开的项目目录，下次启动自动恢复。失败不打扰用户（仅是便利功能）。
fn persist_last_project_root(app: &tauri::AppHandle, root: &Path) {
    let Ok(config_dir) = app.path().app_config_dir() else {
        return;
    };
    let Ok(mut settings) = settings::load(&config_dir) else {
        return;
    };
    settings.workspace.last_project_root = Some(root.to_string_lossy().to_string());
    let _ = settings::save(&config_dir, &settings);
}

#[tauri::command]
fn get_last_project_root(app: tauri::AppHandle) -> Option<String> {
    let config_dir = app.path().app_config_dir().ok()?;
    settings::load(&config_dir)
        .ok()?
        .workspace
        .last_project_root
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
    let pattern = match request.chapter_pattern.as_deref() {
        Some(pattern) => Some(importer::compile_split_pattern(pattern)?),
        None => importer::detect_chapter_pattern(&imported.text),
    };
    importer::import_source_with_pattern(&conn, &manifest.id, &imported, pattern.as_ref())?;
    tts::ensure_default_voice_profiles(&conn, &manifest.id)?;
    storage::snapshot(&root)
}

#[tauri::command]
fn list_chapter_rules() -> StudioResult<Vec<importer::SplitRuleDto>> {
    Ok(importer::split_rule_dtos())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewSplitRequest {
    source_path: String,
    chapter_pattern: Option<String>,
}

#[tauri::command]
fn preview_chapter_split(
    request: PreviewSplitRequest,
) -> StudioResult<importer::SplitPreview> {
    // 预览不依赖已打开的项目，独立连接只读文件即可
    let imported = importer::read_source(Path::new(&request.source_path))?;
    let pattern = match request.chapter_pattern.as_deref() {
        Some(pattern) => Some(importer::compile_split_pattern(pattern)?),
        None => importer::detect_chapter_pattern(&imported.text),
    };
    Ok(importer::preview_split(&imported.text, pattern.as_ref()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetectChapterRuleRequest {
    source_path: String,
    /// 可选的自然语言格式说明，引导 LLM 归纳正则
    hint: Option<String>,
    settings: Option<ai::LlmSettings>,
}

#[tauri::command]
async fn detect_chapter_rule(
    request: DetectChapterRuleRequest,
) -> StudioResult<String> {
    let settings = llm_settings_with_stored_key(request.settings)?;
    let imported = importer::read_source(Path::new(&request.source_path))?;
    let excerpt = ai::excerpt_for_rule_detection(&imported.text);
    let pattern = ai::detect_chapter_rule(
        &excerpt,
        request.hint.as_deref(),
        settings.ok_or_else(|| err("请先在设置中配置 LLM 标注"))?,
    )
    .await?;
    // 编译通过才返回，语法错误直接暴露给用户
    importer::compile_split_pattern(&pattern)?;
    Ok(pattern)
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
    let (manifest, conn) = storage::ensure_project_loaded(&root)?;
    let settings = llm_settings_with_stored_key(settings)?;
    let prepared = ai::prepare_mark_chapter(&conn, &manifest.id, &chapter_id, settings)?;
    let project_id = manifest.id;
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

/// 可空的人工文本字段（情绪等）：去空白后为空则视作"没有值"。
/// 统一成 `None` 而不是 `Some("")`，库里的语义才唯一。
fn normalize_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
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
    // 情绪是可空的人工表演提示：空白一律归一成 NULL，否则库里会留下 `''`，
    // 且"清空情绪"与"本来就是空"会被算成两次不同的改动，白白让音频失效。
    let emotion = normalize_optional_text(request.emotion.as_deref());
    let script_changed = current.0 != request.text
        || current.1 != request.segment_type.as_str()
        || current.2 != request.speaker
        || current.3 != emotion
        || current.4 != request.sound_cue
        || current.5 != request.anchor;

    // 角色绑定：选角色即带出其首选声音；选旁白（空）落到旁白声音档案。
    let character_id = request.character_id.filter(|value| !value.trim().is_empty());
    let project_id: String = conn.query_row(
        "SELECT c.project_id FROM segments s JOIN chapters c ON c.id = s.chapter_id WHERE s.id = ?1",
        params![request.segment_id],
        |row| row.get(0),
    )?;
    let voice_profile_id: Option<String> = match &character_id {
        Some(character_id) => tts::preferred_voice_profile_for_character(&conn, character_id)?,
        None => Some(tts::ensure_default_narrator_profile(&conn, &project_id)?),
    };
    conn.execute(
        "UPDATE segments SET text = ?1, segment_type = ?2, speaker = ?3, character_id = ?4, voice_profile_id = ?5, emotion = ?6, sound_cue = ?7, anchor = ?8, is_manual_edit = 1, updated_at = ?9 WHERE id = ?10",
        params![
            request.text,
            request.segment_type.as_str(),
            request.speaker,
            character_id,
            voice_profile_id,
            emotion,
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

/// 删除分段的归档保留期。撤销窗口只有几十秒，但"过一天才想起来"同样常见，
/// 而磁盘上的音频是用户花钱生成的——多留几天的成本远低于重生成。
const DELETED_SEGMENT_RETAIN_DAYS: i64 = 7;

/// 清理超过保留期的归档，并删除它们对应的磁盘音频文件。
///
/// 挂在删除/恢复之后顺带跑：这两个动作本身就低频，且此刻一定有项目上下文，
/// 不必额外引入启动钩子或定时任务。
fn purge_expired_deleted_segments(root: &Path, conn: &rusqlite::Connection) {
    let Ok(paths) = storage::purge_expired_deleted_segments(conn, DELETED_SEGMENT_RETAIN_DAYS)
    else {
        return;
    };
    for relative_path in paths {
        // 文件清理尽力而为，失败不影响归档本身的清理
        if let Ok(path) = audio::resolve_audio_path(root, &relative_path) {
            let _ = fs::remove_file(path);
        }
    }
}

#[tauri::command]
fn delete_segment(
    state: tauri::State<'_, AppState>,
    segment_id: String,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let (manifest, _) = storage::ensure_project_loaded(&root)?;
    // 归档式删除：分段整行连同音频记录一起留存，磁盘文件也不删，
    // 所以这里不再 fs::remove_file——那正是"误触即永久丢失"的根源。
    storage::delete_segment(&conn, &manifest.id, &segment_id)?;
    purge_expired_deleted_segments(&root, &conn);
    storage::snapshot(&root)
}

/// 撤销删除：把归档的分段连同音频、审听备注一起插回原位。
#[tauri::command]
fn restore_segment(
    state: tauri::State<'_, AppState>,
    segment_id: String,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::restore_segment(&conn, &segment_id)?;
    purge_expired_deleted_segments(&root, &conn);
    storage::snapshot(&root)
}

#[tauri::command]
fn split_segment(
    state: tauri::State<'_, AppState>,
    request: SplitSegmentRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::split_segment_at(&conn, &request.segment_id, request.offset)?;
    storage::snapshot(&root)
}

#[tauri::command]
fn merge_segments(
    state: tauri::State<'_, AppState>,
    request: MergeSegmentsRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let orphaned = storage::merge_segments(&conn, &request.segment_ids)?;
    for relative_path in orphaned {
        if let Ok(path) = audio::resolve_audio_path(&root, &relative_path) {
            let _ = fs::remove_file(path);
        }
    }
    storage::snapshot(&root)
}

#[tauri::command]
fn insert_segment(
    state: tauri::State<'_, AppState>,
    request: InsertSegmentRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let character_id = request.character_id.filter(|value| !value.trim().is_empty());
    let after_segment_id = request
        .after_segment_id
        .as_deref()
        .filter(|value| !value.trim().is_empty());
    storage::insert_segment_after(
        &conn,
        &request.chapter_id,
        after_segment_id,
        &request.text,
        request.segment_type,
        character_id,
        request.speaker,
    )?;
    storage::snapshot(&root)
}

#[tauri::command]
fn list_characters(state: tauri::State<'_, AppState>) -> StudioResult<Vec<Character>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::list_characters(&conn)
}

/// 手动新建角色。
///
/// 标注流程之外也需要能建角色：有些角色只是被叙述提到（没被标成台词的说话人），
/// 或者用户想先建好档案、之后再慢慢把分段挂上去。重名与别名冲突由
/// `storage::create_character` 挡住；这里补上"立刻给一条默认音色档"，
/// 否则新角色一进合成就会撞上「这个角色没有音色」的闸门。
#[tauri::command]
fn create_character(
    state: tauri::State<'_, AppState>,
    request: CreateCharacterRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let (manifest, mut conn) = storage::ensure_project_loaded(&root)?;
    storage::create_character(
        &mut conn,
        &manifest.id,
        &request.canonical_name,
        &request.aliases,
        request.gender.as_deref(),
        request.age_timeline.as_deref(),
        request.notes.as_deref(),
    )?;
    tts::ensure_default_voice_profiles(&conn, &manifest.id)?;
    storage::snapshot(&root)
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
    // 目标角色已经有"人定过"的音色 → 保留它；否则丢掉目标的自动默认档、改用来源角色的音色。
    // 只看"有没有档案"会被自动兜底的默认音色骗过去，所以这里用 user_voice_profile_for_character。
    let mut target_profile_id = tts::user_voice_profile_for_character(conn, target_character_id)?;
    if target_profile_id.is_none() {
        conn.execute(
            "DELETE FROM voice_profiles WHERE character_id = ?1 AND is_default = 1",
            params![target_character_id],
        )?;
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
    // 生成前的音色闸门：任何说话人没有音色都不入队，也不许静默兜底成 mock。
    tts::validate_voice_resolution(&conn, &manifest.id, request.segment_ids.clone())?;
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
async fn test_tts(
    app: tauri::AppHandle,
    request: TtsTestRequest,
) -> StudioResult<String> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateVoiceDescriptionRequest {
    character_id: String,
    settings: Option<ai::LlmSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CharacterVoiceContextRequest {
    character_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CharacterVoiceContextPayload {
    /// 依据章节上下文生成的音色描述草稿（纯本地，无需 LLM）
    draft: String,
    line_count: usize,
    narration_count: usize,
    /// 抽样台词，供界面展示"参考了什么"
    sample_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceDescriptionPayload {
    description: String,
    /// llm = AI 精修；draft = 回退到章节上下文草稿
    source: String,
    warning: Option<String>,
    line_count: usize,
    narration_count: usize,
}

struct CharacterProfile {
    name: String,
    aliases: Vec<String>,
    gender: Option<String>,
    age_timeline: Option<String>,
    notes: Option<String>,
}

fn load_character_profile(conn: &rusqlite::Connection, character_id: &str) -> StudioResult<CharacterProfile> {
    let (name, gender, age_timeline, notes): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn.query_row(
        "SELECT canonical_name, gender, age_timeline, notes FROM characters WHERE id = ?1",
        params![character_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let mut stmt = conn.prepare("SELECT alias FROM character_aliases WHERE character_id = ?1")?;
    let aliases = stmt
        .query_map(params![character_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(CharacterProfile {
        name,
        aliases,
        gender,
        age_timeline,
        notes,
    })
}

/// 装配该角色的章节上下文：自己的台词 + 关于他的叙述。
fn build_voice_context(
    conn: &rusqlite::Connection,
    character_id: &str,
) -> StudioResult<(CharacterProfile, voice_context::VoiceContext)> {
    let profile = load_character_profile(conn, character_id)?;
    let mut keywords = vec![profile.name.clone()];
    keywords.extend(profile.aliases.iter().cloned());
    let segments = storage::list_segments_for_voice_context(conn, character_id, &keywords)?;
    let context = voice_context::assemble(character_id, &profile.name, &profile.aliases, &segments);
    Ok((profile, context))
}

fn draft_for(profile: &CharacterProfile, context: &voice_context::VoiceContext) -> String {
    voice_context::compose_draft(
        &profile.name,
        &profile.aliases,
        profile.gender.as_deref(),
        profile.age_timeline.as_deref(),
        profile.notes.as_deref(),
        context,
    )
}

/// 送给 LLM 的角色资料：性别/年龄阶段先归一成中文，不把 `female` / `adult` 这类
/// 内部 token 直接丢给模型——否则模型会照抄进音色描述里。
fn character_info_text(profile: &CharacterProfile) -> String {
    let aliases = if profile.aliases.is_empty() {
        "无".to_string()
    } else {
        profile.aliases.join("、")
    };
    let gender = profile
        .gender
        .as_deref()
        .and_then(voice_context::normalize_gender)
        .unwrap_or_else(|| "未标注".to_string());
    let age = profile
        .age_timeline
        .as_deref()
        .and_then(voice_context::age_stage_label)
        .unwrap_or_else(|| "未标注".to_string());
    format!(
        "别名：{aliases}；性别：{gender}；年龄阶段：{age}；备注：{}",
        profile.notes.as_deref().unwrap_or("无"),
    )
}

/// 工坊打开时调用：给出该角色的章节上下文概览与可用的音色描述草稿。
#[tauri::command]
fn character_voice_context(
    state: tauri::State<'_, AppState>,
    request: CharacterVoiceContextRequest,
) -> StudioResult<CharacterVoiceContextPayload> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let (profile, context) = build_voice_context(&conn, &request.character_id)?;
    Ok(CharacterVoiceContextPayload {
        draft: draft_for(&profile, &context),
        line_count: context.line_count(),
        narration_count: context.narration_count(),
        sample_lines: context.lines.iter().take(6).cloned().collect(),
    })
}

/// 依据章节上下文让 LLM 精修音色描述。
///
/// 与旧实现的区别：① 送进去的是拆干净的台词 + 相关叙述，而不是「对白混叙述」的整段原文；
/// ② 返回值会被校验，疑似照抄原文时先严格重试一次，仍不合格就回退到本地草稿并给出 warning——
/// 宁可给一份能用的草稿，也不把章节原文塞回描述框。
#[tauri::command]
async fn generate_voice_description(
    state: tauri::State<'_, AppState>,
    request: GenerateVoiceDescriptionRequest,
) -> StudioResult<VoiceDescriptionPayload> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let settings = llm_settings_with_stored_key(request.settings)?
        .ok_or_else(|| err("请先在「设置 · LLM 标注」里填写并保存 API Key"))?;
    if settings
        .api_key
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        return Err(err(
            "还没配置 LLM API Key：设置 · LLM 标注 里填入并保存后再试",
        ));
    }
    let (profile, context) = build_voice_context(&conn, &request.character_id)?;
    let draft = draft_for(&profile, &context);
    let info = character_info_text(&profile);

    let first = ai::generate_voice_description(
        &profile.name,
        &info,
        &context.lines,
        &context.narration(),
        settings.clone(),
        false,
    )
    .await;

    let mut warning = None;
    let mut source = "llm";
    let description = match first {
        Ok(text) if ai::invalid_voice_description_reason(&text, &context.lines).is_none() => text,
        other => {
            let reason = match &other {
                Ok(text) => ai::invalid_voice_description_reason(text, &context.lines)
                    .unwrap_or_else(|| "LLM 返回内容不可用".to_string()),
                Err(error) => error.to_string(),
            };
            let retried = ai::generate_voice_description(
                &profile.name,
                &info,
                &context.lines,
                &context.narration(),
                settings,
                true,
            )
            .await;
            match retried {
                Ok(text) if ai::invalid_voice_description_reason(&text, &context.lines).is_none() => {
                    warning = Some(format!("首次返回不合格（{reason}），已重试成功"));
                    text
                }
                _ => {
                    warning = Some(format!("{reason}；已回退到章节上下文草稿"));
                    source = "draft";
                    draft
                }
            }
        }
    };
    Ok(VoiceDescriptionPayload {
        description,
        source: source.to_string(),
        warning,
        line_count: context.line_count(),
        narration_count: context.narration_count(),
    })
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateVoiceSampleRequest {
    character_id: String,
    description: String,
    sample_text: String,
    settings: Option<tts::ProviderSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeneratedVoiceSample {
    asset_id: String,
    audio_path: String,
}

/// 用 voicedesign 按描述合成一段角色声音样本，存为项目音色资产（试听用）。
#[tauri::command]
async fn generate_character_voice_sample(
    state: tauri::State<'_, AppState>,
    request: GenerateVoiceSampleRequest,
) -> StudioResult<GeneratedVoiceSample> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let (manifest, _) = storage::ensure_project_loaded(&root)?;
    let description = request.description.trim();
    if description.is_empty() {
        return Err(err("请先填写音色描述"));
    }
    let sample_text = request.sample_text.trim();
    if sample_text.is_empty() {
        return Err(err("请先填写试听台词"));
    }
    let settings = provider_settings_with_stored_key(request.settings)?
        .ok_or_else(|| err("还没配置 TTS：设置 · 语音生成 里选择 mimo 并保存 API Key"))?;
    if settings.provider != "mimo" {
        return Err(err("音色设计流程仅支持 Mimo（voicedesign + voiceclone）"));
    }
    let provider = tts::provider_from_settings(Some(settings.clone()));
    if provider.provider_id() != "mimo" {
        return Err(err("音色设计流程仅支持 Mimo"));
    }
    let tts_request = tts::TtsRequest {
        segment_id: format!("voice-design-{}", request.character_id),
        text: sample_text.to_string(),
        tts_provider: "mimo".to_string(),
        model: Some("mimo-v2.5-tts-voicedesign".to_string()),
        voice_id: String::new(),
        voice_sample: None,
        speed: 1.0,
        pitch: 0.0,
        style: Some(description.to_string()),
    };
    let result = tauri::async_runtime::spawn_blocking(move || provider.synthesize_blocking(&tts_request))
        .await
        .map_err(|error| err(format!("音色样本合成任务异常：{error}")))??;

    let asset_id = uuid::Uuid::new_v4().to_string();
    let relative_path = format!("assets/source/voices/{asset_id}.wav");
    let destination = root.join(&relative_path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&destination, &result.audio_bytes)?;
    let timestamp = storage::now();
    conn.execute(
        "INSERT INTO voice_assets
         (id, project_id, name, asset_type, provider, model, relative_path, mime_type,
          source_file_name, consent_confirmed, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'voice_design_sample', 'mimo', 'mimo-v2.5-tts-voicedesign', ?4, 'audio/wav', ?5, 1, 'ready', ?6, ?7)",
        params![
            asset_id,
            manifest.id,
            format!("音色试听 {}", timestamp),
            relative_path,
            format!("design-{}", request.character_id),
            timestamp,
            timestamp
        ],
    )?;
    Ok(GeneratedVoiceSample {
        asset_id,
        audio_path: destination.to_string_lossy().to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FinalizeCharacterVoiceRequest {
    character_id: String,
    /// 缺省时自动使用该角色最近一次生成的试听样本
    asset_id: Option<String>,
}

/// 把试听满意的样本固化为角色音色：走 voiceclone 模式，
/// 该角色后续合成都引用同一份样本，音色确定性由样本保证。
#[tauri::command]
fn finalize_character_voice(
    state: tauri::State<'_, AppState>,
    request: FinalizeCharacterVoiceRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let (manifest, _) = storage::ensure_project_loaded(&root)?;
    let (asset_id, relative_path, mime_type): (String, String, String) = match request.asset_id.as_deref() {
        Some(asset_id) => {
            let (relative_path, mime_type) = conn
                .query_row(
                    "SELECT relative_path, mime_type FROM voice_assets WHERE id = ?1 AND project_id = ?2",
                    params![asset_id, manifest.id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or_else(|| err("音色样本不存在，请重新生成"))?;
            (asset_id.to_string(), relative_path, mime_type)
        }
        None => conn
            .query_row(
                "SELECT id, relative_path, mime_type FROM voice_assets
                 WHERE project_id = ?1 AND asset_type = 'voice_design_sample' AND source_file_name = ?2
                 ORDER BY updated_at DESC LIMIT 1",
                params![manifest.id, format!("design-{}", request.character_id)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| err("该角色还没有生成过声音样本，请先在声音工坊里生成"))?,
    };
    let sample_path = root.join(&relative_path);
    if !sample_path.is_file() {
        return Err(err("音色样本文件已丢失，请重新生成"));
    }
    let name: String = conn.query_row(
        "SELECT canonical_name FROM characters WHERE id = ?1 AND project_id = ?2",
        params![request.character_id, manifest.id],
        |row| row.get(0),
    )?;
    // 固化 = 把该角色这一阶段的音色就地写成克隆音色。
    // 旧实现把同角色的旧档案 `character_id = NULL` 解绑成"孤儿"，这些孤儿会被当成旁白档案
    // ——斗破苍穹库里那 4 条 character_id 为空的档案正是这么来的。这里改为就地更新，不再产生孤儿。
    let timestamp = storage::now();
    let voice_id = format!("clone:{asset_id}");
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM voice_profiles
             WHERE character_id = ?1 AND age_stage = 'adult'
             ORDER BY name LIMIT 1",
            params![request.character_id],
            |row| row.get(0),
        )
        .optional()?;
    match existing {
        Some(profile_id) => {
            conn.execute(
                "UPDATE voice_profiles
                 SET name = ?1, tts_provider = 'mimo', model = 'mimo-v2.5-tts-voiceclone',
                     voice_id = ?2, voice_asset_id = ?3, is_default = 0, updated_at = ?4
                 WHERE id = ?5",
                params![
                    format!("{name} 声音（已固化）"),
                    voice_id,
                    asset_id,
                    timestamp,
                    profile_id
                ],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO voice_profiles
                 (id, project_id, character_id, name, age_stage, tts_provider, model, voice_id,
                  voice_asset_id, speed, pitch, style, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'adult', 'mimo', 'mimo-v2.5-tts-voiceclone', ?5, ?6, 1.0, 0.0, NULL, ?7, ?8)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    manifest.id,
                    request.character_id,
                    format!("{name} 声音（已固化）"),
                    voice_id,
                    asset_id,
                    timestamp,
                    timestamp
                ],
            )?;
        }
    }
    mark_character_segments_pending(
        &conn,
        &request.character_id,
        "角色音色已固化，请重新生成该分段音频",
    )?;
    let _ = (relative_path, mime_type);
    storage::snapshot(&root)
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
    // 重试复用同一条任务记录：重置为待执行后再派发，避免失败任务越积越多
    storage::mark_job(&conn, &job_id, "pending", 0.0, None)?;
    spawn_tts_batch(
        root.clone(),
        job.project_id,
        job_id.clone(),
        segment_ids,
        settings,
        force_regenerate,
    );
    storage::list_jobs(&conn)
}

#[tauri::command]
fn delete_job(
    state: tauri::State<'_, AppState>,
    job_id: String,
) -> StudioResult<Vec<StudioJob>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let job = storage::list_jobs(&conn)?
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| err("任务不存在，无法删除"))?;
    if matches!(job.status.as_str(), "pending" | "running") {
        return Err(err("任务进行中，请先取消再删除"));
    }
    storage::delete_job(&conn, &job_id)?;
    storage::list_jobs(&conn)
}

#[tauri::command]
fn clear_finished_jobs(state: tauri::State<'_, AppState>) -> StudioResult<Vec<StudioJob>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    storage::clear_finished_jobs(&conn)?;
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

/// 角色音色的「固化样本」原文件路径。
/// 为什么不放在前端拼 rootPath + relativePath：样本文件可能被外部删掉，
/// 只有后端能给出「路径 + 文件是否还在」这一个确定答案，前端只管播。
#[tauri::command]
fn voice_asset_audio_path(
    state: tauri::State<'_, AppState>,
    asset_id: String,
) -> StudioResult<Option<String>> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let (manifest, _) = storage::ensure_project_loaded(&root)?;
    storage::voice_asset_audio_path(&conn, &root, &manifest.id, &asset_id)
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
fn save_segment_recording(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: SaveSegmentRecordingRequest,
) -> StudioResult<StudioSnapshot> {
    use base64::Engine as _;
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(request.data_base64.as_bytes())
        .map_err(|error| err(format!("录音数据解码失败：{error}")))?;
    if data.is_empty() {
        return Err(err("录音内容为空"));
    }
    let extension = match request.mime_type.split(';').next().unwrap_or("") {
        "audio/webm" => "webm",
        "audio/mp4" | "audio/m4a" => "m4a",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        other => return Err(err(format!("不支持的录音格式：{other}"))),
    };
    let ffmpeg_path = request.ffmpeg_path.or_else(|| {
        let config_dir = app.path().app_config_dir().ok()?;
        let value = settings::load(&config_dir).ok()?.audio.ffmpeg_path;
        (!value.is_empty()).then_some(value)
    });
    audio::save_segment_recording(
        &conn,
        &root,
        &request.segment_id,
        &data,
        extension,
        ffmpeg_path.as_deref(),
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
    write_provider_secret(&provider, api_key)?;
    Ok(ProviderSecretStatus {
        provider,
        exists: true,
    })
}

#[tauri::command]
fn get_provider_api_key(provider: String) -> StudioResult<Option<String>> {
    let provider = normalize_provider_id(&provider)?;
    Ok(read_provider_secret(&provider))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BindCharacterVoiceRequest {
    profile_id: String,
    character_id: Option<String>,
}

#[tauri::command]
fn bind_character_voice(
    state: tauri::State<'_, AppState>,
    request: BindCharacterVoiceRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let character_id = request.character_id.filter(|value| !value.trim().is_empty());
    storage::bind_voice_profile(&conn, &request.profile_id, character_id.as_deref())?;
    storage::snapshot(&root)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetCharacterVoiceRequest {
    character_id: String,
    voice_id: String,
    tts_provider: String,
    model: Option<String>,
    /// 指定要更新的音色档案；一个角色有多个年龄阶段档位时用来精确落到某一条。
    profile_id: Option<String>,
    /// 未指定 `profile_id` 时按年龄阶段定位；缺省为默认档 `adult`。
    age_stage: Option<String>,
}

fn normalize_age_stage(value: Option<&str>) -> StudioResult<&'static str> {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("adult");
    tts::AGE_STAGES
        .iter()
        .copied()
        .find(|stage| *stage == candidate)
        .ok_or_else(|| err(format!("未知的年龄阶段：{candidate}")))
}

/// 音色变更后，把该角色**尚未定稿**的分段标记为需要重新生成。
/// 已审听通过 / 人工上传的分段是定稿：音色不再跟随角色变化，
/// 这样单段返修继承的仍是生成时的音色，不会跟相邻段落脱节（product-brief 第 6 节）。
fn mark_character_segments_pending(
    conn: &rusqlite::Connection,
    character_id: &str,
    reason: &str,
) -> StudioResult<()> {
    let pending: Vec<String> = conn
        .prepare("SELECT id FROM segments WHERE character_id = ?1 AND review_status != 'approved'")?
        .query_map(params![character_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    conn.execute(
        "UPDATE segments SET voice_profile_id = NULL, updated_at = ?1
         WHERE character_id = ?2 AND review_status != 'approved'",
        params![storage::now(), character_id],
    )?;
    for segment_id in pending {
        audio::invalidate_segment_audio(conn, &segment_id, reason)?;
    }
    Ok(())
}

/// 为角色设定音色：定位该角色的某一条音色档案并就地更新（没有则新建）。
///
/// 音色是**角色的属性**（product-brief 第 4 节 `VoiceProfile: character ID, age stage, ...`）。
/// 分段不持有可独立演化的副本——合成时按 `character_id` + `age_progress` 实时解析，
/// 所以改完音色**不需要、也不允许**去清空 `segments.voice_profile_id`：
/// 那个"清空"动作正是此前「改一次角色音色就冒出一批 mock 分段」的机制性根因。
#[tauri::command]
fn set_character_voice(
    state: tauri::State<'_, AppState>,
    request: SetCharacterVoiceRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let voice_id = request.voice_id.trim();
    if voice_id.is_empty() {
        return Err(err("音色 ID 不能为空"));
    }
    let provider = normalize_provider_id(&request.tts_provider)?;
    apply_character_voice(
        &conn,
        &request.character_id,
        voice_id,
        &provider,
        request.model.as_deref(),
        request.profile_id.as_deref(),
        request.age_stage.as_deref(),
    )?;
    storage::snapshot(&root)
}

/// `set_character_voice` 的可测内核：定位目标档案 → 就地更新 → 把未定稿分段标记为待重生成。
#[allow(clippy::too_many_arguments)]
fn apply_character_voice(
    conn: &rusqlite::Connection,
    character_id: &str,
    voice_id: &str,
    provider: &str,
    model: Option<&str>,
    requested_profile_id: Option<&str>,
    requested_age_stage: Option<&str>,
) -> StudioResult<()> {
    let project_id: String = conn.query_row(
        "SELECT project_id FROM characters WHERE id = ?1",
        params![character_id],
        |row| row.get(0),
    )?;
    let name: String = conn.query_row(
        "SELECT canonical_name FROM characters WHERE id = ?1",
        params![character_id],
        |row| row.get(0),
    )?;
    let profile_id = match requested_profile_id {
        Some(profile_id) => conn
            .query_row(
                "SELECT id FROM voice_profiles WHERE id = ?1 AND character_id = ?2",
                params![profile_id, character_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| err("音色档案不存在或不属于该角色"))?,
        None => {
            let age_stage = normalize_age_stage(requested_age_stage)?;
            match conn
                .query_row(
                    "SELECT id FROM voice_profiles
                     WHERE character_id = ?1 AND age_stage = ?2
                     ORDER BY is_default ASC, name LIMIT 1",
                    params![character_id, age_stage],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            {
                Some(existing) => existing,
                None => {
                    let new_id = uuid::Uuid::new_v4().to_string();
                    let timestamp = storage::now();
                    conn.execute(
                        "INSERT INTO voice_profiles
                         (id, project_id, character_id, name, age_stage, tts_provider, model, voice_id, speed, pitch, style, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1.0, 0.0, NULL, ?9, ?10)",
                        params![
                            new_id,
                            project_id,
                            character_id,
                            format!("{name} 声音"),
                            age_stage,
                            provider,
                            model,
                            voice_id,
                            timestamp,
                            timestamp
                        ],
                    )?;
                    new_id
                }
            }
        }
    };
    conn.execute(
        "UPDATE voice_profiles
         SET voice_id = ?1, tts_provider = ?2, model = ?3, voice_asset_id = NULL,
             is_default = 0, updated_at = ?4
         WHERE id = ?5",
        params![voice_id, provider, model, storage::now(), profile_id],
    )?;
    mark_character_segments_pending(conn, character_id, "角色音色已更新，请重新生成该分段音频")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetNarratorVoiceRequest {
    voice_id: String,
    tts_provider: String,
    model: Option<String>,
}

/// 旁白音色：存储在 character_id 为空的那条声音档案上。
#[tauri::command]
fn set_narrator_voice(
    state: tauri::State<'_, AppState>,
    request: SetNarratorVoiceRequest,
) -> StudioResult<StudioSnapshot> {
    let root = current_root(&state)?;
    let conn = storage::open_connection(&root)?;
    let voice_id = request.voice_id.trim();
    if voice_id.is_empty() {
        return Err(err("音色 ID 不能为空"));
    }
    let (manifest, _) = storage::ensure_project_loaded(&root)?;
    let narrator_profile_id = tts::ensure_default_narrator_profile(&conn, &manifest.id)?;
    conn.execute(
        "UPDATE voice_profiles
         SET voice_id = ?1, tts_provider = ?2, model = ?3, is_default = 0, updated_at = ?4
         WHERE id = ?5",
        params![
            voice_id,
            normalize_provider_id(&request.tts_provider)?,
            request.model,
            storage::now(),
            narrator_profile_id
        ],
    )?;
    storage::snapshot(&root)
}

#[tauri::command]
fn delete_provider_api_key(provider: String) -> StudioResult<ProviderSecretStatus> {
    let provider = normalize_provider_id(&provider)?;
    delete_provider_secret(&provider)?;
    Ok(ProviderSecretStatus {
        provider,
        exists: false,
    })
}

/* ----------
 * 供应商密钥存储：配置目录下的混淆文件（XOR + base64，防平凡扫描，非强加密）。
 * 全部项目共用一套密钥；XOR 的 salt 是 salt 字段自身的原始字符串字节，
 * 写入与读取必须使用同一口径。
 * ---------- */

const PROVIDER_KEYS_FILE: &str = "provider-keys.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct ProviderKeyFile {
    /// base64 随机盐；首写时生成
    salt: String,
    /// provider -> base64(xor(key, salt 原始字符串字节))
    #[serde(flatten)]
    keys: std::collections::HashMap<String, String>,
}

fn provider_keys_path() -> StudioResult<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| err("无法确定用户主目录"))?;
    Ok(PathBuf::from(home)
        .join("Library/Application Support/com.xiic.voice-studio")
        .join(PROVIDER_KEYS_FILE))
}

fn load_provider_keys() -> ProviderKeyFile {
    provider_keys_path()
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn store_provider_keys(file: &ProviderKeyFile) -> StudioResult<()> {
    let path = provider_keys_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(file)?)?;
    Ok(())
}

fn xor_with_salt(data: &[u8], salt: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(index, byte)| byte ^ salt[index % salt.len()])
        .collect()
}

fn read_provider_secret(provider: &str) -> Option<String> {
    let file = load_provider_keys();
    let salt = file.salt.clone().into_bytes();
    if let Some(encoded) = file.keys.get(provider) {
        if let Ok(decoded) = BASE64_ENGINE.decode(encoded) {
            if let Ok(key) = String::from_utf8(xor_with_salt(&decoded, &salt)) {
                if !key.trim().is_empty() {
                    return Some(key);
                }
            }
        }
    }
    None
}

fn write_provider_secret(provider: &str, key: &str) -> StudioResult<()> {
    let mut file = load_provider_keys();
    if file.salt.is_empty() {
        file.salt = BASE64_ENGINE.encode(Uuid::new_v4().as_bytes());
    }
    let encoded = BASE64_ENGINE.encode(xor_with_salt(key.as_bytes(), file.salt.as_bytes()));
    file.keys.insert(provider.to_string(), encoded);
    store_provider_keys(&file)?;
    Ok(())
}

fn delete_provider_secret(provider: &str) -> StudioResult<()> {
    let mut file = load_provider_keys();
    if file.keys.remove(provider).is_some() {
        store_provider_keys(&file)?;
    }
    Ok(())
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
        settings.api_key = read_provider_secret(&provider);
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
        settings.api_key = read_provider_secret(LLM_KEYCHAIN_ID);
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
            get_last_project_root,
            import_source,
            list_chapter_rules,
            preview_chapter_split,
            detect_chapter_rule,
            list_chapters,
            delete_chapter,
            mark_chapter,
            list_segments,
            update_segment,
            delete_segment,
            restore_segment,
            split_segment,
            merge_segments,
            insert_segment,
            list_characters,
            create_character,
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
            delete_job,
            clear_finished_jobs,
            play_segment_audio,
            voice_asset_audio_path,
            get_audio_output_directory,
            review_segment_audio,
            upload_segment_audio,
            save_segment_recording,
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
            delete_provider_api_key,
            bind_character_voice,
            character_voice_context,
            generate_voice_description,
            generate_character_voice_sample,
            finalize_character_voice,
            set_character_voice,
            set_narrator_voice
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}
