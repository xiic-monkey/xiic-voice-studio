//! 角色音色描述的「章节上下文」装配。
//!
//! 旧实现把该角色的前 5 条 dialogue 分段直接当"台词样本"喂给 LLM，
//! 而这些分段常常是「对白 + 叙述」混在一起的长文本，例如：
//!
//! ```text
//! “萧炎，斗之力，三段！级别：低级！”测验魔石碑之旁，一位中年男子，
//! 看了一眼碑上所显示出来的信息，语气漠然的将之公布了出来…
//! ```
//!
//! 于是 LLM 把章节原文照抄回来当"音色描述"——用户在工坊里看到的就是这个。
//!
//! 这里把上下文拆成三份：
//!   1. 台词：引号内的文本，来自该角色自己的分段；
//!   2. 人物叙述：同一分段里引号外的部分，通常正是"语气漠然""略微恭声"这类气质线索；
//!   3. 相关叙述：其它提到该角色（含别名）的叙述分段。
//!
//! 并据此给出一份「有据可查」的本地草稿，作为工坊表单的自动填充值——
//! 没有 LLM Key 也能用，有 Key 时再由 AI 精修。

use crate::domain::Segment;

/// 上下文里每类文本的条数上限（防止把整本书塞进提示词）。
const MAX_LINES: usize = 16;
const MAX_NARRATION: usize = 16;
/// 单条文本超过这个长度就截断，避免长段落淹没关键线索。
const MAX_TEXT: usize = 120;

/// 线索词出现在名字/别名附近多少字以内，算「强证据」。
const PROXIMITY_WINDOW: usize = 12;
/// 强证据的权重（普通出现记为 1）。
const NEAR_WEIGHT: usize = 3;

/// `characters.age_timeline` 的占位值。
///
/// `ai::insert_character` 建角色时无条件写入 `'adult'`（gender 写 NULL），
/// 也就是说它代表"没人确认过"，不能压过名字里更具体的线索
/// ——否则「中年男子」会被写成成年，甚至原样打印出 `adult` 这种英文 token。
const PLACEHOLDER_AGE_STAGE: &str = "adult";

/// 年龄阶段 token → 中文标签，取值与前端 `ageStageLabels` 一致。
const AGE_STAGE_LABELS: &[(&str, &str)] = &[
    ("childhood", "童年"),
    ("teenager", "少年"),
    ("young_adult", "青年"),
    ("adult", "成年"),
    ("middle_aged", "中年"),
    ("elderly", "老年"),
];

/// 把 DB 里的性别写法归一成「男」/「女」。
///
/// 注意 `female` 与 `male` 是包含关系，必须整串比较，不能用 contains。
pub fn normalize_gender(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.eq_ignore_ascii_case("male") {
        return Some("男".to_string());
    }
    if value.eq_ignore_ascii_case("female") {
        return Some("女".to_string());
    }
    if value.contains('男') {
        return Some("男".to_string());
    }
    if value.contains('女') {
        return Some("女".to_string());
    }
    None
}

/// 该值是否只是导入阶段写下的占位年龄阶段。
pub fn is_placeholder_age_stage(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case(PLACEHOLDER_AGE_STAGE)
}

/// 把 DB 里的年龄阶段写法归一成中文标签。
///
/// 既认阶段 token（`middle_aged`），也认人直接在自由文本框里写的中文（`中年`）。
pub fn age_stage_label(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some((_, label)) = AGE_STAGE_LABELS
        .iter()
        .find(|(token, _)| token.eq_ignore_ascii_case(value))
    {
        return Some((*label).to_string());
    }
    if let Some((_, label)) = AGE_STAGE_LABELS.iter().find(|(_, label)| *label == value) {
        return Some((*label).to_string());
    }
    if AGE_CUES.iter().any(|(_, label)| *label == value) {
        return Some(value.to_string());
    }
    None
}

/// 人物的性别/年龄段线索：命中即用，顺序即优先级。
const GENDER_CUES: &[(&str, &str)] = &[
    ("少女", "女"),
    ("女子", "女"),
    ("女孩", "女"),
    ("姑娘", "女"),
    ("妇人", "女"),
    ("女人", "女"),
    ("小姐", "女"),
    ("女声", "女"),
    ("少年", "男"),
    ("男子", "男"),
    ("男人", "男"),
    ("青年", "男"),
    ("汉子", "男"),
    ("老者", "男"),
    ("老人", "男"),
    ("公子", "男"),
    ("少爷", "男"),
    ("男声", "男"),
];

