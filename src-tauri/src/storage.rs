use crate::domain::*;
use crate::error::{err, StudioResult};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const MANIFEST_FILE: &str = "project.json";
pub const DATABASE_FILE: &str = "studio.sqlite";

pub fn now() -> String {
    Utc::now().to_rfc3339()
}

pub fn ensure_project_dirs(root: &Path) -> StudioResult<()> {
    fs::create_dir_all(root)?;
    fs::create_dir_all(root.join("assets/audio"))?;
    fs::create_dir_all(root.join("assets/source"))?;
    fs::create_dir_all(root.join("assets/source/voices"))?;
    fs::create_dir_all(root.join("exports"))?;
    fs::create_dir_all(root.join("cache/providers"))?;
    Ok(())
}

pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(MANIFEST_FILE)
}

pub fn database_path(root: &Path) -> PathBuf {
    root.join(DATABASE_FILE)
}

pub fn load_manifest(root: &Path) -> StudioResult<ProjectManifest> {
    let text = fs::read_to_string(manifest_path(root))?;
    Ok(serde_json::from_str(&text)?)
}

pub fn save_manifest(root: &Path, manifest: &ProjectManifest) -> StudioResult<()> {
    fs::write(manifest_path(root), serde_json::to_string_pretty(manifest)?)?;
    Ok(())
}

