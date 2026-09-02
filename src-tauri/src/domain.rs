use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub language: String,
    pub production_type: String,
    pub schema_version: i64,
    pub app_version: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub manifest: ProjectManifest,
    pub root_path: String,
    pub chapter_count: i64,
    pub segment_count: i64,
    pub character_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub order_index: i64,
    pub raw_text: String,
    pub script_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub id: String,
    pub chapter_id: String,
    pub order_index: i64,
    pub description: String,
    pub time_location: Option<String>,
    pub mood: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentType {
    Narration,
    Dialogue,
    InnerMonologue,
    SoundCue,
    Transition,
    TimingAnchor,
    Unknown,
}

impl SegmentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SegmentType::Narration => "narration",
            SegmentType::Dialogue => "dialogue",
            SegmentType::InnerMonologue => "inner_monologue",
            SegmentType::SoundCue => "sound_cue",
            SegmentType::Transition => "transition",
            SegmentType::TimingAnchor => "timing_anchor",
            SegmentType::Unknown => "unknown",
        }
    }
}

impl From<&str> for SegmentType {
    fn from(value: &str) -> Self {
        match value {
            "narration" => SegmentType::Narration,
            "dialogue" => SegmentType::Dialogue,
            "inner_monologue" => SegmentType::InnerMonologue,
            "sound_cue" => SegmentType::SoundCue,
            "transition" => SegmentType::Transition,
            "timing_anchor" => SegmentType::TimingAnchor,
            _ => SegmentType::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: String,
    pub chapter_id: String,
    pub scene_id: Option<String>,
    pub order_index: i64,
    pub text: String,
    pub segment_type: SegmentType,
    pub speaker: Option<String>,
    pub character_id: Option<String>,
    pub emotion: Option<String>,
    pub sound_cue: Option<String>,
    pub anchor: Option<String>,
    pub voice_profile_id: Option<String>,
    pub audio_status: String,
    pub review_status: String,
    pub age_progress: Option<f64>,
    pub is_manual_edit: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub id: String,
    pub project_id: String,
    pub canonical_name: String,
    pub aliases: Vec<String>,
    pub gender: Option<String>,
    pub age_timeline: Option<String>,
    pub notes: Option<String>,
    pub default_color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceProfile {
    pub id: String,
    pub project_id: String,
    pub character_id: Option<String>,
    pub name: String,
    pub age_stage: String,
    pub tts_provider: String,
    pub model: Option<String>,
    pub voice_id: String,
    pub voice_asset_id: Option<String>,
    pub speed: f64,
    pub pitch: f64,
    pub style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceAsset {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub asset_type: String,
    pub provider: String,
    pub model: String,
    pub relative_path: String,
    pub mime_type: String,
    pub source_file_name: String,
    pub consent_confirmed: bool,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceBatch {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub scope: String,
    pub status: String,
    pub parameters_json: String,
    pub context_window: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentAudio {
    pub id: String,
    pub segment_id: String,
    pub relative_path: String,
    pub duration_ms: Option<i64>,
    pub loudness_lufs: Option<f64>,
    pub version: i64,
    pub source: String,
    pub status: String,
    pub created_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewIssue {
    pub id: String,
    pub segment_id: Option<String>,
    pub audio_id: Option<String>,
    pub issue_type: String,
    pub note: String,
    pub status: String,
    pub created_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportJob {
    pub id: String,
    pub project_id: String,
    pub target_format: String,
    pub selected_chapters_json: String,
    pub audio_mix_settings_json: String,
    pub status: String,
    pub output_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioJob {
    pub id: String,
    pub project_id: String,
    pub job_type: String,
    pub status: String,
    pub progress: f64,
    pub payload_json: String,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioSnapshot {
    pub project: ProjectSummary,
    pub chapters: Vec<Chapter>,
    pub segments: Vec<Segment>,
    pub characters: Vec<Character>,
    pub voice_profiles: Vec<VoiceProfile>,
    pub voice_assets: Vec<VoiceAsset>,
    pub review_issues: Vec<ReviewIssue>,
    pub jobs: Vec<StudioJob>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceInfo {
    pub provider: String,
    pub voice_id: String,
    pub name: String,
    pub language: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionCheckReport {
    pub generated_at: String,
    pub can_publish: bool,
    pub total_segments: i64,
    pub ready_segments: i64,
    pub approved_segments: i64,
    pub missing_audio: i64,
    pub rejected_segments: i64,
    pub unreviewed_segments: i64,
    pub total_duration_ms: i64,
    pub issues: Vec<ProductionCheckIssue>,
    pub assets: Vec<ProductionAssetLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionCheckIssue {
    pub severity: String,
    pub segment_id: String,
    pub order_index: i64,
    pub speaker: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionAssetLine {
    pub segment_id: String,
    pub audio_id: Option<String>,
    pub order_index: i64,
    pub speaker: Option<String>,
    pub audio_status: String,
    pub review_status: String,
    pub source: Option<String>,
    pub relative_path: Option<String>,
    pub export_path: Option<String>,
    pub version: Option<i64>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductionPackageManifest {
    pub generated_at: String,
    pub project_id: String,
    pub project_title: String,
    pub app_version: String,
    pub schema_version: i64,
    pub total_segments: i64,
    pub total_duration_ms: i64,
    pub assets: Vec<ProductionAssetLine>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetValidationIssue {
    pub asset_type: String,
    pub relative_path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetValidationReport {
    pub valid: bool,
    pub checked_assets: i64,
    pub missing_assets: i64,
    pub issues: Vec<AssetValidationIssue>,
}