const AGE_CUES: &[(&str, &str)] = &[
    ("童年", "童年"),
    ("孩童", "童年"),
    ("少年", "少年"),
    ("少女", "少年"),
    ("青年", "青年"),
    ("中年", "中年"),
    ("老者", "老年"),
    ("老人", "老年"),
    ("老太", "老年"),
    ("白发", "老年"),
];

/// 语气 / 音色质感线索词。左列是文中的字面，右列是归并后的说法。
const TONE_CUES: &[(&str, &str)] = &[
    ("漠然", "漠然"),
    ("冷淡", "冷淡"),
    ("冷冷", "冷淡"),
    ("冷声", "冷硬"),
    ("冷哼", "冷硬"),
    ("冷笑", "讥诮"),
    ("讥讽", "讥诮"),
    ("嘲讽", "讥诮"),
    ("戏谑", "戏谑"),
    ("调侃", "戏谑"),
    ("淡淡", "淡然"),
    ("平静", "平静"),
    ("沉稳", "沉稳"),
    ("恭声", "客气"),
    ("恭敬", "恭敬"),
    ("温和", "温和"),
    ("温柔", "温柔"),
    ("热情", "热情"),
    ("慵懒", "慵懒"),
    ("懒散", "懒散"),
    ("无奈", "无奈"),
    ("苦笑", "无奈"),
    ("焦急", "焦急"),
    ("急切", "急切"),
    ("颤抖", "紧张"),
    ("紧张", "紧张"),
    ("坚定", "坚定"),
    ("严肃", "严肃"),
    ("欣喜", "喜悦"),
    ("兴奋", "兴奋"),
    ("愤怒", "易怒"),
    ("大怒", "易怒"),
    ("咬牙", "强硬"),
];

/// 直接描写音色/嗓音质感的线索词。
const TIMBRE_CUES: &[(&str, &str)] = &[
    ("低沉", "低沉"),
    ("沙哑", "沙哑"),
    ("嘶哑", "沙哑"),
    ("清亮", "清亮"),
    ("清脆", "清脆"),
    ("洪亮", "洪亮"),
    ("苍老", "苍老"),
    ("浑厚", "浑厚"),
    ("尖细", "尖细"),
    ("柔美", "柔美"),
];

/// 把一段文本拆成「引号内的台词」与「引号外的叙述」。
///
/// 支持中文引号（“ ”）、直角引号（「 」『 』）和英文双引号。
pub fn split_quotes(text: &str) -> (Vec<String>, String) {
    const PAIRS: [(char, char); 4] = [('“', '”'), ('「', '」'), ('『', '』'), ('"', '"')];
    let mut quotes: Vec<String> = Vec::new();
    let mut rest = String::new();
    let mut current: Option<(char, String)> = None;
    for character in text.chars() {
        if let Some((end, mut buffer)) = current.take() {
            if character == end {
                let value = buffer.trim().to_string();
                if !value.is_empty() {
                    quotes.push(value);
                }
            } else {
                buffer.push(character);
                current = Some((end, buffer));
            }
            continue;
        }
        let closing = PAIRS
            .iter()
            .find_map(|(start, end)| (*start == character).then_some(*end));
        match closing {
            Some(end) => current = Some((end, String::new())),
            None => rest.push(character),
        }
    }
    // 未闭合的引号：内容还回叙述，不丢字
    if let Some((_, buffer)) = current {
        rest.push_str(&buffer);
    }
    (quotes, rest.trim().to_string())
}

/// 该角色的章节上下文。
#[derive(Debug, Default, Clone)]
pub struct VoiceContext {
    /// 该角色自己的台词（引号内文本优先）
    pub lines: Vec<String>,
    /// 与自己台词同段的叙述——这是「说话人标签」，直接指认本人
    /// （例：*一位中年男子…语气漠然的将之公布了出来*）
    pub self_narration: Vec<String>,
    /// 其它提到该角色的叙述（别名也算）
    pub about_narration: Vec<String>,
}