pub fn open_connection(root: &Path) -> StudioResult<Connection> {
    let conn = Connection::open(database_path(root))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn migrate(conn: &Connection) -> StudioResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            author TEXT,
            language TEXT NOT NULL,
            production_type TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS chapters (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            order_index INTEGER NOT NULL,
            raw_text TEXT NOT NULL,
            script_status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS scenes (
            id TEXT PRIMARY KEY,
            chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
            order_index INTEGER NOT NULL,
            description TEXT NOT NULL,
            time_location TEXT,
            mood TEXT
        );
        CREATE TABLE IF NOT EXISTS characters (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            canonical_name TEXT NOT NULL,
            gender TEXT,
            age_timeline TEXT,
            notes TEXT,
            default_color TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS character_aliases (
            id TEXT PRIMARY KEY,
            character_id TEXT NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
            alias TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS voice_assets (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            asset_type TEXT NOT NULL,
            provider TEXT NOT NULL,
            model TEXT NOT NULL,
            relative_path TEXT NOT NULL,
            mime_type TEXT NOT NULL,
            source_file_name TEXT NOT NULL,
            consent_confirmed INTEGER NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS voice_profiles (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            character_id TEXT REFERENCES characters(id) ON DELETE SET NULL,
            name TEXT NOT NULL,
            age_stage TEXT NOT NULL,
            tts_provider TEXT NOT NULL,
            model TEXT,
            voice_id TEXT NOT NULL,
            voice_asset_id TEXT REFERENCES voice_assets(id) ON DELETE SET NULL,
            speed REAL NOT NULL,
            pitch REAL NOT NULL,
            style TEXT
        );
        CREATE TABLE IF NOT EXISTS segments (
            id TEXT PRIMARY KEY,
            chapter_id TEXT NOT NULL REFERENCES chapters(id) ON DELETE CASCADE,
            scene_id TEXT REFERENCES scenes(id) ON DELETE SET NULL,
            order_index INTEGER NOT NULL,
            text TEXT NOT NULL,
            segment_type TEXT NOT NULL,
            speaker TEXT,
            character_id TEXT REFERENCES characters(id) ON DELETE SET NULL,
            emotion TEXT,
            sound_cue TEXT,
            anchor TEXT,
            voice_profile_id TEXT REFERENCES voice_profiles(id) ON DELETE SET NULL,
            audio_status TEXT NOT NULL,
            review_status TEXT NOT NULL,
            age_progress REAL,
            is_manual_edit INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS voice_batches (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            scope TEXT NOT NULL,
            status TEXT NOT NULL,
            parameters_json TEXT NOT NULL,
            context_window INTEGER NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS segment_audio (
            id TEXT PRIMARY KEY,
            segment_id TEXT NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
            relative_path TEXT NOT NULL,
            duration_ms INTEGER,
            loudness_lufs REAL,
            version INTEGER NOT NULL,
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS review_issues (
            id TEXT PRIMARY KEY,
            segment_id TEXT REFERENCES segments(id) ON DELETE SET NULL,
            audio_id TEXT REFERENCES segment_audio(id) ON DELETE SET NULL,
            issue_type TEXT NOT NULL,
            note TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS export_jobs (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            target_format TEXT NOT NULL,
            selected_chapters_json TEXT NOT NULL,
            audio_mix_settings_json TEXT NOT NULL,
            status TEXT NOT NULL,
            output_path TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS jobs (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            job_type TEXT NOT NULL,
            status TEXT NOT NULL,
            progress REAL NOT NULL,
            payload_json TEXT NOT NULL,
            error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_cache (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            provider TEXT NOT NULL,
            cache_key TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (1, ?1)",
        params![now()],
    )?;
    ensure_column(conn, "voice_profiles", "model", "TEXT")?;
    ensure_column(conn, "voice_profiles", "voice_asset_id", "TEXT")?;
    ensure_column(conn, "voice_profiles", "created_at", "TEXT")?;
    ensure_column(conn, "voice_profiles", "updated_at", "TEXT")?;
    Ok(())
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> StudioResult<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|existing| existing == column) {
        conn.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
    Ok(())
}

pub fn create_project(
    root: &Path,
    title: &str,
    author: Option<String>,
) -> StudioResult<ProjectSummary> {
    if title.trim().is_empty() {
        return Err(err("项目名称不能为空"));
    }
    if root.join(MANIFEST_FILE).exists() || root.join(DATABASE_FILE).exists() {
        return Err(err(
            "目标文件夹已经包含 Xiic Voice Studio 项目，请改用打开项目",
        ));
    }
    ensure_project_dirs(root)?;
    let created_at = now();
    let manifest = ProjectManifest {
        id: Uuid::new_v4().to_string(),
        title: title.to_string(),
        author,
        language: "zh-CN".to_string(),
        production_type: "audiobook_drama".to_string(),
        schema_version: SCHEMA_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: created_at.clone(),
        updated_at: created_at.clone(),
    };
    save_manifest(root, &manifest)?;
    let conn = open_connection(root)?;
    conn.execute(
        "INSERT OR REPLACE INTO projects (id, title, author, language, production_type, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?7)",
        params![
            manifest.id,
            manifest.title,
            manifest.author,
            manifest.language,
            manifest.production_type,
            manifest.created_at,
            manifest.updated_at
        ],
    )?;
    project_summary(root)
}

pub fn project_summary(root: &Path) -> StudioResult<ProjectSummary> {
    let manifest = load_manifest(root)?;
    let conn = open_connection(root)?;
    let chapter_count: i64 = conn.query_row("SELECT COUNT(*) FROM chapters", [], |r| r.get(0))?;
    let segment_count: i64 = conn.query_row("SELECT COUNT(*) FROM segments", [], |r| r.get(0))?;
    let character_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM characters", [], |r| r.get(0))?;
    Ok(ProjectSummary {
        manifest,
        root_path: root.to_string_lossy().to_string(),
        chapter_count,
        segment_count,
        character_count,
    })
}

pub fn ensure_project_loaded(root: &Path) -> StudioResult<(ProjectManifest, Connection)> {
    ensure_project_dirs(root)?;
    let manifest = load_manifest(root)?;
    let conn = open_connection(root)?;
    Ok((manifest, conn))
}

pub fn list_chapters(conn: &Connection) -> StudioResult<Vec<Chapter>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, title, order_index, raw_text, script_status, created_at, updated_at
         FROM chapters ORDER BY order_index",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Chapter {
            id: row.get(0)?,
            project_id: row.get(1)?,
            title: row.get(2)?,
            order_index: row.get(3)?,
            raw_text: row.get(4)?,
            script_status: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;
    collect_rows(rows)
}

pub fn delete_chapter(conn: &Connection, chapter_id: &str) -> StudioResult<()> {
    let deleted = conn.execute("DELETE FROM chapters WHERE id = ?1", params![chapter_id])?;
    if deleted == 0 {
        return Err(crate::error::err("章节不存在或已被删除"));
    }
    Ok(())
}

pub fn list_segments(conn: &Connection, chapter_id: Option<&str>) -> StudioResult<Vec<Segment>> {
    let sql_all = "SELECT s.id, s.chapter_id, s.scene_id, s.order_index, s.text, s.segment_type, s.speaker, s.character_id, s.emotion, s.sound_cue, s.anchor, s.voice_profile_id, s.audio_status, s.review_status, s.age_progress, s.is_manual_edit, s.created_at, s.updated_at
                   FROM segments s JOIN chapters c ON c.id = s.chapter_id
                   ORDER BY c.order_index, s.order_index, s.id";
    let sql_chapter = "SELECT s.id, s.chapter_id, s.scene_id, s.order_index, s.text, s.segment_type, s.speaker, s.character_id, s.emotion, s.sound_cue, s.anchor, s.voice_profile_id, s.audio_status, s.review_status, s.age_progress, s.is_manual_edit, s.created_at, s.updated_at
                      FROM segments s JOIN chapters c ON c.id = s.chapter_id
                      WHERE s.chapter_id = ?1 ORDER BY s.order_index, s.id";
    let mut stmt = conn.prepare(if chapter_id.is_some() {
        sql_chapter
    } else {
        sql_all
    })?;
    let mapper = |row: &rusqlite::Row<'_>| {
        let segment_type: String = row.get(5)?;
        let is_manual_edit: i64 = row.get(15)?;
        Ok(Segment {
            id: row.get(0)?,
            chapter_id: row.get(1)?,
            scene_id: row.get(2)?,
            order_index: row.get(3)?,
            text: row.get(4)?,
            segment_type: SegmentType::from(segment_type.as_str()),
            speaker: row.get(6)?,
            character_id: row.get(7)?,
            emotion: row.get(8)?,
            sound_cue: row.get(9)?,
            anchor: row.get(10)?,
            voice_profile_id: row.get(11)?,
            audio_status: row.get(12)?,
            review_status: row.get(13)?,
            age_progress: row.get(14)?,
            is_manual_edit: is_manual_edit != 0,
            created_at: row.get(16)?,
            updated_at: row.get(17)?,
        })
    };
    if let Some(chapter_id) = chapter_id {
        collect_rows(stmt.query_map(params![chapter_id], mapper)?)
    } else {
        collect_rows(stmt.query_map([], mapper)?)
    }
}

pub fn list_characters(conn: &Connection) -> StudioResult<Vec<Character>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, canonical_name, gender, age_timeline, notes, default_color
         FROM characters ORDER BY canonical_name",
    )?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let aliases = aliases_for(conn, &id).unwrap_or_default();
        Ok(Character {
            id,
            project_id: row.get(1)?,
            canonical_name: row.get(2)?,
            aliases,
            gender: row.get(3)?,
            age_timeline: row.get(4)?,
            notes: row.get(5)?,
            default_color: row.get(6)?,
        })
    })?;
    collect_rows(rows)
}

/// 把声音档案绑定到角色（character_id 为 None 表示解绑）。
pub fn bind_voice_profile(
    conn: &Connection,
    profile_id: &str,
    character_id: Option<&str>,
) -> StudioResult<()> {
    let changed = conn.execute(
        "UPDATE voice_profiles SET character_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![character_id, now(), profile_id],
    )?;
    if changed == 0 {
        return Err(err("声音档案不存在，无法绑定"));
    }
    Ok(())
}

pub fn list_voice_profiles(conn: &Connection) -> StudioResult<Vec<VoiceProfile>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, character_id, name, age_stage, tts_provider, model, voice_id, voice_asset_id, speed, pitch, style
         FROM voice_profiles ORDER BY name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(VoiceProfile {
            id: row.get(0)?,
            project_id: row.get(1)?,
            character_id: row.get(2)?,
            name: row.get(3)?,
            age_stage: row.get(4)?,
            tts_provider: row.get(5)?,
            model: row.get(6)?,
            voice_id: row.get(7)?,
            voice_asset_id: row.get(8)?,
            speed: row.get(9)?,
            pitch: row.get(10)?,
            style: row.get(11)?,
        })
    })?;
    collect_rows(rows)
}

pub fn list_voice_assets(conn: &Connection) -> StudioResult<Vec<VoiceAsset>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, name, asset_type, provider, model, relative_path, mime_type,
                source_file_name, consent_confirmed, status, created_at, updated_at
         FROM voice_assets ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(VoiceAsset {
            id: row.get(0)?,
            project_id: row.get(1)?,
            name: row.get(2)?,
            asset_type: row.get(3)?,
            provider: row.get(4)?,
            model: row.get(5)?,
            relative_path: row.get(6)?,
            mime_type: row.get(7)?,
            source_file_name: row.get(8)?,
            consent_confirmed: row.get(9)?,
            status: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    })?;
    collect_rows(rows)
}

