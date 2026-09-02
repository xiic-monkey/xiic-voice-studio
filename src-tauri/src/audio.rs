use crate::domain::{
    ProductionAssetLine, ProductionCheckIssue, ProductionCheckReport, ProductionPackageManifest,
};
use crate::error::{err, StudioResult};
use crate::storage::{latest_audio_for_segment, newest_audio_for_segment, next_audio_version, now};
use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tokio::process::Command;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

const PROJECT_AUDIO_PREFIX: &str = "assets/audio/";
const LEGACY_DESKTOP_AUDIO_PREFIX: &str = "desktop-audio/";

pub fn new_audio_asset(
    project_root: &Path,
    project_id: &str,
    file_name: &str,
) -> StudioResult<(String, PathBuf)> {
    let relative_path = format!("{PROJECT_AUDIO_PREFIX}{project_id}/{file_name}");
    let output_path = resolve_audio_path(project_root, &relative_path)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok((relative_path, output_path))
}

pub fn resolve_audio_path(project_root: &Path, relative_path: &str) -> StudioResult<PathBuf> {
    if let Some(suffix) = relative_path.strip_prefix(LEGACY_DESKTOP_AUDIO_PREFIX) {
        let mut components = Path::new(suffix).components();
        let project_id = components
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .filter(|value| !value.is_empty() && *value != "." && *value != "..")
            .ok_or_else(|| err("音频路径缺少项目标识"))?;
        let mut output_path = legacy_audio_output_root(project_id)?;
        for component in components {
            let value = component.as_os_str();
            if value == ".." || value == "." {
                return Err(err("音频路径不安全"));
            }
            output_path.push(value);
        }
        return Ok(output_path);
    }
    let path = Path::new(relative_path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(err("音频路径不安全"));
    }
    Ok(project_root.join(path))
}

pub fn audio_output_directory(project_root: &Path, project_id: &str) -> StudioResult<PathBuf> {
    audio_output_root(project_root, project_id)
}

pub fn project_cover_path(project_root: &Path) -> Option<PathBuf> {
    ["jpg", "jpeg", "png"]
        .iter()
        .map(|extension| project_root.join(format!("assets/source/cover.{extension}")))
        .find(|path| path.is_file())
}

pub fn copy_project_cover(project_root: &Path, source_path: &Path) -> StudioResult<PathBuf> {
    if !source_path.is_file() {
        return Err(err("封面文件不存在"));
    }
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| err("封面文件缺少扩展名"))?;
    if !["jpg", "jpeg", "png"].contains(&extension.as_str()) {
        return Err(err("封面仅支持 JPG、JPEG 或 PNG 文件"));
    }
    let destination = project_root.join(format!("assets/source/cover.{extension}"));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    for other_extension in ["jpg", "jpeg", "png"] {
        let other = project_root.join(format!("assets/source/cover.{other_extension}"));
        if other != destination {
            let _ = fs::remove_file(other);
        }
    }
    fs::copy(source_path, &destination)?;
    Ok(destination)
}

pub fn validate_project_assets(
    conn: &Connection,
    project_root: &Path,
) -> StudioResult<crate::domain::AssetValidationReport> {
    let mut issues = Vec::new();
    let mut checked_assets = 0;
    let mut check = |asset_type: &str, relative_path: String| -> StudioResult<()> {
        checked_assets += 1;
        let path = resolve_audio_path(project_root, &relative_path)?;
        if !path.is_file() {
            issues.push(crate::domain::AssetValidationIssue {
                asset_type: asset_type.to_string(),
                relative_path,
                message: "资产文件不存在".to_string(),
            });
        }
        Ok(())
    };
    let mut audio_stmt = conn.prepare(
        "SELECT relative_path FROM segment_audio
         WHERE status IN ('approved', 'generated', 'uploaded')",
    )?;
    for path in audio_stmt.query_map([], |row| row.get::<_, String>(0))? {
        check("segment_audio", path?)?;
    }
    let mut voice_stmt =
        conn.prepare("SELECT relative_path FROM voice_assets WHERE status != 'deleted'")?;
    for path in voice_stmt.query_map([], |row| row.get::<_, String>(0))? {
        check("voice_asset", path?)?;
    }
    let missing_assets = issues.len() as i64;
    Ok(crate::domain::AssetValidationReport {
        valid: missing_assets == 0,
        checked_assets,
        missing_assets,
        issues,
    })
}

pub fn export_project_backup(project_root: &Path) -> StudioResult<PathBuf> {
    let exports_dir = project_root.join("exports");
    fs::create_dir_all(&exports_dir)?;
    let output_path = exports_dir.join(format!("project-backup-{}.zip", now().replace(':', "-")));
    let file = File::create(&output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for relative_root in ["project.json", "studio.sqlite", "assets", "exports"] {
        let source = project_root.join(relative_root);
        if !source.exists() {
            continue;
        }
        for entry in WalkDir::new(&source) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(project_root)
                .map_err(|error| err(format!("计算备份路径失败：{error}")))?;
            if relative.starts_with("exports/project-backup-") {
                continue;
            }
            add_file_to_zip(&mut zip, entry.path(), &relative.to_string_lossy(), options)?;
        }
    }
    zip.finish()?;
    Ok(output_path)
}