impl VoiceContext {
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn narration_count(&self) -> usize {
        self.self_narration.len() + self.about_narration.len()
    }

    /// 送进提示词的叙述：说话人标签优先于泛泛提及。
    pub fn narration(&self) -> Vec<String> {
        let mut all = self.self_narration.clone();
        all.extend(self.about_narration.iter().cloned());
        all
    }
}

/// 按角色 id + 名字/别名，从全书分段里装配上下文。
///
/// 关键区分：**角色自己说的话 != 对角色自己的描写**。
/// 台词里提到的是别人（「对着少女略微恭声道」「薰儿小姐…」），
/// 所以必须把「与自己台词同段的叙述」和「自己的台词」分开存放，
/// 否则拿台词去推性别/年龄必然出错。
pub fn assemble(
    character_id: &str,
    name: &str,
    aliases: &[String],
    segments: &[Segment],
) -> VoiceContext {
    let mut keys: Vec<String> = vec![name.trim().to_string()];
    keys.extend(
        aliases
            .iter()
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty()),
    );
    keys.retain(|key| !key.is_empty());

    let mut context = VoiceContext::default();
    for segment in segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        let own = segment.character_id.as_deref() == Some(character_id);
        let mentioned = !own && keys.iter().any(|key| text.contains(key.as_str()));
        if !own && !mentioned {
            continue;
        }
        let (quotes, rest) = split_quotes(text);
        if own {
            if quotes.is_empty() {
                // 没有引号的对话段：整段就是台词
                push_unique(&mut context.lines, truncate(text, MAX_TEXT), MAX_LINES);
            } else {
                for quote in quotes {
                    push_unique(&mut context.lines, truncate(&quote, MAX_TEXT), MAX_LINES);
                }
            }
            if rest.chars().count() >= 6 {
                push_unique(
                    &mut context.self_narration,
                    truncate(&rest, MAX_TEXT),
                    MAX_NARRATION,
                );
            }
        } else if rest.chars().count() >= 6 {
            push_unique(
                &mut context.about_narration,
                truncate(&rest, MAX_TEXT),
                MAX_NARRATION,
            );
        } else {
            push_unique(
                &mut context.about_narration,
                truncate(text, MAX_TEXT),
                MAX_NARRATION,
            );
        }
    }
    context
}

/// 依据章节上下文给出音色描述草稿（纯本地、无网络）。
///
/// 只写"文中有据"的结论；推断内容一律声明，不编造音色。
pub fn compose_draft(
    name: &str,
    aliases: &[String],
    gender: Option<&str>,
    age_timeline: Option<&str>,
    notes: Option<&str>,
    context: &VoiceContext,
) -> String {
    let anchors = anchor_keys(name, aliases);

    // 身份证据：能"指认本人"的文本。名字/别名是最强的自述证据，
    // 叙述次之。**刻意不含台词**——角色嘴里说的是别人。
    let mut identity = anchors.join("\n");
    identity.push('\n');
    for item in context
        .self_narration
        .iter()
        .chain(context.about_narration.iter())
    {
        identity.push_str(item);
        identity.push('\n');
    }

    // 作风证据：音色质感与语气，只取"与他直接相关"的文本
    // ——说话人标签（self_narration）和他自己说的话。
    // 不取 about_narration：那里面写的是别人的动作，套到他头上就是错的。
    let mut manner = anchors.join("\n");
    manner.push('\n');
    for item in context.self_narration.iter().chain(context.lines.iter()) {
        manner.push_str(item);
        manner.push('\n');
    }

    let gender_hint = resolve_gender(gender, &anchors, &identity);
    let age_hint = resolve_age(age_timeline, &anchors, &identity);
    let subject = subject_clause(
        age_hint.as_ref().map(|(label, _)| label.as_str()),
        gender_hint.as_deref(),
    );
    let timbre = cue_labels_near(&manner, &anchors, TIMBRE_CUES, 2);
    let tone = cue_labels_near(&manner, &anchors, TONE_CUES, 3);

    let mut parts = vec![subject];
    if !timbre.is_empty() {
        parts.push(format!("音色偏{}", timbre.join("、")));
    }
    if !tone.is_empty() {
        parts.push(format!("语气{}", tone.join("、")));
    }
    parts.push(pace_clause(&context.lines));
    if let Some(notes) = notes.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(format!("角色备注：{}", truncate(notes, 40)));
    }

    let mut draft = format!("{}。", parts.join("；"));
    // 说不准的东西必须说出来，不能让"自动填充"看起来像已经确认过。
    let mut caveats = Vec::new();
    if timbre.is_empty() {
        caveats.push("文中未直接描写嗓音质感".to_string());
    }
    if matches!(
        age_hint.as_ref().map(|(_, source)| source),
        Some(AgeSource::PlaceholderFallback)
    ) {
        caveats.push("年龄阶段按默认值处理、未能从文中确定".to_string());
    }
    if gender_hint.is_none() {
        caveats.push("文中未能确定性别，请先确认再合成".to_string());
    }
    if !caveats.is_empty() {
        draft.push_str(&format!(
            "（依据 {} 条台词、{} 段叙述自动生成：{}。可点「AI 根据角色生成」精修。）",
            context.line_count(),
            context.narration_count(),
            caveats.join("；"),
        ));
    }
    draft
}