pub fn list_review_issues(conn: &Connection) -> StudioResult<Vec<ReviewIssue>> {
    let mut stmt = conn.prepare(
        "SELECT id, segment_id, audio_id, issue_type, note, status, created_at
         FROM review_issues ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ReviewIssue {
            id: row.get(0)?,
            segment_id: row.get(1)?,
            audio_id: row.get(2)?,
            issue_type: row.get(3)?,
            note: row.get(4)?,
            status: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    collect_rows(rows)
}

pub fn list_jobs(conn: &Connection) -> StudioResult<Vec<StudioJob>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, job_type, status, progress, payload_json, error, created_at, updated_at
         FROM jobs ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(StudioJob {
            id: row.get(0)?,
            project_id: row.get(1)?,
            job_type: row.get(2)?,
            status: row.get(3)?,
            progress: row.get(4)?,
            payload_json: row.get(5)?,
            error: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
        })
    })?;
    collect_rows(rows)
}

pub fn delete_job(conn: &Connection, job_id: &str) -> StudioResult<()> {
    conn.execute("DELETE FROM jobs WHERE id = ?1", params![job_id])?;
    Ok(())
}



pub fn clear_finished_jobs(conn: &Connection) -> StudioResult<usize> {
    let removed = conn.execute(
        "DELETE FROM jobs WHERE status IN ('failed', 'canceled', 'succeeded')",
        [],
    )?;
    Ok(removed)
}

