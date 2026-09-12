use crate::domain::SegmentType;
use crate::error::{err, StudioResult};
use crate::storage::now;
use quick_xml::events::Event;
use quick_xml::Reader;
use regex::Regex;
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

/// 内置拆章规则。顺序即启发式择优顺序：越靠前越特异，命中即停。
pub struct SplitRule {
    pub id: &'static str,
    pub label: &'static str,
    pub pattern: &'static str,
    pub description: &'static str,
}

pub const SPLIT_RULES: &[SplitRule] = &[
    SplitRule {
        id: "di",
        label: "第X章 / 第X回 / 第X卷",
        pattern: r"^第[〇零0-9０-９一二三四五六七八九十百千两]+\s*[章节回卷集部篇]",
        description: "匹配「第一章 陨落的天才」「第123章」「第三回」等中文序号标题",
    },
    SplitRule {
        id: "chapter-en",
        label: "Chapter N",
        pattern: r"^Chapter\s+[0-9０-９]+",
        description: "匹配英文编号标题，如「Chapter 12 A New Dawn」",
    },
    SplitRule {
        id: "bracket",
        label: "【第X章 …】",
        pattern: r"^【[^】]{0,40}[章节回卷集][^】]{0,40}】",
        description: "匹配整行括号包裹的标题，如「【第一章 起程】」",
    },
    SplitRule {
        id: "numbered",
        label: "1. 标题 / 1、标题",
        pattern: r"^[0-9０-９]{1,4}([.、,:：．]|\s)\s*\S+",
        description: "匹配阿拉伯数字编号标题，如「12. 离开新手村」",
    },
    SplitRule {
        id: "number-line",
        label: "单独数字行",
        pattern: r"^[0-9０-９]{1,4}$",
        description: "匹配整行只有一个数字的标题行",
    },
];

pub fn compile_split_pattern(pattern: &str) -> StudioResult<Regex> {
    Regex::new(pattern).map_err(|error| err(format!("正则无效：{error}")))
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitRuleDto {
    pub id: &'static str,
    pub label: &'static str,
    pub pattern: &'static str,
    pub description: &'static str,
}

pub fn split_rule_dtos() -> Vec<SplitRuleDto> {
    SPLIT_RULES
        .iter()
        .map(|rule| SplitRuleDto {
            id: rule.id,
            label: rule.label,
            pattern: rule.pattern,
            description: rule.description,
        })
        .collect()
}

/// 启发式合理性：至少 3 个命中、命中行都是短行（≤50 字）、
/// 且标题行不超过非空行数的 50%（标题不能是正文主体），
/// 避免把正文里的引用或编号列表当成章节。
fn is_plausible_split_pattern(text: &str, regex: &Regex) -> bool {
    let non_empty = text.lines().filter(|line| !line.trim().is_empty()).count();
    let mut matches = 0usize;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !regex.is_match(trimmed) {
            continue;
        }
        if trimmed.chars().count() > 50 {
            return false;
        }
        matches += 1;
    }
    matches >= 3 && non_empty > 0 && matches * 2 <= non_empty
}

/// 内置规则按顺序择优，全部不命中则返回 None（整篇作为单章）。
pub fn detect_chapter_pattern(text: &str) -> Option<Regex> {
    for rule in SPLIT_RULES {
        let Ok(regex) = Regex::new(rule.pattern) else {
            continue;
        };
        if is_plausible_split_pattern(text, &regex) {
            return Some(regex);
        }
    }
    None
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitPreview {
    pub chapter_count: usize,
    pub sample_titles: Vec<String>,
    /// 实际命中的章节标题行（最多 12 行），供导入前预览正则匹配效果。
    pub matched_lines: Vec<String>,
    /// "pattern"：使用给定正则；"heuristic"：启发式择优；"single"：未能拆分
    pub rule_source: String,
}

pub fn preview_split(text: &str, pattern: Option<&Regex>) -> SplitPreview {
    // 解析实际生效的正则：传入 None 时走启发式检测。
    let effective = pattern.cloned().or_else(|| detect_chapter_pattern(text));
    let matched_lines: Vec<String> = match &effective {
        Some(regex) => text
            .lines()
            .map(clean_title)
            .filter(|line| !line.is_empty() && regex.is_match(line))
            .take(12)
            .collect(),
        None => Vec::new(),
    };
    let chapters = split_chapters("导入稿件", text, effective.as_ref());
    let rule_source = if pattern.is_some() {
        "pattern"
    } else if chapters.len() > 1 {
        "heuristic"
    } else {
        "single"
    };
    SplitPreview {
        chapter_count: chapters.len(),
        sample_titles: chapters
            .iter()
            .take(8)
            .map(|(title, _)| title.clone())
            .collect(),
        matched_lines,
        rule_source: rule_source.to_string(),
    }
}

/// 便捷入口：启发式自动识别拆章规则。仅测试使用，正式导入走
/// `import_source_with_pattern`（支持用户选择/自定义正则）。
#[allow(dead_code)]
pub fn import_source(
    conn: &Connection,
    project_id: &str,
    source: &ImportedSource,
) -> StudioResult<Vec<String>> {
    let pattern = detect_chapter_pattern(&source.text);
    import_source_with_pattern(conn, project_id, source, pattern.as_ref())
}

pub fn import_source_with_pattern(
    conn: &Connection,
    project_id: &str,
    source: &ImportedSource,
    pattern: Option<&Regex>,
) -> StudioResult<Vec<String>> {
    let chapters = split_chapters(&source.title, &source.text, pattern);
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

/// 章节标题清洗：折叠所有空白（含换行/制表/零宽空格）为单个空格并去首尾，
/// 保证标题永远是单行、无异常换行。源文件里的换行式标题或多余空格都在此兜底。
fn clean_title(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_chapters(
    default_title: &str,
    text: &str,
    pattern: Option<&Regex>,
) -> Vec<(String, String)> {
    let mut chapters = Vec::new();
    let mut current_title = default_title.to_string();
    let mut current = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let is_heading = match pattern {
            Some(regex) => regex.is_match(trimmed),
            None => {
                trimmed.starts_with('第')
                    && (trimmed.contains('章') || trimmed.contains('回') || trimmed.contains('集'))
            }
        };
        if is_heading && !current.is_empty() {
            chapters.push((current_title, current.join("\n")));
            current.clear();
            current_title = clean_title(trimmed);
        } else if is_heading {
            current_title = clean_title(trimmed);
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
    // 以开引号开头的行视为台词（announcement/对话），不要按“speaker：text”拆分，
    // 否则 "斗之气：七段！" 会被拆成 speaker="“斗之气"、text="七段！"。
    if clean.starts_with(['“', '‘', '「', '『']) {
        return (SegmentType::Dialogue, None, clean);
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