/// 名字 + 别名，作为"指认本人"的锚点。
fn anchor_keys(name: &str, aliases: &[String]) -> Vec<String> {
    let mut keys = vec![name.trim().to_string()];
    keys.extend(
        aliases
            .iter()
            .map(|alias| alias.trim().to_string())
            .filter(|alias| !alias.is_empty()),
    );
    keys.retain(|key| !key.is_empty());
    keys
}

/// 性别取证：人标注的字段 → 名字/别名自述 → 叙述证据（须无冲突）。
///
/// 逐层降级，第一层有结论就停——避免把"文里出现过的女性称谓"算成角色本人。
fn resolve_gender(stated: Option<&str>, anchors: &[String], identity: &str) -> Option<String> {
    stated
        .and_then(normalize_gender)
        .or_else(|| unanimous_cue(&anchors.join("\n"), GENDER_CUES))
        .or_else(|| unanimous_cue(identity, GENDER_CUES))
}

/// 年龄结论的来源，用于在草稿里说清"这个年龄是文里读出来的，还是默认值"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeSource {
    /// 人写的阶段字段
    Stated,
    /// 名字/别名或叙述里的文本证据
    Text,
    /// 谁都推断不出来，按导入占位值 `adult` 兜底
    PlaceholderFallback,
}

/// 年龄取证：人写的阶段 → 名字/别名自述 → 叙述证据（须无冲突）→ 占位值兜底。
///
/// 占位值（`adult`）是导入时给**每个**角色统一写下的，所以排在文本证据之后；
/// 但它也不是垃圾数据，谁都推断不出来时仍按「成年」使用，
/// 只是要标成 [`AgeSource::PlaceholderFallback`]，让草稿如实说明"这是默认值"。
fn resolve_age(
    stated: Option<&str>,
    anchors: &[String],
    identity: &str,
) -> Option<(String, AgeSource)> {
    let stated = stated.map(str::trim).filter(|value| !value.is_empty());
    if let Some(value) = stated.filter(|value| !is_placeholder_age_stage(value)) {
        if let Some(label) = age_stage_label(value) {
            return Some((label, AgeSource::Stated));
        }
    }
    if let Some(label) = unanimous_cue(&anchors.join("\n"), AGE_CUES) {
        return Some((label, AgeSource::Text));
    }
    if let Some(label) = unanimous_cue(identity, AGE_CUES) {
        return Some((label, AgeSource::Text));
    }
    stated
        .and_then(age_stage_label)
        .map(|label| (label, AgeSource::PlaceholderFallback))
}

/// 只有当全部命中都指向同一个标签时才给结论。
///
/// 中文小说里同一段叙述常同时写到多个人
/// （「面对着少女毫不掩饰的坦率话语，少年尴尬的笑了一声」「少女顿下了脚步，对着萧炎恭敬的弯了弯腰」），
/// 靠词频无法判断哪个词说的是谁。**性别猜错等于音色直接选错**，
/// 所以宁可交白卷让用户或 AI 补，也不能给出一个错的。
fn unanimous_cue(corpus: &str, table: &[(&str, &str)]) -> Option<String> {
    let mut found: Vec<String> = Vec::new();
    for (needle, label) in table {
        if corpus.contains(needle) && !found.iter().any(|value| value == label) {
            found.push((*label).to_string());
        }
    }
    if found.len() == 1 {
        found.pop()
    } else {
        None
    }
}