/// 删除分段；返回其音频文件相对路径，供调用方清理磁盘文件。
/// segment_audio 随外键级联删除；审听问题对分段是 SET NULL，显式清掉避免留下无主记录。
pub fn delete_segment(conn: &Connection, segment_id: &str) -> StudioResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT relative_path FROM segment_audio WHERE segment_id = ?1")?;
    let paths = stmt
        .query_map(params![segment_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    conn.execute("DELETE FROM review_issues WHERE segment_id = ?1", params![segment_id])?;
    conn.execute("DELETE FROM segments WHERE id = ?1", params![segment_id])?;
    Ok(paths)
}

pub fn snapshot(root: &Path) -> StudioResult<StudioSnapshot> {
    let project = project_summary(root)?;
    let conn = open_connection(root)?;
    Ok(StudioSnapshot {
        project,
        chapters: list_chapters(&conn)?,
        segments: list_segments(&conn, None)?,
        characters: list_characters(&conn)?,
        voice_profiles: list_voice_profiles(&conn)?,
        voice_assets: list_voice_assets(&conn)?,
        review_issues: list_review_issues(&conn)?,
        jobs: list_jobs(&conn)?,
    })
}

pub fn insert_job(
    conn: &Connection,
    project_id: &str,
    job_type: &str,
    payload_json: &str,
) -> StudioResult<StudioJob> {
    let timestamp = now();
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO jobs (id, project_id, job_type, status, progress, payload_json, error, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'pending', 0, ?4, NULL, ?5, ?6)",
        params![id, project_id, job_type, payload_json, timestamp, timestamp],
    )?;
    let jobs = list_jobs(conn)?;
    jobs.into_iter()
        .find(|job| job.id == id)
        .ok_or_else(|| err("创建任务后读取失败"))
}

