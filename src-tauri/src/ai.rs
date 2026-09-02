use crate::domain::SegmentType;
use crate::error::{err, StudioResult};
use crate::importer;
use crate::storage::{insert_job, mark_job, now};
use reqwest::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

const LLM_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const LLM_MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmSettings {
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MarkChapterPrepared {
    pub job_id: String,
    pub raw_text: Option<String>,
    pub settings: Option<LlmSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmSegmentMark {
    #[serde(default)]
    pub text: String,
    #[serde(rename = "segmentType", alias = "type", default)]
    pub segment_type: String,
    pub speaker: Option<String>,
    pub emotion: Option<String>,
    pub sound_cue: Option<String>,
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmCharacterMark {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LlmMarking {
    #[serde(default)]
    pub segments: Vec<LlmSegmentMark>,
    #[serde(default)]
    pub characters: Vec<LlmCharacterMark>,
}

pub fn prepare_mark_chapter(
    conn: &Connection,
    project_id: &str,
    chapter_id: &str,
    settings: Option<LlmSettings>,
) -> StudioResult<MarkChapterPrepared> {
    let chapter_text: Option<String> = conn
        .query_row(
            "SELECT raw_text FROM chapters WHERE id = ?1 AND project_id = ?2",
            params![chapter_id, project_id],
            |row| row.get(0),
        )
        .optional()?;
    let chapter_text = chapter_text.ok_or_else(|| err("章节不存在，无法标注"))?;
    let used_mock = settings
        .as_ref()
        .and_then(|settings| settings.api_key.as_ref())
        .map(|key| key.trim().is_empty())
        .unwrap_or(true);
    let payload = json!({ "chapterId": chapter_id, "provider": "openai-compatible" }).to_string();
    let job = insert_job(conn, project_id, "mark_chapter", &payload)?;
    mark_job(conn, &job.id, "running", 0.2, None)?;
    Ok(MarkChapterPrepared {
        job_id: job.id,
        raw_text: (!used_mock).then_some(chapter_text),
        settings,
    })
}

pub async fn call_openai_compatible(raw_text: &str, settings: LlmSettings) -> StudioResult<String> {
    let base_url = settings
        .base_url
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model = settings.model.unwrap_or_else(|| "gpt-4.1-mini".to_string());
    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "你是中文有声书制作标注助手。只返回 JSON，不要 Markdown 或解释。JSON 必须符合：{\"segments\":[{\"text\":\"原文分段\",\"segmentType\":\"narration|dialogue|inner_monologue|sound_cue|transition|timing_anchor|unknown\",\"speaker\":null,\"emotion\":null,\"soundCue\":null,\"anchor\":null}],\"characters\":[{\"name\":\"角色名\",\"aliases\":[]}]}. 不要改写 segments.text。"},
            {"role": "user", "content": format!("请按原文顺序标注下面的中文脚本，并尽量让每个 segments.text 与原文中的一行或一句完全一致：\n{}", raw_text)}
        ]
    });
    let base_url = base_url.trim_end_matches('/');
    let endpoint = if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else {
        format!("{base_url}/chat/completions")
    };
    let client = llm_http_client()?;
    let response = send_with_retries(
        &client,
        &endpoint,
        settings.api_key.unwrap_or_default(),
        &body,
    )
    .await?;
    let status = response.status();
    let response_text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(err(format!(
            "LLM 标注请求失败（HTTP {status}）：{}",
            response_text.chars().take(1000).collect::<String>()
        )));
    }
    Ok(response_text)
}

pub async fn test_openai_compatible(settings: LlmSettings) -> StudioResult<String> {
    let base_url = settings
        .base_url
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model = settings.model.unwrap_or_else(|| "gpt-4.1-mini".to_string());
    let api_key = settings
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| err("请先保存 LLM API Key"))?;
    let base_url = base_url.trim_end_matches('/');
    let endpoint = if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else {
        format!("{base_url}/chat/completions")
    };
    let client = llm_http_client()?;
    let response = send_with_retries(
        &client,
        &endpoint,
        api_key.to_string(),
        &json!({
            "model": model,
            "messages": [{"role": "user", "content": "只回复 OK"}],
            "max_tokens": 8
        }),
    )
    .await?;
    let status = response.status();
    let response_text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(err(format!(
            "LLM 连接失败（HTTP {status}）：{}",
            response_text.chars().take(500).collect::<String>()
        )));
    }
    Ok(format!("连接成功 · {model}"))
}