fn subject_clause(age: Option<&str>, gender: Option<&str>) -> String {
    match (age, gender) {
        (Some(age), Some(gender)) => format!("{age}{gender}性角色"),
        (Some(age), None) => format!("{age}角色"),
        (None, Some(gender)) => format!("{gender}性角色"),
        (None, None) => "角色".to_string(),
    }
}

/// 依据台词长度判断语速节奏。台词越短越像短促宣告，越长越像从容铺陈。
fn pace_clause(lines: &[String]) -> String {
    if lines.is_empty() {
        return "语速节奏：无台词样本，按常规中速处理".to_string();
    }
    let total: usize = lines
        .iter()
        .map(|line| line.chars().filter(|character| !is_punctuation(*character)).count())
        .sum();
    let average = total / lines.len().max(1);
    let pace = if average <= 10 {
        "语速中偏快、吐字干脆"
    } else if average >= 22 {
        "语速偏缓、气息绵长"
    } else {
        "语速中等、节奏平稳"
    };
    format!("{pace}（台词平均 {average} 字，共 {} 条）", lines.len())
}

fn is_punctuation(character: char) -> bool {
    matches!(
        character,
        '，' | '。' | '！' | '？' | '、' | '；' | '：' | '…' | '—' | '“' | '”' | '「' | '」'
            | '『' | '』' | '"' | '\'' | ',' | '.' | '!' | '?' | ';' | ':'
    )
}

/// 带"就近加权"的线索打分。
///
/// 中文里指认本人的词几乎总是紧挨着名字（「一位中年男子」「萧薰儿微笑着柔声道」），
/// 而说到别人时名字不在附近（「对着少女略微恭声道」）。
/// 所以靠近名字/别名出现的线索记 [`NEAR_WEIGHT`] 分，其它只记 1 分。
fn cue_labels_near(
    corpus: &str,
    anchors: &[String],
    table: &[(&str, &str)],
    limit: usize,
) -> Vec<String> {
    let chars: Vec<char> = corpus.chars().collect();
    let spans = anchor_spans(&chars, anchors);
    let mut hits: Vec<(usize, usize, String)> = Vec::new();
    for (index, (needle, label)) in table.iter().enumerate() {
        let needle_chars: Vec<char> = needle.chars().collect();
        let positions = char_positions(&chars, &needle_chars);
        if positions.is_empty() {
            continue;
        }
        let mut score = 0usize;
        for position in positions {
            let end = position + needle_chars.len();
            let near = spans
                .iter()
                .any(|(start, stop)| gap(position, end, *start, *stop) <= PROXIMITY_WINDOW);
            score += if near { NEAR_WEIGHT } else { 1 };
        }
        if let Some(existing) = hits.iter_mut().find(|(_, _, value)| value == label) {
            existing.0 += score;
        } else {
            hits.push((score, index, (*label).to_string()));
        }
    }
    hits.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    hits.into_iter().take(limit).map(|(_, _, label)| label).collect()
}

/// 名字/别名在文本里出现的所有区间（按字符下标）。
fn anchor_spans(chars: &[char], anchors: &[String]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for anchor in anchors {
        let needle: Vec<char> = anchor.chars().collect();
        let length = needle.len();
        for start in char_positions(chars, &needle) {
            spans.push((start, start + length));
        }
    }
    spans
}

fn char_positions(haystack: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    (0..=(haystack.len() - needle.len()))
        .filter(|start| haystack[*start..*start + needle.len()] == *needle)
        .collect()
}

/// 两个区间之间隔了多少字符；相交则为 0。
fn gap(left_start: usize, left_end: usize, right_start: usize, right_end: usize) -> usize {
    if left_end <= right_start {
        right_start - left_end
    } else if right_end <= left_start {
        left_start - right_end
    } else {
        0
    }
}

fn push_unique(target: &mut Vec<String>, value: String, limit: usize) {
    if value.is_empty() || target.len() >= limit {
        return;
    }
    if target.iter().any(|existing| existing == &value) {
        return;
    }
    target.push(value);
}

fn truncate(value: &str, limit: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let kept: String = trimmed.chars().take(limit).collect();
    format!("{kept}…")
}