pub fn mark_job(
    conn: &Connection,
    job_id: &str,
    status: &str,
    progress: f64,
    error: Option<&str>,
) -> StudioResult<()> {
    conn.execute(
        "UPDATE jobs SET status = ?1, progress = ?2, error = ?3, updated_at = ?4 WHERE id = ?5",
        params![status, progress, error, now(), job_id],
    )?;
    Ok(())
}

pub fn start_job(conn: &Connection, job_id: &str) -> StudioResult<bool> {
    let changed = conn.execute(
        "UPDATE jobs
         SET status = 'running', progress = 0.05, error = NULL, updated_at = ?1
         WHERE id = ?2 AND status = 'pending'",
        params![now(), job_id],
    )?;
    Ok(changed == 1)
}

pub fn recover_incomplete_jobs(conn: &Connection) -> StudioResult<usize> {
    let recovered = conn.execute(
        "UPDATE jobs
         SET status = 'failed',
             progress = 0.0,
             error = '应用上次退出时任务未完成，请重试',
             updated_at = ?1
         WHERE status IN ('pending', 'running')",
        params![now()],
    )?;
    Ok(recovered)
}

pub fn latest_audio_for_segment(
    conn: &Connection,
    segment_id: &str,
) -> StudioResult<Option<SegmentAudio>> {
    conn.query_row(
        "SELECT id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at
         FROM segment_audio
         WHERE segment_id = ?1 AND status IN ('approved', 'generated', 'uploaded')
         ORDER BY CASE status WHEN 'approved' THEN 0 ELSE 1 END, version DESC
         LIMIT 1",
        params![segment_id],
        |row| {
            Ok(SegmentAudio {
                id: row.get(0)?,
                segment_id: row.get(1)?,
                relative_path: row.get(2)?,
                duration_ms: row.get(3)?,
                loudness_lufs: row.get(4)?,
                version: row.get(5)?,
                source: row.get(6)?,
                status: row.get(7)?,
                created_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn newest_audio_for_segment(
    conn: &Connection,
    segment_id: &str,
) -> StudioResult<Option<SegmentAudio>> {
    conn.query_row(
        "SELECT id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at
         FROM segment_audio
         WHERE segment_id = ?1 AND status IN ('approved', 'generated', 'uploaded', 'rejected')
         ORDER BY version DESC
         LIMIT 1",
        params![segment_id],
        |row| {
            Ok(SegmentAudio {
                id: row.get(0)?,
                segment_id: row.get(1)?,
                relative_path: row.get(2)?,
                duration_ms: row.get(3)?,
                loudness_lufs: row.get(4)?,
                version: row.get(5)?,
                source: row.get(6)?,
                status: row.get(7)?,
                created_at: row.get(8)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn next_audio_version(conn: &Connection, segment_id: &str) -> StudioResult<i64> {
    let version: Option<i64> = conn
        .query_row(
            "SELECT MAX(version) FROM segment_audio WHERE segment_id = ?1",
            params![segment_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(version.unwrap_or(0) + 1)
}

fn aliases_for(conn: &Connection, character_id: &str) -> StudioResult<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT alias FROM character_aliases WHERE character_id = ?1 ORDER BY alias")?;
    let rows = stmt.query_map(params![character_id], |row| row.get(0))?;
    collect_rows(rows)
}

fn collect_rows<T, I>(rows: I) -> StudioResult<Vec<T>>
where
    I: IntoIterator<Item = rusqlite::Result<T>>,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
