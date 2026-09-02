use crate::domain::SegmentType;
use crate::error::{err, StudioResult};
use crate::storage::now;
use quick_xml::events::Event;
use quick_xml::Reader;
use rusqlite::{params, Connection};
use std::fs;
use std::io::Read;
use std::path::Path;
use uuid::Uuid;
use zip::ZipArchive;

#[derive(Debug)]
pub struct ImportedSource {
    pub title: String,
    pub text: String,
}

pub fn read_source(path: &Path) -> StudioResult<ImportedSource> {
    let title = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("导入稿件")
        .to_string();
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let text = match ext.as_str() {
        "txt" => fs::read_to_string(path)?,
        "docx" => read_docx_text(path)?,
        _ => return Err(err("当前版本仅支持导入 TXT 和 DOCX 文件")),
    };
    if text.trim().is_empty() {
        return Err(err("稿件内容为空，无法导入"));
    }
    Ok(ImportedSource {
        title,
        text: normalize_text(&text),
    })
}

pub fn import_source(
    conn: &Connection,
    project_id: &str,
    source: &ImportedSource,
) -> StudioResult<Vec<String>> {
    let chapters = split_chapters(&source.title, &source.text);
    let mut chapter_ids = Vec::new();
    let existing_count: i64 = conn.query_row("SELECT COUNT(*) FROM chapters", [], |r| r.get(0))?;
    for (index, (title, text)) in chapters.into_iter().enumerate() {
        let chapter_id = Uuid::new_v4().to_string();
        let timestamp = now();
        conn.execute(
            "INSERT INTO chapters (id, project_id, title, order_index, raw_text, script_status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'imported', ?6, ?7)",
            params![
                chapter_id,
                project_id,
                title,
                existing_count + index as i64,
                text,
                timestamp,
                timestamp
            ],
        )?;
        chapter_ids.push(chapter_id);
    }
    Ok(chapter_ids)
}

pub fn seed_segments_from_chapter(
    conn: &Connection,
    chapter_id: &str,
    overwrite_ai_segments: bool,
) -> StudioResult<usize> {
    let (raw_text, existing_manual): (String, i64) = conn.query_row(
        "SELECT c.raw_text, (SELECT COUNT(*) FROM segments s WHERE s.chapter_id = c.id AND s.is_manual_edit = 1)
         FROM chapters c WHERE c.id = ?1",
        params![chapter_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if existing_manual > 0 && !overwrite_ai_segments {
        return Err(err("章节中已有人工编辑的分段，已停止覆盖操作"));
    }
    conn.execute(
        "DELETE FROM segments WHERE chapter_id = ?1 AND is_manual_edit = 0",
        params![chapter_id],
    )?;
    let lines = split_segments(&raw_text);
    let timestamp = now();
    for (index, line) in lines.iter().enumerate() {
        let (segment_type, speaker, text) = infer_segment(line);
        conn.execute(
            "INSERT INTO segments (id, chapter_id, scene_id, order_index, text, segment_type, speaker, character_id, emotion, sound_cue, anchor, voice_profile_id, audio_status, review_status, age_progress, is_manual_edit, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, NULL, NULL, NULL, NULL, NULL, 'missing', 'unreviewed', NULL, 0, ?7, ?8)",
            params![
                Uuid::new_v4().to_string(),
                chapter_id,
                index as i64,
                text,
                segment_type.as_str(),
                speaker,
                timestamp,
                timestamp
            ],
        )?;
    }
    conn.execute(
        "UPDATE chapters SET script_status = 'marked', updated_at = ?1 WHERE id = ?2",
        params![now(), chapter_id],
    )?;
    Ok(lines.len())
}

fn read_docx_text(path: &Path) -> StudioResult<String> {
    let file = fs::File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")?
        .read_to_string(&mut xml)?;
    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(text)) => {
                out.push_str(&text.decode().unwrap_or_default());
            }
            Ok(Event::End(end)) if end.name().as_ref() == b"w:p" => out.push('\n'),
            Ok(Event::Eof) => break,
            Err(error) => return Err(err(format!("解析 DOCX 内容失败：{error}"))),
            _ => {}
        }
    }
    Ok(out)
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_chapters(default_title: &str, text: &str) -> Vec<(String, String)> {
    let mut chapters = Vec::new();
    let mut current_title = default_title.to_string();
    let mut current = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let is_heading = trimmed.starts_with('第')
            && (trimmed.contains('章') || trimmed.contains('回') || trimmed.contains('集'));
        if is_heading && !current.is_empty() {
            chapters.push((current_title, current.join("\n")));
            current.clear();
            current_title = trimmed.to_string();
        } else if is_heading {
            current_title = trimmed.to_string();
        } else if !trimmed.is_empty() {
            current.push(trimmed.to_string());
        }
    }
    if !current.is_empty() {
        chapters.push((current_title, current.join("\n")));
    }
    if chapters.is_empty() {
        chapters.push((default_title.to_string(), text.to_string()));
    }
    chapters
}

fn split_segments(text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line.chars().count() <= 120 {
            segments.push(line.to_string());
            continue;
        }
        let mut buf = String::new();
        for ch in line.chars() {
            buf.push(ch);
            if matches!(ch, '。' | '！' | '？' | '；') && buf.chars().count() >= 20 {
                segments.push(buf.trim().to_string());
                buf.clear();
            }
        }
        if !buf.trim().is_empty() {
            segments.push(buf.trim().to_string());
        }
    }
    segments
}

fn infer_segment(line: &str) -> (SegmentType, Option<String>, String) {
    let clean = line.trim().to_string();
    if clean.starts_with('【') && clean.ends_with('】') {
        return (SegmentType::SoundCue, None, clean);
    }
    let separator_index = clean.find('：').or_else(|| clean.find(':'));
    if let Some(separator_index) = separator_index {
        let (speaker, text) = clean.split_at(separator_index);
        let text = text
            .strip_prefix('：')
            .or_else(|| text.strip_prefix(':'))
            .unwrap_or(text);
        if speaker.chars().count() <= 8 && !text.trim().is_empty() {
            return (
                SegmentType::Dialogue,
                Some(speaker.trim().to_string()),
                text.trim().to_string(),
            );
        }
    }
    if clean.starts_with('“') || clean.contains('”') {
        return (SegmentType::Dialogue, None, clean);
    }
    (SegmentType::Narration, None, clean)
}
