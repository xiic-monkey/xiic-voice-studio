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

/// 这些 speaker 泛称不建角色：旁白由声音配置承担，其余无法稳定对应单一音色。
/// 有身份的群杂（族人1、中年男人等）是合法角色，各自独立建档、可分配不同音色。
const GENERIC_SPEAKERS: [&str; 12] = [
    "旁白", "台词", "未知", "众人", "群众", "路人", "路人甲", "路人乙", "声音", "男声", "女声", "人群",
];

fn is_generic_speaker(value: &str) -> bool {
    GENERIC_SPEAKERS.contains(&value.trim())
}

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
            {"role": "system", "content": r#"你是中文有声书制作标注助手。任务：把中文小说原文拆成分段，标注每段的类型、说话人，并提取出场角色。只返回 JSON，不要 Markdown 代码块，不要任何解释。

JSON 格式：
{"segments":[{"text":"原文分段","segmentType":"narration|dialogue|inner_monologue|sound_cue|transition|timing_anchor|unknown","speaker":null,"emotion":null,"soundCue":null,"anchor":null}],"characters":[{"name":"角色名","aliases":[]}]}

分段规则：
1. segments.text 必须逐字复制原文，禁止改写、总结、合并或拆分。按原文顺序，一段对话或一句叙述为一个分段。
2. segmentType 判定：
   - narration：叙述、描写、旁白
   - dialogue：角色说出口的台词（引号内内容或对话行）
   - inner_monologue：人物内心活动、心理描写（没有说出口）
   - sound_cue：音效、环境声提示
   - transition：场景切换、时间过渡
   - timing_anchor：时间点标记
3. speaker 规则（最重要）：
   - dialogue 的 speaker 填说话角色的名字，优先用全名（如"萧炎"）；名字尚未揭示时用稳定的描述性称呼（如"灰袍老者"），后文揭示后沿用正式名字
   - inner_monologue 的 speaker 填心理活动所属的角色
   - narration、sound_cue、transition、timing_anchor 的 speaker 必须是 null，旁白不是角色
   - speaker 只能填具体人物，禁止填"旁白""台词""众人""群众""路人""声音""男声""女声"这类泛称；无法确定说话人时填 null
   - 有身份但无姓名的配角是合法的 speaker：族人1、族人2、中年男人、店小二等。同身份的不同个体（族人1、族人2）是不同的人，编号必须原样保留、各自独立，禁止合并成同一个称呼
4. emotion 情绪规则：
   - dialogue 和 inner_monologue 必须根据台词内容和上下文给出情绪，用以下固定词表：平静、温柔、愤怒、悲伤、喜悦、兴奋、紧张、恐惧、疑惑、坚定、疲惫、嘲讽、撒娇、冷漠、惊讶
   - 情绪要看上下文：同一角色相邻的台词情绪可能递进或转折，不要机械地全部填同一个值
   - narration、sound_cue、transition、timing_anchor 的 emotion 填 null
5. characters：列出本章出场、有名字或有稳定称呼的故事角色，包括有身份无姓名的配角（族人1、族人2、中年男人等，每个编号单独一条，不要合并）。aliases 填同一角色的其他称呼（例如"药老"的别名是"药尘"）。不要收录旁白和纯泛称（众人、群众、人群、路人等）。"#},
            {"role": "user", "content": format!("请标注以下章节原文：\n\n{raw_text}")}
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

/// 截取文本开头用于规则归纳：非空行最多 400 行、最多 8000 字。
pub fn excerpt_for_rule_detection(text: &str) -> String {
    let mut lines = Vec::new();
    let mut size = 0usize;
    for line in text.lines().filter(|line| !line.trim().is_empty()).take(400) {
        size += line.chars().count();
        if size > 8000 {
            break;
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// 让 LLM 从文本样本归纳章节标题的正则。只做编译校验，命中合理性由调用方预览确认。
/// `hint` 为可选的自然语言格式说明，用于引导模型（例如「章节标题形如『卷一· 初入江湖』」）。
pub async fn detect_chapter_rule(
    excerpt: &str,
    hint: Option<&str>,
    settings: LlmSettings,
) -> StudioResult<String> {
    let key_empty = settings
        .api_key
        .as_ref()
        .map(|key| key.trim().is_empty())
        .unwrap_or(true);
    if key_empty {
        return Err(err("未配置 LLM API Key，无法自动解析拆章规则"));
    }
    let hint_block = match hint {
        Some(hint) if !hint.trim().is_empty() => {
            format!("\n\n用户额外说明的分章格式（请优先满足）：{}", hint.trim())
        }
        _ => String::new(),
    };
    let system = r#"你是文本格式分析助手。分析给定小说文本的章节标题格式，返回 JSON：{"regex":"...","description":"..."}。regex 要求：
1. Rust regex 语法，不支持反向引用和 lookaround
2. 从行首匹配完整的章节标题行（用 ^ 开头）
3. 应能匹配文本中所有章节标题，且尽量不匹配正文行
4. 示例：文本用「第一章 xxx」「第二章 xxx」分章，返回 "^第[〇零0-9０-９一二三四五六七八九十百千两]+[章节回卷集部篇]"
5. 文本用「1. 标题」「2. 标题」分章，返回 "^\\d{1,4}[.、．]\\s*\\S+"
只返回 JSON，不要 Markdown 代码块和解释。"#;
    let user = format!(
        "请分析以下小说开头的分章规则：\n\n{excerpt}{hint_block}"
    );
    let body = json!({
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    let response_text = call_openai_compatible_json(body, settings).await?;
    parse_detected_rule(&response_text)
}

async fn call_openai_compatible_json(body: Value, settings: LlmSettings) -> StudioResult<String> {
    let base_url = settings
        .base_url
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    let model = settings.model.unwrap_or_else(|| "gpt-4.1-mini".to_string());
    let mut body = body;
    body["model"] = json!(model);
    let base_url = base_url.trim_end_matches('/');
    let endpoint = if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else {
        format!("{base_url}/chat/completions")
    };
    let client = llm_http_client()?;
    let response = send_with_retries(&client, &endpoint, settings.api_key.unwrap_or_default(), &body).await?;
    let status = response.status();
    let response_text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(err(format!(
            "LLM 请求失败（HTTP {status}）：{}",
            response_text.chars().take(1000).collect::<String>()
        )));
    }
    Ok(response_text)
}

fn parse_detected_rule(payload: &str) -> StudioResult<String> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|error| err(format!("LLM 拆章规则响应不是有效 JSON：{error}")))?;
    let content = value
        .pointer("/choices/0/message/content")
        .ok_or_else(|| err("LLM 拆章规则响应缺少内容"))?;
    let text = content
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            content.as_array().map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<String>()
            })
        })
        .ok_or_else(|| err("LLM 拆章规则响应格式异常"))?;
    let start = text.find('{').ok_or_else(|| err("LLM 未返回拆章规则 JSON"))?;
    let end = text.rfind('}').ok_or_else(|| err("LLM 未返回拆章规则 JSON"))?;
    let rule: Value = serde_json::from_str(&text[start..=end])
        .map_err(|error| err(format!("LLM 拆章规则 JSON 解析失败：{error}")))?;
    let regex = rule
        .get("regex")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| err("LLM 返回的拆章规则缺少 regex 字段"))?;
    Ok(regex.to_string())
}