fn llm_http_client() -> StudioResult<Client> {
    Client::builder()
        .connect_timeout(LLM_CONNECT_TIMEOUT)
        .timeout(LLM_REQUEST_TIMEOUT)
        .build()
        .map_err(Into::into)
}

async fn send_with_retries(
    client: &Client,
    endpoint: &str,
    api_key: String,
    body: &Value,
) -> StudioResult<reqwest::Response> {
    let mut last_error = None;
    for attempt in 0..LLM_MAX_ATTEMPTS {
        match client
            .post(endpoint)
            .bearer_auth(&api_key)
            .json(body)
            .send()
            .await
        {
            Ok(response)
                if retryable_status(response.status()) && attempt + 1 < LLM_MAX_ATTEMPTS =>
            {
                let _ = response.bytes().await;
                sleep(retry_delay(attempt)).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if attempt + 1 < LLM_MAX_ATTEMPTS => {
                last_error = Some(error.to_string());
                sleep(retry_delay(attempt)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(err(format!(
        "LLM 网络请求失败，重试后仍未成功：{}",
        last_error.unwrap_or_else(|| "未收到响应".to_string())
    )))
}

fn retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn retry_delay(attempt: usize) -> Duration {
    Duration::from_millis(500 * 2u64.pow(attempt.min(4) as u32))
}

pub fn finish_mark_chapter(
    conn: &Connection,
    project_id: &str,
    chapter_id: &str,
    job_id: &str,
    provider_payload: Option<String>,
) -> StudioResult<usize> {
    let marking = provider_payload
        .as_deref()
        .map(parse_marking_payload)
        .transpose()?;
    if let Some(payload) = provider_payload {
        conn.execute(
            "INSERT OR REPLACE INTO provider_cache (id, project_id, provider, cache_key, payload_json, updated_at)
             VALUES (?1, ?2, 'openai-compatible', ?3, ?4, ?5)",
            params![
                Uuid::new_v4().to_string(),
                project_id,
                format!("mark_chapter:{chapter_id}"),
                payload,
                now()
            ],
        )?;
    }
    let count = importer::seed_segments_from_chapter(conn, chapter_id, false)?;
    if let Some(marking) = marking.as_ref() {
        apply_llm_marking(conn, chapter_id, marking)?;
    }
    extract_characters(conn, project_id)?;
    if let Some(marking) = marking.as_ref() {
        apply_character_aliases(conn, project_id, &marking.characters)?;
    }
    mark_job(conn, job_id, "succeeded", 1.0, None)?;
    Ok(count)
}

pub fn parse_marking_payload(payload: &str) -> StudioResult<LlmMarking> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|error| err(format!("LLM 标注响应不是有效 JSON：{error}")))?;
    if let Some(content) = value.pointer("/choices/0/message/content") {
        if let Some(marking) = parse_marking_content(content) {
            return Ok(marking);
        }
    }
    parse_marking_value(&value).ok_or_else(|| err("LLM 标注响应缺少 segments 数组"))
}

fn parse_marking_content(content: &Value) -> Option<LlmMarking> {
    if let Some(text) = content.as_str() {
        return parse_marking_text(text);
    }
    content.as_array().and_then(|parts| {
        let text = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<String>();
        parse_marking_text(&text)
    })
}

fn parse_marking_text(text: &str) -> Option<LlmMarking> {
    let trimmed = text.trim();
    serde_json::from_str(trimmed)
        .ok()
        .or_else(|| {
            let without_fence = trimmed
                .strip_prefix("```json")
                .or_else(|| trimmed.strip_prefix("```"))
                .and_then(|value| value.strip_suffix("```"))
                .map(str::trim)?;
            serde_json::from_str(without_fence).ok()
        })
        .or_else(|| {
            let start = trimmed.find('{')?;
            let end = trimmed.rfind('}')?;
            serde_json::from_str(&trimmed[start..=end]).ok()
        })
}

fn parse_marking_value(value: &Value) -> Option<LlmMarking> {
    if value.get("segments").is_some() {
        return serde_json::from_value(value.clone()).ok();
    }
    value
        .get("data")
        .and_then(parse_marking_value)
        .or_else(|| value.get("result").and_then(parse_marking_value))
}

pub fn apply_llm_marking(
    conn: &Connection,
    chapter_id: &str,
    marking: &LlmMarking,
) -> StudioResult<usize> {
    let mut stmt = conn.prepare(
        "SELECT id, text, is_manual_edit FROM segments WHERE chapter_id = ?1 ORDER BY order_index",
    )?;
    let current = stmt
        .query_map(params![chapter_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut used = HashSet::new();
    let mut updated = 0;
    for (mark_index, mark) in marking.segments.iter().enumerate() {
        let matching_index = current
            .iter()
            .enumerate()
            .find(|(index, (_, text, _))| {
                !used.contains(index)
                    && normalize_for_match(text) == normalize_for_match(&mark.text)
            })
            .map(|(index, _)| index)
            .or_else(|| {
                (marking.segments.len() == current.len())
                    .then_some(mark_index)
                    .filter(|index| !used.contains(index))
            });
        let Some(index) = matching_index else {
            continue;
        };
        used.insert(index);
        let (segment_id, _, is_manual) = &current[index];
        if *is_manual {
            continue;
        }
        let Some(segment_type) = normalized_segment_type(&mark.segment_type) else {
            continue;
        };
        conn.execute(
            "UPDATE segments SET segment_type = ?1, speaker = ?2, emotion = ?3, sound_cue = ?4, anchor = ?5, updated_at = ?6 WHERE id = ?7",
            params![
                segment_type,
                mark.speaker.as_deref().map(str::trim).filter(|value| !value.is_empty()),
                mark.emotion,
                mark.sound_cue,
                mark.anchor,
                now(),
                segment_id
            ],
        )?;
        updated += 1;
    }
    Ok(updated)
}

fn apply_character_aliases(
    conn: &Connection,
    project_id: &str,
    characters: &[LlmCharacterMark],
) -> StudioResult<()> {
    for character in characters {
        let Some(character_id) = conn
            .query_row(
                "SELECT id FROM characters WHERE project_id = ?1 AND canonical_name = ?2",
                params![project_id, character.name.trim()],
                |row| row.get::<_, String>(0),
            )
            .ok()
        else {
            continue;
        };
        for alias in &character.aliases {
            let alias = alias.trim();
            if alias.is_empty() || alias == character.name.trim() {
                continue;
            }
            conn.execute(
                "INSERT INTO character_aliases (id, character_id, alias)
                 SELECT ?1, ?2, ?3 WHERE NOT EXISTS
                 (SELECT 1 FROM character_aliases WHERE character_id = ?2 AND alias = ?3)",
                params![Uuid::new_v4().to_string(), character_id, alias],
            )?;
        }
    }
    Ok(())
}

fn normalize_for_match(value: &str) -> String {
    value.split_whitespace().collect::<String>()
}

fn normalized_segment_type(value: &str) -> Option<&'static str> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "narration" | "旁白" => Some(SegmentType::Narration.as_str()),
        "dialogue" | "台词" | "dialog" => Some(SegmentType::Dialogue.as_str()),
        "inner_monologue" | "innermonologue" | "内心独白" => {
            Some(SegmentType::InnerMonologue.as_str())
        }
        "sound_cue" | "soundcue" | "音效" => Some(SegmentType::SoundCue.as_str()),
        "transition" | "转场" => Some(SegmentType::Transition.as_str()),
        "timing_anchor" | "timinganchor" | "时间点" => Some(SegmentType::TimingAnchor.as_str()),
        "unknown" | "未知" => Some(SegmentType::Unknown.as_str()),
        _ => None,
    }
}

pub fn extract_characters(conn: &Connection, project_id: &str) -> StudioResult<()> {
    let colors = [
        "#2f80ed", "#27ae60", "#b55400", "#9b51e0", "#c0392b", "#118c8c",
    ];
    let mut stmt = conn.prepare(
        "SELECT DISTINCT speaker FROM segments
         WHERE speaker IS NOT NULL AND speaker != '' AND character_id IS NULL ORDER BY speaker",
    )?;
    let speakers = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for speaker in speakers {
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM characters WHERE project_id = ?1 AND canonical_name = ?2",
                params![project_id, speaker],
                |row| row.get(0),
            )
            .ok();
        let character_id = if let Some(id) = existing {
            id
        } else {
            let id = Uuid::new_v4().to_string();
            let color = colors[(speaker.len() + id.len()) % colors.len()];
            conn.execute(
                "INSERT INTO characters (id, project_id, canonical_name, gender, age_timeline, notes, default_color)
                 VALUES (?1, ?2, ?3, NULL, 'adult', NULL, ?4)",
                params![id, project_id, speaker, color],
            )?;
            id
        };
        conn.execute(
            "UPDATE segments SET character_id = ?1 WHERE speaker = ?2 AND character_id IS NULL",
            params![character_id, speaker],
        )?;
    }
    Ok(())
}