fn audio_output_root(project_root: &Path, project_id: &str) -> StudioResult<PathBuf> {
    let root = project_root.join(PROJECT_AUDIO_PREFIX).join(project_id);
    fs::create_dir_all(&root)?;
    Ok(root)
}

fn legacy_audio_output_root(project_id: &str) -> StudioResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| err("无法确定用户主目录，不能读取旧版桌面音频目录"))?;
    Ok(PathBuf::from(home)
        .join("Desktop")
        .join("Xiic Voice Studio Audio")
        .join(project_id))
}

pub async fn ffmpeg_version(ffmpeg_path: Option<String>) -> StudioResult<String> {
    let binary = ffmpeg_path.unwrap_or_else(|| "ffmpeg".to_string());
    let output = Command::new(binary).arg("-version").output().await?;
    if !output.status.success() {
        return Err(err(
            "未检测到 FFmpeg。请先安装 FFmpeg，或在设置中填写可执行文件路径。",
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.lines().next().unwrap_or("FFmpeg 可用").to_string())
}

#[allow(dead_code)]
pub fn export_voice_script(conn: &Connection, root: &Path) -> StudioResult<PathBuf> {
    export_voice_script_for_chapters(conn, root, None)
}

pub fn export_voice_script_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<PathBuf> {
    fs::create_dir_all(root.join("exports"))?;
    let segments = selected_segments(conn, chapter_ids)?;
    if segments.is_empty() {
        return Err(err("所选章节没有可导出的脚本分段"));
    }
    let mut markdown = String::from("# 配音脚本\n\n");
    for segment in &segments {
        markdown.push_str(&format!(
            "- [{}] {}{}\n",
            segment.segment_type.as_str(),
            segment
                .speaker
                .as_ref()
                .map(|speaker| format!("{speaker}: "))
                .unwrap_or_default(),
            segment.text
        ));
    }
    let md_path = root.join("exports/voice-script.md");
    let json_path = root.join("exports/voice-script.json");
    fs::write(&md_path, markdown)?;
    fs::write(&json_path, serde_json::to_string_pretty(&segments)?)?;
    Ok(md_path)
}

pub fn export_character_script(conn: &Connection, root: &Path) -> StudioResult<PathBuf> {
    fs::create_dir_all(root.join("exports"))?;
    let characters = crate::storage::list_characters(conn)?;
    let path = root.join("exports/character-script.csv");
    let mut writer = csv::Writer::from_path(&path)?;
    writer.write_record(["角色 ID", "角色名", "别名", "年龄阶段", "备注"])?;
    for character in characters {
        writer.write_record([
            character.id,
            character.canonical_name,
            character.aliases.join("|"),
            character.age_timeline.unwrap_or_default(),
            character.notes.unwrap_or_default(),
        ])?;
    }
    writer.flush()?;
    let json_path = root.join("exports/character-script.json");
    fs::write(
        &json_path,
        serde_json::to_string_pretty(&crate::storage::list_characters(conn)?)?,
    )?;
    Ok(path)
}

#[allow(dead_code)]
pub fn export_production_check_report(
    conn: &Connection,
    root: &Path,
) -> StudioResult<ProductionCheckReport> {
    export_production_check_report_for_chapters(conn, root, None)
}

pub fn export_production_check_report_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<ProductionCheckReport> {
    fs::create_dir_all(root.join("exports"))?;
    let report = build_production_check_report_for_chapters(conn, root, chapter_ids)?;
    let json_path = root.join("exports/production-check.json");
    let md_path = root.join("exports/production-check.md");
    fs::write(&json_path, serde_json::to_string_pretty(&report)?)?;
    fs::write(&md_path, production_check_markdown(&report))?;
    Ok(report)
}

#[allow(dead_code)]
pub fn build_production_check_report(
    conn: &Connection,
    root: &Path,
) -> StudioResult<ProductionCheckReport> {
    build_production_check_report_for_chapters(conn, root, None)
}

pub fn build_production_check_report_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<ProductionCheckReport> {
    let segments = selected_segments(conn, chapter_ids)?;
    let mut issues = Vec::new();
    let mut assets = Vec::new();
    let mut ready_segments = 0;
    let mut approved_segments = 0;
    let mut missing_audio = 0;
    let mut rejected_segments = 0;
    let mut unreviewed_segments = 0;
    let mut total_duration_ms = 0;

    for segment in &segments {
        let audio = latest_audio_for_segment(conn, &segment.id)?;
        let mut has_usable_audio = false;
        if let Some(audio) = &audio {
            let path = resolve_audio_path(root, &audio.relative_path)?;
            if path.exists() {
                has_usable_audio = true;
                ready_segments += 1;
                total_duration_ms += audio.duration_ms.unwrap_or(0);
                if audio.status == "approved" || segment.review_status == "approved" {
                    approved_segments += 1;
                }
                if audio.duration_ms.is_none() {
                    issues.push(report_issue(
                        "warning",
                        segment,
                        "音频缺少时长元数据，建议重新探测或重新导入音频",
                    ));
                }
            } else {
                issues.push(report_issue(
                    "blocking",
                    segment,
                    "数据库记录了音频，但资产文件不存在",
                ));
            }
        }

        if !has_usable_audio {
            missing_audio += 1;
            issues.push(report_issue("blocking", segment, "分段缺少可导出的音频"));
        }
        if segment.review_status == "rejected" {
            rejected_segments += 1;
            issues.push(report_issue("blocking", segment, "分段音频已标记返修"));
        } else if segment.review_status != "approved" {
            unreviewed_segments += 1;
            issues.push(report_issue("blocking", segment, "分段音频尚未审听通过"));
        }

        assets.push(ProductionAssetLine {
            segment_id: segment.id.clone(),
            audio_id: audio.as_ref().map(|audio| audio.id.clone()),
            order_index: segment.order_index,
            speaker: segment.speaker.clone(),
            audio_status: segment.audio_status.clone(),
            review_status: segment.review_status.clone(),
            source: audio.as_ref().map(|audio| audio.source.clone()),
            relative_path: audio.as_ref().map(|audio| audio.relative_path.clone()),
            export_path: audio
                .as_ref()
                .map(|audio| production_segment_export_path(conn, segment, audio))
                .transpose()?,
            version: audio.as_ref().map(|audio| audio.version),
            duration_ms: audio.as_ref().and_then(|audio| audio.duration_ms),
        });
    }

    let has_blocking = issues.iter().any(|issue| issue.severity == "blocking");
    Ok(ProductionCheckReport {
        generated_at: now(),
        can_publish: !has_blocking && !segments.is_empty(),
        total_segments: segments.len() as i64,
        ready_segments,
        approved_segments,
        missing_audio,
        rejected_segments,
        unreviewed_segments,
        total_duration_ms,
        issues,
        assets,
    })
}

#[allow(dead_code)]
pub fn export_segment_audio(conn: &Connection, root: &Path) -> StudioResult<PathBuf> {
    export_segment_audio_for_chapters(conn, root, None)
}

pub fn export_segment_audio_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<PathBuf> {
    let export_dir = root.join("exports/segmented-audio");
    if export_dir.exists() {
        fs::remove_dir_all(&export_dir)?;
    }
    fs::create_dir_all(&export_dir)?;
    let segments = selected_segments(conn, chapter_ids)?;
    let mut exported = 0;
    for segment in segments {
        if let Some(audio) = latest_audio_for_segment(conn, &segment.id)? {
            let source = resolve_audio_path(root, &audio.relative_path)?;
            if source.exists() {
                let file_name = production_segment_export_file_name(conn, &segment, &audio)?;
                fs::copy(source, export_dir.join(file_name))?;
                exported += 1;
            }
        }
    }
    if exported == 0 {
        return Err(err("没有可导出的分段音频，请先生成或导入音频"));
    }
    Ok(export_dir)
}

fn production_segment_export_path(
    conn: &Connection,
    segment: &crate::domain::Segment,
    audio: &crate::domain::SegmentAudio,
) -> StudioResult<String> {
    Ok(format!(
        "segmented-audio/{}",
        production_segment_export_file_name(conn, segment, audio)?
    ))
}

fn production_segment_export_file_name(
    conn: &Connection,
    segment: &crate::domain::Segment,
    audio: &crate::domain::SegmentAudio,
) -> StudioResult<String> {
    let ext = Path::new(&audio.relative_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("wav");
    let chapter_order: i64 = conn.query_row(
        "SELECT order_index FROM chapters WHERE id = ?1",
        params![segment.chapter_id],
        |row| row.get(0),
    )?;
    Ok(format!(
        "chapter-{:04}-segment-{:04}-{}.{}",
        chapter_order + 1,
        segment.order_index + 1,
        sanitize(&segment.text),
        ext
    ))
}

#[allow(dead_code)]
pub fn collect_episode_audio_paths(conn: &Connection, root: &Path) -> StudioResult<Vec<PathBuf>> {
    collect_episode_audio_paths_for_chapters(conn, root, None)
}

pub fn collect_episode_audio_paths_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<Vec<PathBuf>> {
    Ok(collect_episode_audio_selection_for_chapters(conn, root, chapter_ids)?.paths)
}

#[derive(Debug, Clone)]
pub struct EpisodeChapterMarker {
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
}

#[derive(Debug, Clone)]
pub struct EpisodeAudioSelection {
    pub paths: Vec<PathBuf>,
    pub chapters: Vec<EpisodeChapterMarker>,
}

pub fn collect_episode_audio_selection_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<EpisodeAudioSelection> {
    ensure_publishable_for_chapters(conn, root, chapter_ids)?;
    let segments = selected_segments(conn, chapter_ids)?;
    let mut audio_paths = Vec::new();
    let mut chapter_markers = Vec::new();
    let mut current_chapter_id: Option<String> = None;
    let mut current_chapter_title = String::new();
    let mut current_chapter_start = 0;
    let mut elapsed_ms = 0;
    for segment in &segments {
        if let Some(audio) = latest_audio_for_segment(conn, &segment.id)? {
            let path = resolve_audio_path(root, &audio.relative_path)?;
            if path.exists() && (audio.status == "approved" || segment.review_status == "approved")
            {
                if current_chapter_id.as_deref() != Some(segment.chapter_id.as_str()) {
                    if current_chapter_id.take().is_some() {
                        chapter_markers.push(EpisodeChapterMarker {
                            title: current_chapter_title,
                            start_ms: current_chapter_start,
                            end_ms: elapsed_ms,
                        });
                    }
                    current_chapter_id = Some(segment.chapter_id.clone());
                    current_chapter_title = conn.query_row(
                        "SELECT title FROM chapters WHERE id = ?1",
                        params![segment.chapter_id],
                        |row| row.get(0),
                    )?;
                    current_chapter_start = elapsed_ms;
                }
                audio_paths.push(path);
                elapsed_ms += audio
                    .duration_ms
                    .ok_or_else(|| err("整集导出失败：有音频缺少时长元数据，请重新导入或生成"))?;
            }
        }
    }
    if audio_paths.is_empty() {
        return Err(err("没有可用于整集导出的已审听通过音频"));
    }
    if audio_paths.len() != segments.len() {
        return Err(err("整集导出失败：仍有分段缺少审听通过的音频"));
    }
    if let Some(_) = current_chapter_id {
        chapter_markers.push(EpisodeChapterMarker {
            title: current_chapter_title,
            start_ms: current_chapter_start,
            end_ms: elapsed_ms,
        });
    }
    Ok(EpisodeAudioSelection {
        paths: audio_paths,
        chapters: chapter_markers,
    })
}

#[allow(dead_code)]
pub async fn export_episode_from_paths(
    root: &Path,
    audio_paths: Vec<PathBuf>,
    ffmpeg_path: Option<String>,
) -> StudioResult<PathBuf> {
    export_episode_from_selection(
        root,
        audio_paths,
        Vec::new(),
        ffmpeg_path,
        "wav",
        None,
        None,
        None,
    )
    .await
}

pub async fn export_episode_from_selection(
    root: &Path,
    audio_paths: Vec<PathBuf>,
    chapter_markers: Vec<EpisodeChapterMarker>,
    ffmpeg_path: Option<String>,
    format: &str,
    title: Option<&str>,
    author: Option<&str>,
    cover_path: Option<&Path>,
) -> StudioResult<PathBuf> {
    if audio_paths.is_empty() {
        return Err(err("没有可导出的整集音频"));
    }
    fs::create_dir_all(root.join("exports"))?;
    fs::create_dir_all(root.join("cache/providers"))?;
    let normalized_format = format.trim().to_ascii_lowercase();
    let (extension, audio_args) = match normalized_format.as_str() {
        "wav" => ("wav", vec!["-c:a", "pcm_s16le"]),
        "mp3" => ("mp3", vec!["-c:a", "libmp3lame", "-b:a", "192k"]),
        "m4b" => ("m4b", vec!["-c:a", "aac", "-b:a", "128k", "-f", "ipod"]),
        _ => return Err(err("不支持的整集格式，请选择 WAV、MP3 或 M4B")),
    };
    let binary = ffmpeg_path.unwrap_or_else(|| "ffmpeg".to_string());
    let normalized_paths = normalize_episode_audio_paths(root, &audio_paths, &binary).await?;
    let list_path = root.join("cache/providers/ffmpeg-concat.txt");
    let list_text = normalized_paths
        .iter()
        .map(|path| format!("file '{}'", path.to_string_lossy().replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&list_path, list_text)?;
    let metadata_path = if normalized_format != "m4b" || chapter_markers.is_empty() {
        None
    } else {
        let path = root.join("cache/providers/episode-metadata.txt");
        fs::write(&path, ffmetadata_text(&chapter_markers))?;
        Some(path)
    };
    let output_path =
        root.join("exports")
            .join(format!("episode-{}.{}", now().replace(':', "-"), extension));
    let mut command = Command::new(&binary);
    command
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list_path);
    let mut metadata_index = None;
    if let Some(metadata_path) = &metadata_path {
        metadata_index = Some(1);
        command.args(["-f", "ffmetadata", "-i"]).arg(metadata_path);
    }
    let mut cover_index = None;
    if normalized_format == "m4b" {
        if let Some(cover_path) = cover_path.filter(|path| path.is_file()) {
            let input_index = if metadata_index.is_some() { 2 } else { 1 };
            cover_index = Some(input_index);
            command.args(["-i"]).arg(cover_path);
        }
    }
    command.args(["-map", "0:a"]);
    if let Some(cover_index) = cover_index {
        command
            .args(["-map"])
            .arg(format!("{cover_index}:v"))
            .args(["-c:v", "copy", "-disposition:v", "attached_pic"]);
    }
    if let Some(metadata_index) = metadata_index {
        command
            .args(["-map_metadata"])
            .arg(metadata_index.to_string())
            .args(["-map_chapters"])
            .arg(metadata_index.to_string());
    }
    if let Some(title) = title.filter(|value| !value.trim().is_empty()) {
        command
            .args(["-metadata"])
            .arg(format!("title={}", title.trim()));
    }
    if let Some(author) = author.filter(|value| !value.trim().is_empty()) {
        command
            .args(["-metadata"])
            .arg(format!("artist={}", author.trim()));
    }
    let output = command.args(audio_args).arg(&output_path).output().await?;
    if !output.status.success() {
        return Err(err(format!(
            "FFmpeg 整集导出失败：{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output_path)
}

fn ffmetadata_text(chapters: &[EpisodeChapterMarker]) -> String {
    let mut output = String::from(";FFMETADATA1\n");
    for chapter in chapters {
        output.push_str("[CHAPTER]\nTIMEBASE=1/1000\n");
        output.push_str(&format!(
            "START={}\nEND={}\n",
            chapter.start_ms, chapter.end_ms
        ));
        output.push_str(&format!("title={}\n", escape_ffmetadata(&chapter.title)));
    }
    output
}

fn escape_ffmetadata(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('=', "\\=")
        .replace(';', "\\;")
        .replace('#', "\\#")
        .replace('\n', "\\n")
}

async fn normalize_episode_audio_paths(
    root: &Path,
    audio_paths: &[PathBuf],
    ffmpeg_binary: &str,
) -> StudioResult<Vec<PathBuf>> {
    let normalized_dir = root.join("cache/providers/episode-normalized");
    if normalized_dir.exists() {
        fs::remove_dir_all(&normalized_dir)?;
    }
    fs::create_dir_all(&normalized_dir)?;
    let mut normalized_paths = Vec::new();
    for (index, source) in audio_paths.iter().enumerate() {
        let output_path = normalized_dir.join(format!("{index:04}.wav"));
        let output = Command::new(ffmpeg_binary)
            .arg("-y")
            .arg("-i")
            .arg(source)
            .args([
                "-vn",
                "-ar",
                "44100",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                "-af",
                "loudnorm=I=-18:TP=-1.5:LRA=11",
            ])
            .arg(&output_path)
            .output()
            .await?;
        if !output.status.success() {
            return Err(err(format!(
                "FFmpeg 分段标准化失败：{}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        normalized_paths.push(output_path);
    }
    Ok(normalized_paths)
}

#[allow(dead_code)]
pub fn export_production_package(conn: &Connection, root: &Path) -> StudioResult<PathBuf> {
    export_production_package_for_chapters(conn, root, None)
}

pub fn export_production_package_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<PathBuf> {
    fs::create_dir_all(root.join("exports"))?;
    let report = export_production_check_report_for_chapters(conn, root, chapter_ids)?;
    if !report.can_publish {
        return Err(err(format!(
            "发布检查未通过，制作包未导出：{}",
            production_blocker_summary(&report)
        )));
    }
    export_voice_script_for_chapters(conn, root, chapter_ids)?;
    export_character_script(conn, root)?;
    let segmented_dir = export_segment_audio_for_chapters(conn, root, chapter_ids)?;
    export_production_manifest(root, &report, &segmented_dir)?;

    let output_path = root.join(format!(
        "exports/production-package-{}.zip",
        now().replace(':', "-")
    ));
    let file = File::create(&output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    add_file_to_zip(
        &mut zip,
        &root.join("project.json"),
        "project.json",
        options,
    )?;
    add_file_to_zip(
        &mut zip,
        &root.join("exports/voice-script.md"),
        "voice-script.md",
        options,
    )?;
    add_file_to_zip(
        &mut zip,
        &root.join("exports/voice-script.json"),
        "voice-script.json",
        options,
    )?;
    add_file_to_zip(
        &mut zip,
        &root.join("exports/character-script.csv"),
        "character-script.csv",
        options,
    )?;
    add_file_to_zip(
        &mut zip,
        &root.join("exports/character-script.json"),
        "character-script.json",
        options,
    )?;
    add_file_to_zip(
        &mut zip,
        &root.join("exports/production-check.md"),
        "production-check.md",
        options,
    )?;
    add_file_to_zip(
        &mut zip,
        &root.join("exports/production-check.json"),
        "production-check.json",
        options,
    )?;
    add_file_to_zip(
        &mut zip,
        &root.join("exports/production-manifest.json"),
        "production-manifest.json",
        options,
    )?;
    if segmented_dir.exists() {
        for entry in WalkDir::new(&segmented_dir).min_depth(1).max_depth(1) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let file_name = entry.file_name().to_string_lossy();
                add_file_to_zip(
                    &mut zip,
                    entry.path(),
                    &format!("segmented-audio/{file_name}"),
                    options,
                )?;
            }
        }
    }
    zip.finish()?;
    Ok(output_path)
}

fn export_production_manifest(
    root: &Path,
    report: &ProductionCheckReport,
    segmented_dir: &Path,
) -> StudioResult<PathBuf> {
    let project = crate::storage::project_summary(root)?;
    let mut files = vec![
        "project.json".to_string(),
        "voice-script.md".to_string(),
        "voice-script.json".to_string(),
        "character-script.csv".to_string(),
        "character-script.json".to_string(),
        "production-check.md".to_string(),
        "production-check.json".to_string(),
        "production-manifest.json".to_string(),
    ];
    if segmented_dir.exists() {
        let mut segmented = WalkDir::new(&segmented_dir)
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| format!("segmented-audio/{}", entry.file_name().to_string_lossy()))
            .collect::<Vec<_>>();
        segmented.sort();
        files.extend(segmented);
    }
    let manifest = ProductionPackageManifest {
        generated_at: now(),
        project_id: project.manifest.id,
        project_title: project.manifest.title,
        app_version: project.manifest.app_version,
        schema_version: project.manifest.schema_version,
        total_segments: report.total_segments,
        total_duration_ms: report.total_duration_ms,
        assets: report.assets.clone(),
        files,
    };
    let path = root.join("exports/production-manifest.json");
    fs::write(&path, serde_json::to_string_pretty(&manifest)?)?;
    Ok(path)
}

fn ensure_publishable_for_chapters(
    conn: &Connection,
    root: &Path,
    chapter_ids: Option<&[String]>,
) -> StudioResult<()> {
    let report = build_production_check_report_for_chapters(conn, root, chapter_ids)?;
    if report.can_publish {
        return Ok(());
    }
    Err(err(format!(
        "发布检查未通过，不能导出整集：{}",
        production_blocker_summary(&report)
    )))
}

fn selected_segments(
    conn: &Connection,
    chapter_ids: Option<&[String]>,
) -> StudioResult<Vec<crate::domain::Segment>> {
    let mut segments = crate::storage::list_segments(conn, None)?;
    let Some(chapter_ids) = chapter_ids else {
        return Ok(segments);
    };
    if chapter_ids.is_empty() {
        return Err(err("至少选择一个章节后再导出"));
    }
    let requested = chapter_ids.iter().collect::<HashSet<_>>();
    let known = crate::storage::list_chapters(conn)?
        .into_iter()
        .map(|chapter| chapter.id)
        .collect::<HashSet<_>>();
    if requested
        .iter()
        .any(|chapter_id| !known.contains(*chapter_id))
    {
        return Err(err("导出范围包含当前项目不存在的章节"));
    }
    segments.retain(|segment| requested.contains(&segment.chapter_id));
    Ok(segments)
}

fn production_blocker_summary(report: &ProductionCheckReport) -> String {
    let blocking = report
        .issues
        .iter()
        .filter(|issue| issue.severity == "blocking")
        .take(3)
        .map(|issue| format!("#{} {}", issue.order_index + 1, issue.message))
        .collect::<Vec<_>>();
    if blocking.is_empty() {
        return "请先完成脚本、音频生成和审听".to_string();
    }
    let suffix = report
        .issues
        .iter()
        .filter(|issue| issue.severity == "blocking")
        .count()
        .saturating_sub(blocking.len());
    if suffix > 0 {
        format!("{}；另有 {suffix} 个阻塞问题", blocking.join("；"))
    } else {
        blocking.join("；")
    }
}

pub fn create_review_issue(
    conn: &Connection,
    segment_id: Option<String>,
    audio_id: Option<String>,
    issue_type: String,
    note: String,
) -> StudioResult<()> {
    conn.execute(
        "INSERT INTO review_issues (id, segment_id, audio_id, issue_type, note, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'open', ?6)",
        params![
            uuid::Uuid::new_v4().to_string(),
            segment_id,
            audio_id,
            issue_type,
            note,
            now()
        ],
    )?;
    if let Some(segment_id) = segment_id {
        conn.execute(
            "UPDATE segments SET review_status = 'unreviewed', updated_at = ?1 WHERE id = ?2",
            params![now(), segment_id],
        )?;
    }
    Ok(())
}

pub fn upload_segment_audio(
    conn: &Connection,
    root: &Path,
    segment_id: &str,
    source_path: &Path,
    ffmpeg_path: Option<&str>,
) -> StudioResult<()> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM segments WHERE id = ?1",
        params![segment_id],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Err(err("分段不存在，无法上传音频"));
    }
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| err("音频文件缺少扩展名"))?;
    if !["wav", "mp3", "m4a", "aac", "flac", "ogg"].contains(&extension.as_str()) {
        return Err(err("仅支持上传 wav、mp3、m4a、aac、flac、ogg 音频文件"));
    }
    let version = next_audio_version(conn, segment_id)?;
    let file_name = format!("{segment_id}_manual_v{version}.{extension}");
    let project_id: String = conn.query_row(
        "SELECT c.project_id
         FROM segments s
         JOIN chapters c ON c.id = s.chapter_id
         WHERE s.id = ?1",
        params![segment_id],
        |row| row.get(0),
    )?;
    let (relative_path, output_path) = new_audio_asset(root, &project_id, &file_name)?;
    fs::copy(source_path, output_path)?;
    let duration_ms = detect_audio_duration_ms(source_path, ffmpeg_path).ok();
    conn.execute(
        "INSERT INTO segment_audio (id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at)
         VALUES (?1, ?2, ?3, ?4, NULL, ?5, 'manual_upload', 'uploaded', ?6)",
        params![
            uuid::Uuid::new_v4().to_string(),
            segment_id,
            relative_path,
            duration_ms,
            version,
            now()
        ],
    )?;
    conn.execute(
        "UPDATE segments SET audio_status = 'uploaded', review_status = 'unreviewed', updated_at = ?1 WHERE id = ?2",
        params![now(), segment_id],
    )?;
    Ok(())
}

pub fn set_latest_segment_audio_status(
    conn: &Connection,
    segment_id: &str,
    status: &str,
) -> StudioResult<()> {
    if !["approved", "rejected", "generated", "uploaded"].contains(&status) {
        return Err(err("不支持的音频状态"));
    }
    let Some(audio) = newest_audio_for_segment(conn, segment_id)? else {
        return Err(err("当前分段没有可审听的音频"));
    };
    conn.execute(
        "UPDATE segment_audio SET status = ?1 WHERE id = ?2",
        params![status, audio.id],
    )?;
    let review_status = match status {
        "approved" => "approved",
        "rejected" => "rejected",
        _ => "unreviewed",
    };
    let audio_status = if status == "rejected" {
        "generated"
    } else {
        status
    };
    conn.execute(
        "UPDATE segments SET audio_status = ?1, review_status = ?2, updated_at = ?3 WHERE id = ?4",
        params![audio_status, review_status, now(), segment_id],
    )?;
    if status == "approved" {
        conn.execute(
            "UPDATE review_issues SET status = 'resolved'
             WHERE segment_id = ?1 AND status = 'open'",
            params![segment_id],
        )?;
    }
    Ok(())
}

pub fn invalidate_segment_audio(
    conn: &Connection,
    segment_id: &str,
    reason: &str,
) -> StudioResult<()> {
    let changed = conn.execute(
        "UPDATE segment_audio
         SET status = 'stale'
         WHERE segment_id = ?1 AND status IN ('approved', 'generated', 'uploaded')",
        params![segment_id],
    )?;
    if changed > 0 {
        conn.execute(
            "INSERT INTO review_issues (id, segment_id, audio_id, issue_type, note, status, created_at)
             VALUES (?1, ?2, NULL, 'script_changed', ?3, 'open', ?4)",
            params![uuid::Uuid::new_v4().to_string(), segment_id, reason, now()],
        )?;
    }
    conn.execute(
        "UPDATE segments SET audio_status = 'missing', review_status = 'unreviewed', updated_at = ?1 WHERE id = ?2",
        params![now(), segment_id],
    )?;
    Ok(())
}

pub fn segment_audio_path(
    conn: &Connection,
    root: &Path,
    segment_id: &str,
) -> StudioResult<Option<String>> {
    Ok(newest_audio_for_segment(conn, segment_id)?
        .map(|audio| resolve_audio_path(root, &audio.relative_path))
        .transpose()?
        .map(|path| path.to_string_lossy().to_string()))
}

pub fn detect_audio_duration_ms(
    source_path: &Path,
    ffmpeg_path: Option<&str>,
) -> StudioResult<i64> {
    probe_audio_duration_ms(source_path, ffmpeg_path).or_else(|_| wav_duration_ms(source_path))
}

fn probe_audio_duration_ms(source_path: &Path, ffmpeg_path: Option<&str>) -> StudioResult<i64> {
    let ffprobe = ffprobe_binary(ffmpeg_path);
    let output = StdCommand::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(source_path)
        .output()?;
    if !output.status.success() {
        return Err(err("无法读取音频时长"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let seconds: f64 = text.trim().parse().map_err(|_| err("无法解析音频时长"))?;
    Ok((seconds * 1000.0).round() as i64)
}

fn wav_duration_ms(source_path: &Path) -> StudioResult<i64> {
    let mut bytes = Vec::new();
    File::open(source_path)?.read_to_end(&mut bytes)?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(err("不是可解析的 WAV 文件"));
    }
    let mut cursor = 12usize;
    let mut byte_rate = None;
    let mut data_size = None;
    while cursor + 8 <= bytes.len() {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        cursor += 8;
        if cursor + chunk_size > bytes.len() {
            break;
        }
        if chunk_id == b"fmt " && chunk_size >= 16 {
            byte_rate = Some(u32::from_le_bytes([
                bytes[cursor + 8],
                bytes[cursor + 9],
                bytes[cursor + 10],
                bytes[cursor + 11],
            ]) as i64);
        } else if chunk_id == b"data" {
            data_size = Some(chunk_size as i64);
        }
        cursor += chunk_size + (chunk_size % 2);
    }
    let byte_rate = byte_rate
        .filter(|value| *value > 0)
        .ok_or_else(|| err("WAV 缺少码率信息"))?;
    let data_size = data_size.ok_or_else(|| err("WAV 缺少音频数据块"))?;
    Ok((data_size * 1000) / byte_rate)
}

fn ffprobe_binary(ffmpeg_path: Option<&str>) -> String {
    let Some(ffmpeg_path) = ffmpeg_path.map(str::trim).filter(|value| !value.is_empty()) else {
        return "ffprobe".to_string();
    };
    let path = Path::new(ffmpeg_path);
    if path.file_name().and_then(|name| name.to_str()) == Some("ffmpeg") {
        return path.with_file_name("ffprobe").to_string_lossy().to_string();
    }
    "ffprobe".to_string()
}

fn report_issue(
    severity: &str,
    segment: &crate::domain::Segment,
    message: &str,
) -> ProductionCheckIssue {
    ProductionCheckIssue {
        severity: severity.to_string(),
        segment_id: segment.id.clone(),
        order_index: segment.order_index,
        speaker: segment.speaker.clone(),
        message: message.to_string(),
    }
}

fn production_check_markdown(report: &ProductionCheckReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("# 发布检查报告\n\n");
    markdown.push_str(&format!("- 生成时间：{}\n", report.generated_at));
    markdown.push_str(&format!(
        "- 发布状态：{}\n",
        if report.can_publish {
            "可发布"
        } else {
            "需要处理问题"
        }
    ));
    markdown.push_str(&format!("- 分段总数：{}\n", report.total_segments));
    markdown.push_str(&format!("- 可用音频：{}\n", report.ready_segments));
    markdown.push_str(&format!("- 审听通过：{}\n", report.approved_segments));
    markdown.push_str(&format!("- 缺失音频：{}\n", report.missing_audio));
    markdown.push_str(&format!("- 返修分段：{}\n", report.rejected_segments));
    markdown.push_str(&format!("- 未审听分段：{}\n", report.unreviewed_segments));
    markdown.push_str(&format!(
        "- 已知总时长：{}\n\n",
        format_duration(report.total_duration_ms)
    ));

    markdown.push_str("## 问题清单\n\n");
    if report.issues.is_empty() {
        markdown.push_str("未发现阻断或警告问题。\n\n");
    } else {
        for issue in &report.issues {
            markdown.push_str(&format!(
                "- [{}] #{:04} {}{}\n",
                issue.severity,
                issue.order_index + 1,
                issue
                    .speaker
                    .as_ref()
                    .map(|speaker| format!("{speaker}："))
                    .unwrap_or_default(),
                issue.message
            ));
        }
        markdown.push('\n');
    }

    markdown.push_str("## 资产清单\n\n");
    markdown.push_str(
        "| 序号 | 说话人 | 音频状态 | 审听状态 | 来源 | 版本 | 时长 | 包内路径 | 源路径 |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for asset in &report.assets {
        markdown.push_str(&format!(
            "| {:04} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            asset.order_index + 1,
            asset.speaker.as_deref().unwrap_or("旁白"),
            asset.audio_status,
            asset.review_status,
            asset.source.as_deref().unwrap_or("-"),
            asset
                .version
                .map(|version| version.to_string())
                .unwrap_or_else(|| "-".to_string()),
            asset
                .duration_ms
                .map(format_duration)
                .unwrap_or_else(|| "-".to_string()),
            asset.export_path.as_deref().unwrap_or("-"),
            asset.relative_path.as_deref().unwrap_or("-")
        ));
    }
    markdown
}

fn format_duration(duration_ms: i64) -> String {
    let total_seconds = duration_ms / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn sanitize(value: &str) -> String {
    let mut out: String = value
        .chars()
        .filter(|ch| ch.is_alphanumeric() || *ch == '_' || *ch == '-')
        .take(24)
        .collect();
    if out.is_empty() {
        out = "分段".to_string();
    }
    out
}

fn add_file_to_zip(
    zip: &mut zip::ZipWriter<File>,
    source: &Path,
    zip_path: &str,
    options: SimpleFileOptions,
) -> StudioResult<()> {
    if !source.exists() {
        return Ok(());
    }
    zip.start_file(zip_path, options)?;
    let mut file = File::open(source)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    zip.write_all(&bytes)?;
    Ok(())
}