/// 通用文本补全：system + user，返回 assistant 文本内容。
pub async fn chat_completion(
    settings: LlmSettings,
    system: &str,
    user: &str,
) -> StudioResult<String> {
    let body = json!({
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    let response_text = call_openai_compatible_json(body, settings).await?;
    let value: Value = serde_json::from_str(&response_text)
        .map_err(|error| err(format!("LLM 响应不是有效 JSON：{error}")))?;
    let content = value
        .pointer("/choices/0/message/content")
        .ok_or_else(|| err("LLM 响应缺少内容"))?;
    let text = content
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            content.as_array().map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<String>()
            })
        })
        .ok_or_else(|| err("LLM 响应格式异常"))?;
    Ok(text.trim().to_string())
}

/// 根据角色信息与台词样本，生成符合 Mimo voicedesign 最佳实践的音色描述。
pub async fn generate_voice_description(
    character_name: &str,
    character_info: &str,
    dialogue_samples: &str,
    settings: LlmSettings,
) -> StudioResult<String> {
    let system = "你是中文有声书配音导演。根据角色的性别、年龄、性格与台词风格，为该角色撰写一段用于 TTS 音色设计的音色描述。要求：1-4 句中文，覆盖性别与年龄段、音色质感、说话语气、语速节奏；与角色性格和台词风格一致；只返回描述文本本身，不要任何前缀、引号或解释。";
    let user = format!(
        "角色：{character_name}\n角色资料：{character_info}\n台词样本：\n{dialogue_samples}"
    );
    let text = chat_completion(settings, system, &user).await?;
    if text.trim().is_empty() {
        return Err(err("LLM 未返回音色描述"));
    }
    Ok(text)
}

