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
            style TEXT,
            is_default INTEGER NOT NULL DEFAULT 0
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
        -- 删除分段的归档。用户点「删除」时整行 + 关联数据序列化存这里，
        -- 让误触能原样撤销（连磁盘音频都还在），超过保留期才真正清理。
        -- 刻意不建指向 chapters / segments 的外键：归档行的 id 就是被删分段的 id，
        -- 建 FK 反而会在删除那一刻被自己绊住。
        CREATE TABLE IF NOT EXISTS deleted_segments (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            chapter_id TEXT NOT NULL,
            position_index INTEGER NOT NULL,
            text_preview TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            audio_json TEXT NOT NULL,
            issues_json TEXT NOT NULL,
            deleted_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_deleted_segments_chapter
            ON deleted_segments(chapter_id, deleted_at);
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
    // 1 = 系统自动兜底的默认音色（尚未由人确认）；0 = 人定过的音色。
    // 用来保证自动档不会顶掉用户选的音色，并让 UI 能标出"还没定音色"的角色。
    ensure_column(conn, "voice_profiles", "is_default", "INTEGER NOT NULL DEFAULT 0")?;
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
    // 旁白是每个项目的必备说话人，开箱就给一条默认音色档案。
    // 否则第一次「生成」会被生成前的音色校验拦下，让用户先撞一次墙。
    crate::tts::ensure_default_narrator_profile(&conn, &manifest.id)?;
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

const SEGMENT_COLUMNS: &str = "s.id, s.chapter_id, s.scene_id, s.order_index, s.text, s.segment_type, s.speaker, s.character_id, s.emotion, s.sound_cue, s.anchor, s.voice_profile_id, s.audio_status, s.review_status, s.age_progress, s.is_manual_edit, s.created_at, s.updated_at";

fn segment_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Segment> {
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
}

pub fn list_segments(conn: &Connection, chapter_id: Option<&str>) -> StudioResult<Vec<Segment>> {
    let sql_all = format!(
        "SELECT {SEGMENT_COLUMNS}
         FROM segments s JOIN chapters c ON c.id = s.chapter_id
         ORDER BY c.order_index, s.order_index, s.id"
    );
    let sql_chapter = format!(
        "SELECT {SEGMENT_COLUMNS}
         FROM segments s JOIN chapters c ON c.id = s.chapter_id
         WHERE s.chapter_id = ?1 ORDER BY s.order_index, s.id"
    );
    let mut stmt = conn.prepare(if chapter_id.is_some() {
        sql_chapter.as_str()
    } else {
        sql_all.as_str()
    })?;
    if let Some(chapter_id) = chapter_id {
        collect_rows(stmt.query_map(params![chapter_id], segment_from_row)?)
    } else {
        collect_rows(stmt.query_map([], segment_from_row)?)
    }
}

/// 只取「该角色自己的分段」+「正文提到该角色（含别名）的分段」，
/// 供音色描述的章节上下文装配使用——避免为了几条样本把整本书的分段全读出来。
pub fn list_segments_for_voice_context(
    conn: &Connection,
    character_id: &str,
    keywords: &[String],
) -> StudioResult<Vec<Segment>> {
    let mut sql = format!(
        "SELECT {SEGMENT_COLUMNS} FROM segments s JOIN chapters c ON c.id = s.chapter_id WHERE s.character_id = ?1"
    );
    for index in 0..keywords.len() {
        sql.push_str(&format!(" OR s.text LIKE ?{}", index + 2));
    }
    // 上限只为兜住"出场几千次的主角"，装配侧另有 16 条上限。
    sql.push_str(" ORDER BY c.order_index, s.order_index, s.id LIMIT 600");
    let mut stmt = conn.prepare(&sql)?;
    let mut values: Vec<String> = vec![character_id.to_string()];
    values.extend(
        keywords
            .iter()
            .map(|keyword| keyword.trim())
            .filter(|keyword| !keyword.is_empty())
            .map(|keyword| format!("%{keyword}%")),
    );
    let rows = stmt.query_map(
        rusqlite::params_from_iter(values.iter().map(|value| value.as_str())),
        segment_from_row,
    )?;
    collect_rows(rows)
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

/// 手动新建角色，返回新角色 id。
///
/// 两道闸都在这里挡：**重名**与**别名撞车** —— 两者都会毁掉归属关系
/// （两个同名角色在界面上无从分辨；别名共用会把分段并到错的人名下）。
/// 建完补默认音色档由调用方负责（`tts::ensure_default_voice_profiles`）。
pub fn create_character(
    conn: &mut Connection,
    project_id: &str,
    name: &str,
    aliases: &[String],
    gender: Option<&str>,
    age_timeline: Option<&str>,
    notes: Option<&str>,
) -> StudioResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(err("角色名不能为空"));
    }
    let duplicate = conn
        .query_row(
            "SELECT canonical_name FROM characters WHERE project_id = ?1 AND canonical_name = ?2",
            params![project_id, name],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if duplicate.is_some() {
        return Err(err(format!(
            "已经有叫「{name}」的角色了 —— 换一个名字，或直接编辑现有那个"
        )));
    }
    let mut accepted: Vec<String> = Vec::new();
    for alias in aliases {
        let alias = alias.trim();
        if alias.is_empty() || alias == name || accepted.iter().any(|value| value == alias) {
            continue;
        }
        let owner = conn
            .query_row(
                "SELECT c.canonical_name FROM characters c
                 WHERE c.project_id = ?1
                   AND (c.canonical_name = ?2
                        OR EXISTS (SELECT 1 FROM character_aliases a
                                   WHERE a.character_id = c.id AND a.alias = ?2))",
                params![project_id, alias],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(owner) = owner {
            return Err(err(format!("「{alias}」已经是「{owner}」的名字或别名，换一个")));
        }
        accepted.push(alias.to_string());
    }
    let character_id = Uuid::new_v4().to_string();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO characters (id, project_id, canonical_name, gender, age_timeline, notes, default_color)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            character_id,
            project_id,
            name,
            gender,
            age_timeline,
            notes,
            crate::ai::pick_character_color(name)
        ],
    )?;
    for alias in accepted {
        tx.execute(
            "INSERT INTO character_aliases (id, character_id, alias) VALUES (?1, ?2, ?3)",
            params![Uuid::new_v4().to_string(), character_id, alias],
        )?;
    }
    tx.commit()?;
    Ok(character_id)
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
        "SELECT id, project_id, character_id, name, age_stage, tts_provider, model, voice_id, voice_asset_id, speed, pitch, style, is_default
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
            is_default: row.get::<_, i64>(12)? == 1,
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

/// 解析音色资产样本的绝对路径。
/// 返回 None 有两种来源：资产不属于本项目、或文件已被外部删掉。
/// 对播放来说两者等价，所以不区分——但必须让调用方拿到 None 而不是一个播不响的路径。
pub fn voice_asset_audio_path(
    conn: &Connection,
    root: &Path,
    project_id: &str,
    asset_id: &str,
) -> StudioResult<Option<String>> {
    let relative_path: Option<String> = conn
        .query_row(
            "SELECT relative_path FROM voice_assets WHERE id = ?1 AND project_id = ?2",
            params![asset_id, project_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(relative_path
        .map(|relative| root.join(relative))
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string()))
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
const SEGMENT_AUDIO_SELECT: &str = "SELECT id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at
     FROM segment_audio WHERE segment_id = ?1 ORDER BY version, id";

const REVIEW_ISSUE_SELECT: &str =
    "SELECT id, segment_id, audio_id, issue_type, note, status, created_at
     FROM review_issues WHERE segment_id = ?1 ORDER BY created_at, id";

fn segment_audio_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SegmentAudio> {
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
}

fn review_issue_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewIssue> {
    Ok(ReviewIssue {
        id: row.get(0)?,
        segment_id: row.get(1)?,
        audio_id: row.get(2)?,
        issue_type: row.get(3)?,
        note: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
    })
}

/// 把一条按 `segment_id` 过滤的查询结果整段序列化成 JSON，用于归档。
fn archive_json<T, F>(conn: &Connection, sql: &str, segment_id: &str, map: F) -> StudioResult<String>
where
    T: serde::Serialize,
    F: Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![segment_id], map)?;
    let items = rows.collect::<Result<Vec<T>, _>>()?;
    Ok(serde_json::to_string(&items)?)
}

/// 归档列表里显示的片段预览。按字符截，别按字节——中文会被切碎。
fn text_preview(text: &str) -> String {
    let trimmed = text.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 40 {
        trimmed.to_string()
    } else {
        format!("{}…", chars[..40].iter().collect::<String>())
    }
}

/// 真正把分段从库里抹掉：读走音频路径 → 删审听备注 → 删分段行
/// （`segment_audio` 由外键 `ON DELETE CASCADE` 一并删除）。
/// 返回音频相对路径，由调用方决定是否删除磁盘文件。
fn purge_segment_row(conn: &Connection, segment_id: &str) -> StudioResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT relative_path FROM segment_audio WHERE segment_id = ?1")?;
    let paths = stmt
        .query_map(params![segment_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    conn.execute("DELETE FROM review_issues WHERE segment_id = ?1", params![segment_id])?;
    conn.execute("DELETE FROM segments WHERE id = ?1", params![segment_id])?;
    Ok(paths)
}

/// 删除分段——**归档式，可撤销**。
///
/// 删除是不可逆操作的入口，但误触的概率远高于真心要删，而这里删掉的东西
/// 有文本、有审听备注、还有用户花钱生成的音频，代价不对称。所以不做物理删除：
/// 把分段整行 + 音频记录 + 审听备注序列化进 `deleted_segments`，原行才从
/// `segments` 表移除。这样：
///   - 现有全部查询照旧（行确实不在了），25 处 SQL 一处都不用补过滤条件；
///   - 撤销 = 反序列化插回原位置，零信息损失；
///   - **磁盘音频保留**，恢复后立刻能播。
/// 真正的清理交给 [`purge_expired_deleted_segments`]。
pub fn delete_segment(
    conn: &Connection,
    project_id: &str,
    segment_id: &str,
) -> StudioResult<DeletedSegment> {
    let segment = get_segment(conn, segment_id)?;
    let audio_json = archive_json(conn, SEGMENT_AUDIO_SELECT, segment_id, segment_audio_from_row)?;
    let issues_json = archive_json(conn, REVIEW_ISSUE_SELECT, segment_id, review_issue_from_row)?;
    let deleted_at = now();
    let preview = text_preview(&segment.text);

    conn.execute(
        "INSERT OR REPLACE INTO deleted_segments
           (id, project_id, chapter_id, position_index, text_preview, payload_json, audio_json, issues_json, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            segment.id,
            project_id,
            segment.chapter_id,
            segment.order_index,
            preview,
            serde_json::to_string(&segment)?,
            audio_json,
            issues_json,
            deleted_at
        ],
    )?;
    // 先落归档再删行。反过来的话，中途失败就真丢了。
    purge_segment_row(conn, segment_id)?;

    Ok(DeletedSegment {
        id: segment.id,
        project_id: project_id.to_string(),
        chapter_id: segment.chapter_id,
        position_index: segment.order_index,
        text_preview: preview,
        deleted_at,
    })
}

/// 恢复：归档里的引用在这期间可能已经失效（角色被删、音色档被换），
/// 直接插回去会被外键拦下。所以逐个校验，失效的降级成 NULL——
/// 分段文本和音频比"当时配的哪个角色"重要得多，不能因为角色没了就拒绝恢复。
fn keep_if_exists(
    conn: &Connection,
    table: &str,
    value: Option<String>,
) -> StudioResult<Option<String>> {
    let Some(id) = value else { return Ok(None) };
    let exists = conn
        .query_row(
            &format!("SELECT 1 FROM {table} WHERE id = ?1"),
            params![id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    Ok(if exists { Some(id) } else { None })
}

pub fn restore_segment(conn: &Connection, deleted_id: &str) -> StudioResult<Segment> {
    let (chapter_id, position_index, payload_json, audio_json, issues_json): (
        String,
        i64,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT chapter_id, position_index, payload_json, audio_json, issues_json
             FROM deleted_segments WHERE id = ?1",
            params![deleted_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| err("这条删除记录已经不在了（超过保留期会被自动清理）"))?;

    // 章节没了就没有归属，恢复不进去，直说
    let chapter_exists = conn
        .query_row(
            "SELECT 1 FROM chapters WHERE id = ?1",
            params![chapter_id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if !chapter_exists {
        return Err(err("原章节已被删除，这个分段无法恢复"));
    }

    let mut segment: Segment = serde_json::from_str(&payload_json)?;
    segment.chapter_id = chapter_id.clone();
    segment.scene_id = keep_if_exists(conn, "scenes", segment.scene_id)?;
    segment.character_id = keep_if_exists(conn, "characters", segment.character_id)?;
    segment.voice_profile_id =
        keep_if_exists(conn, "voice_profiles", segment.voice_profile_id)?;
    segment.updated_at = now();

    // 插回原位：原序号空着（删除时没动过别人的序号）就直接落回；
    // 若这期间有人在同序号插了新段，就让位——把 >= 的整段后移一格。
    let occupied: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE chapter_id = ?1 AND order_index = ?2",
        params![chapter_id, position_index],
        |row| row.get(0),
    )?;
    if occupied > 0 {
        conn.execute(
            "UPDATE segments SET order_index = order_index + 1, updated_at = ?1
             WHERE chapter_id = ?2 AND order_index >= ?3",
            params![segment.updated_at, chapter_id, position_index],
        )?;
    }
    segment.order_index = position_index;

    conn.execute(
        "INSERT INTO segments (id, chapter_id, scene_id, order_index, text, segment_type, speaker, character_id, emotion, sound_cue, anchor, voice_profile_id, audio_status, review_status, age_progress, is_manual_edit, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            segment.id,
            segment.chapter_id,
            segment.scene_id,
            segment.order_index,
            segment.text,
            segment.segment_type.as_str(),
            segment.speaker,
            segment.character_id,
            segment.emotion,
            segment.sound_cue,
            segment.anchor,
            segment.voice_profile_id,
            segment.audio_status,
            segment.review_status,
            segment.age_progress,
            segment.is_manual_edit as i64,
            segment.created_at,
            segment.updated_at
        ],
    )?;

    // 音频记录跟着回来。磁盘文件当时没删，所以恢复后直接能播。
    let audios: Vec<SegmentAudio> = serde_json::from_str(&audio_json)?;
    for audio in &audios {
        conn.execute(
            "INSERT OR REPLACE INTO segment_audio
               (id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                audio.id,
                segment.id,
                audio.relative_path,
                audio.duration_ms,
                audio.loudness_lufs,
                audio.version,
                audio.source,
                audio.status,
                audio.created_at
            ],
        )?;
    }

    let issues: Vec<ReviewIssue> = serde_json::from_str(&issues_json)?;
    for issue in &issues {
        conn.execute(
            "INSERT OR REPLACE INTO review_issues
               (id, segment_id, audio_id, issue_type, note, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                issue.id,
                segment.id,
                issue.audio_id,
                issue.issue_type,
                issue.note,
                issue.status,
                issue.created_at
            ],
        )?;
    }

    conn.execute("DELETE FROM deleted_segments WHERE id = ?1", params![deleted_id])?;
    Ok(segment)
}

pub fn list_deleted_segments(
    conn: &Connection,
    project_id: &str,
) -> StudioResult<Vec<DeletedSegment>> {
    let mut stmt = conn.prepare(
        "SELECT id, project_id, chapter_id, position_index, text_preview, deleted_at
         FROM deleted_segments WHERE project_id = ?1 ORDER BY deleted_at DESC, id",
    )?;
    let rows = stmt.query_map(params![project_id], |row| {
        Ok(DeletedSegment {
            id: row.get(0)?,
            project_id: row.get(1)?,
            chapter_id: row.get(2)?,
            position_index: row.get(3)?,
            text_preview: row.get(4)?,
            deleted_at: row.get(5)?,
        })
    })?;
    collect_rows(rows)
}

/// 清理超过保留期的归档，返回**需要删除的磁盘音频相对路径**。
/// 删文件的动作留在调用方——这里没有项目根目录。
///
/// 保留期取天而不是分钟：撤销窗口是几十秒，但"过一天才想起来"同样常见，
/// 而磁盘上的音频是用户花钱生成的，多留几天的成本远低于重生成。
pub fn purge_expired_deleted_segments(
    conn: &Connection,
    retain_days: i64,
) -> StudioResult<Vec<String>> {
    let cutoff = (Utc::now() - chrono::Duration::days(retain_days)).to_rfc3339();
    let mut stmt = conn.prepare("SELECT audio_json FROM deleted_segments WHERE deleted_at < ?1")?;
    let payloads = stmt
        .query_map(params![cutoff], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let mut paths = Vec::new();
    for payload in payloads {
        // 归档读不出来就只清理这条记录，别让一条坏数据卡住整个清理
        let audios: Vec<SegmentAudio> = serde_json::from_str(&payload).unwrap_or_default();
        paths.extend(audios.into_iter().map(|audio| audio.relative_path));
    }

    conn.execute("DELETE FROM deleted_segments WHERE deleted_at < ?1", params![cutoff])?;
    Ok(paths)
}

/// 读取单个分段（按 id）。
pub fn get_segment(conn: &Connection, segment_id: &str) -> StudioResult<Segment> {
    let sql = "SELECT s.id, s.chapter_id, s.scene_id, s.order_index, s.text, s.segment_type, s.speaker, s.character_id, s.emotion, s.sound_cue, s.anchor, s.voice_profile_id, s.audio_status, s.review_status, s.age_progress, s.is_manual_edit, s.created_at, s.updated_at
               FROM segments s WHERE s.id = ?1";
    conn.query_row(sql, params![segment_id], |row| {
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
    })
    .map_err(|_| err("分段不存在"))
}

/// 重新按 order_index（同 id 兜底）连续编号整章分段，消除拆分/合并/插入造成的空洞。
pub fn renumber_chapter_segments(conn: &Connection, chapter_id: &str) -> StudioResult<()> {
    let ids: Vec<String> = conn
        .prepare("SELECT id FROM segments WHERE chapter_id = ?1 ORDER BY order_index, id")?
        .query_map(params![chapter_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for (index, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE segments SET order_index = ?1 WHERE id = ?2",
            params![index as i64, id],
        )?;
    }
    Ok(())
}

/// 在字符偏移处把分段拆成两句。左半沿用原段，右半新建；两端都标记人工编辑并失效音频。
pub fn split_segment_at(conn: &Connection, segment_id: &str, offset: usize) -> StudioResult<()> {
    let seg = get_segment(conn, segment_id)?;
    let chars: Vec<char> = seg.text.chars().collect();
    let split_at = offset.min(chars.len());
    let left: String = chars[..split_at].iter().collect();
    let right: String = chars[split_at..].iter().collect();
    if left.trim().is_empty() || right.trim().is_empty() {
        return Err(err("拆分点无效：拆分后至少有一侧没有内容"));
    }
    let timestamp = now();
    conn.execute(
        "UPDATE segments SET order_index = order_index + 1, updated_at = ?1 WHERE chapter_id = ?2 AND order_index > ?3",
        params![timestamp, seg.chapter_id, seg.order_index],
    )?;
    conn.execute(
        "UPDATE segments SET text = ?1, is_manual_edit = 1, audio_status = 'missing', review_status = 'unreviewed', updated_at = ?2 WHERE id = ?3",
        params![left, timestamp, segment_id],
    )?;
    crate::audio::invalidate_segment_audio(conn, segment_id, "分段已拆分，原音频仅对应前半，请重新生成或上传")?;
    let new_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO segments (id, chapter_id, scene_id, order_index, text, segment_type, speaker, character_id, emotion, sound_cue, anchor, voice_profile_id, audio_status, review_status, age_progress, is_manual_edit, created_at, updated_at)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, 'missing', 'unreviewed', NULL, 1, ?9, ?10)",
        params![
            new_id,
            seg.chapter_id,
            seg.order_index + 1,
            right,
            seg.segment_type.as_str(),
            seg.speaker,
            seg.character_id,
            // 右半继承左半的情绪：一句话拆成两句，表演提示通常不变；
            // 不给的话用户每拆一次都要重新填一遍情绪。
            seg.emotion,
            timestamp,
            timestamp,
        ],
    )?;
    renumber_chapter_segments(conn, &seg.chapter_id)?;
    Ok(())
}

/// 合并相邻分段（按当前顺序）。首段承接合并后的文本/类型/角色，其余被删除。
/// 返回被删分段的音频文件相对路径，供调用方清理磁盘。
pub fn merge_segments(conn: &Connection, ids: &[String]) -> StudioResult<Vec<String>> {
    if ids.len() < 2 {
        return Err(err("合并至少需要两个分段"));
    }
    let mut segs: Vec<Segment> = Vec::with_capacity(ids.len());
    for id in ids {
        segs.push(get_segment(conn, id)?);
    }
    segs.sort_by_key(|segment| segment.order_index);
    for window in segs.windows(2) {
        if window[0].chapter_id != window[1].chapter_id {
            return Err(err("合并的分段必须属于同一章节"));
        }
        if window[1].order_index - window[0].order_index != 1 {
            return Err(err("合并的分段必须相邻且连续"));
        }
    }
    let chapter_id = segs[0].chapter_id.clone();
    let merged_text = segs
        .iter()
        .map(|segment| segment.text.trim())
        .collect::<Vec<_>>()
        .join("\n");
    let preferred = segs
        .iter()
        .find(|segment| segment.segment_type != SegmentType::Narration)
        .unwrap_or(&segs[0]);
    let first = &segs[0];
    let timestamp = now();
    conn.execute(
        "UPDATE segments SET text = ?1, segment_type = ?2, speaker = ?3, character_id = ?4, is_manual_edit = 1, audio_status = 'missing', review_status = 'unreviewed', updated_at = ?5 WHERE id = ?6",
        params![
            merged_text,
            preferred.segment_type.as_str(),
            preferred.speaker,
            preferred.character_id,
            timestamp,
            first.id,
        ],
    )?;
    crate::audio::invalidate_segment_audio(conn, &first.id, "分段已合并，请重新生成或上传音频")?;
    let mut orphaned = Vec::new();
    for removed in segs.iter().skip(1) {
        // 合并走物理删除，不进归档：合并结果本身留着全部文本，
        // 而"因合并被吃掉的分段"混进撤销列表只会让人看不懂。
        orphaned.extend(purge_segment_row(conn, &removed.id)?);
    }
    renumber_chapter_segments(conn, &chapter_id)?;
    Ok(orphaned)
}

/// 插入新分段（用于补录漏掉的台词）。
///
/// `after_segment_id` 为 `None` 表示插到**章首**——空章节的「添加第一句」
/// 走的就是这条：那时候章里一个分段都没有，没有"某段之后"可以指。
pub fn insert_segment_after(
    conn: &Connection,
    chapter_id: &str,
    after_segment_id: Option<&str>,
    text: &str,
    segment_type: SegmentType,
    character_id: Option<String>,
    speaker: Option<String>,
) -> StudioResult<()> {
    // (从哪个序号开始腾位, 新段落在哪个序号)；None = 插到章首
    let (shift_from, new_index) = match after_segment_id {
        Some(id) => {
            let after = get_segment(conn, id)?;
            if after.chapter_id != chapter_id {
                return Err(err("插入位置与章节不匹配"));
            }
            (after.order_index + 1, after.order_index + 1)
        }
        None => (0, 0),
    };
    let timestamp = now();
    conn.execute(
        "UPDATE segments SET order_index = order_index + 1, updated_at = ?1 WHERE chapter_id = ?2 AND order_index >= ?3",
        params![timestamp, chapter_id, shift_from],
    )?;
    let new_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO segments (id, chapter_id, scene_id, order_index, text, segment_type, speaker, character_id, emotion, sound_cue, anchor, voice_profile_id, audio_status, review_status, age_progress, is_manual_edit, created_at, updated_at)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, NULL, 'missing', 'unreviewed', NULL, 1, ?8, ?9)",
        params![
            new_id,
            chapter_id,
            new_index,
            text,
            segment_type.as_str(),
            speaker,
            character_id,
            timestamp,
            timestamp,
        ],
    )?;
    renumber_chapter_segments(conn, chapter_id)?;
    Ok(())
}

pub fn snapshot(root: &Path) -> StudioResult<StudioSnapshot> {
    let project = project_summary(root)?;
    let conn = open_connection(root)?;
    let narrator_profile_id =
        crate::tts::narrator_profile_for_project(&conn, &project.manifest.id)?;
    // 先取好再 move：project 随后会被整体搬进快照
    let deleted_segments = list_deleted_segments(&conn, &project.manifest.id)?;
    Ok(StudioSnapshot {
        project,
        chapters: list_chapters(&conn)?,
        segments: list_segments(&conn, None)?,
        characters: list_characters(&conn)?,
        voice_profiles: list_voice_profiles(&conn)?,
        voice_assets: list_voice_assets(&conn)?,
        narrator_profile_id,
        review_issues: list_review_issues(&conn)?,
        jobs: list_jobs(&conn)?,
        deleted_segments,
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