pub async fn test_openai_compatible(settings: LlmSettings) -> StudioResult<String> {    let base_url = settings
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
        // 旁白/音效/转场不归属角色，丢弃 LLM 可能误填的 speaker
        let speaker = mark
            .speaker
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .filter(|_| matches!(segment_type, "dialogue" | "inner_monologue"));
        conn.execute(
            "UPDATE segments SET segment_type = ?1, speaker = ?2, emotion = ?3, sound_cue = ?4, anchor = ?5, updated_at = ?6 WHERE id = ?7",
            params![
                segment_type,
                speaker,
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

pub fn apply_character_aliases(
    conn: &Connection,
    project_id: &str,
    characters: &[LlmCharacterMark],
) -> StudioResult<()> {
    for character in characters {
        let name = character.name.trim();
        if name.is_empty() || is_generic_speaker(name) {
            continue;
        }
        let character_id = match conn
            .query_row(
                "SELECT id FROM characters WHERE project_id = ?1 AND canonical_name = ?2",
                params![project_id, name],
                |row| row.get::<_, String>(0),
            )
            .ok()
        {
            Some(id) => id,
            // LLM 提取到的角色若没在任何分段 speaker 里出现过，也补建档案
            None => insert_character(conn, project_id, name)?,
        };
        for alias in &character.aliases {
            let alias = alias.trim();
            if alias.is_empty() || alias == name {
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

fn pick_character_color(seed: &str) -> &'static str {
    let colors = [
        "#2f80ed", "#27ae60", "#b55400", "#9b51e0", "#c0392b", "#118c8c",
    ];
    let hash = seed.bytes().map(u32::from).sum::<u32>();
    colors[(hash % colors.len() as u32) as usize]
}

fn insert_character(conn: &Connection, project_id: &str, name: &str) -> StudioResult<String> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO characters (id, project_id, canonical_name, gender, age_timeline, notes, default_color)
         VALUES (?1, ?2, ?3, NULL, 'adult', NULL, ?4)",
        params![id, project_id, name, pick_character_color(name)],
    )?;
    Ok(id)
}

/// 从分段 speaker 里提取角色。泛称（旁白、众人等）不建角色，保持分段无归属。
pub fn extract_characters(conn: &Connection, project_id: &str) -> StudioResult<()> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT speaker FROM segments
         WHERE speaker IS NOT NULL AND speaker != '' AND character_id IS NULL ORDER BY speaker",
    )?;
    let speakers = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for speaker in speakers {
        if is_generic_speaker(&speaker) {
            continue;
        }
        let character_id = match conn
            .query_row(
                "SELECT id FROM characters WHERE project_id = ?1 AND canonical_name = ?2",
                params![project_id, speaker],
                |row| row.get(0),
            )
            .ok()
        {
            Some(id) => id,
            None => insert_character(conn, project_id, &speaker)?,
        };
        conn.execute(
            "UPDATE segments SET character_id = ?1 WHERE speaker = ?2 AND character_id IS NULL",
            params![character_id, speaker],
        )?;
    }
    Ok(())
}
