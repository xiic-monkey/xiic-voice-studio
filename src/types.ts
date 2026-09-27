export type SegmentType =
  | "narration"
  | "dialogue"
  | "inner_monologue"
  | "sound_cue"
  | "transition"
  | "timing_anchor"
  | "unknown";

export type Chapter = {
  id: string;
  title: string;
  orderIndex: number;
  scriptStatus: string;
  rawText: string;
};

export type Segment = {
  id: string;
  chapterId: string;
  orderIndex: number;
  text: string;
  segmentType: SegmentType;
  speaker?: string;
  characterId?: string;
  emotion?: string;
  soundCue?: string;
  anchor?: string;
  audioStatus: string;
  reviewStatus: string;
  isManualEdit: boolean;
};

export type SegmentDraft = {
  text: string;
  segmentType: SegmentType;
  speaker: string;
  /** 绑定的角色；空字符串表示旁白 */
  characterId?: string;
  /** 台词情绪，由标注给出、可人工调整 */
  emotion?: string;
};

export type Character = {
  id: string;
  canonicalName: string;
  aliases: string[];
  gender?: string;
  ageTimeline?: string;
  notes?: string;
  defaultColor: string;
};

export type VoiceProfile = {
  id: string;
  characterId?: string;
  name: string;
  ageStage: string;
  ttsProvider: string;
  model?: string;
  voiceId: string;
  voiceAssetId?: string;
  speed: number;
  pitch: number;
  style?: string;
  /** 系统自动兜底的默认音色（尚未由人确认）。用户一旦设定即变为 false。 */
  isDefault: boolean;
};

export type VoiceAsset = {
  id: string;
  projectId: string;
  name: string;
  assetType: string;
  provider: string;
  model: string;
  relativePath: string;
  mimeType: string;
  sourceFileName: string;
  consentConfirmed: boolean;
  status: string;
};

export type ReviewIssue = {
  id: string;
  segmentId?: string;
  audioId?: string;
  issueType: string;
  note: string;
  status: string;
  createdAt: string;
};

export type VoiceInfo = {
  provider: string;
  voiceId: string;
  name: string;
  language: string;
  tags: string[];
};

export type StudioJob = {
  id: string;
  jobType: string;
  status: string;
  progress: number;
  error?: string;
  payloadJson?: string;
  createdAt?: string;
  updatedAt?: string;
};

export type ProjectSummary = {
  manifest: {
    title: string;
    author?: string;
    language: string;
    productionType: string;
  };
  rootPath: string;
  chapterCount: number;
  segmentCount: number;
  characterCount: number;
};

export type StudioSnapshot = {
  project: ProjectSummary;
  chapters: Chapter[];
  segments: Segment[];
  characters: Character[];
  voiceProfiles: VoiceProfile[];
  voiceAssets: VoiceAsset[];
  /** 项目旁白音色档案 id，由后端按统一口径给出；前端不要从列表顺序猜。 */
  narratorProfileId?: string;
  reviewIssues: ReviewIssue[];
  jobs: StudioJob[];
  /** 已删除但仍可撤销的分段，超过保留期才会被清理。 */
  deletedSegments: DeletedSegment[];
};

/** 一条删除归档记录。完整的原行数据留在后端，这里只有列表需要的字段。 */
export type DeletedSegment = {
  id: string;
  projectId: string;
  chapterId: string;
  /** 删除时占的序号（0 基），撤销时按它插回原位。 */
  positionIndex: number;
  textPreview: string;
  deletedAt: string;
};

export type SplitRuleDto = {
  id: string;
  label: string;
  pattern: string;
  description: string;
};

/** 导入前拆章预览：章节数、样例标题、实际命中的标题行、规则来源。 */
export type SplitPreview = {
  chapterCount: number;
  sampleTitles: string[];
  matchedLines: string[];
  /** "pattern" 给定正则 / "heuristic" 启发式 / "single" 未拆分 */
  ruleSource: string;
};

export type ProviderSettings = {
  provider: string;
  apiKey?: string;
  endpoint?: string;
  model?: string;
};

export type VoiceStability = "clone" | "preset" | "design" | "none";

export type AppSettings = {
  schemaVersion: number;
  llm: { baseUrl: string; model: string };
  tts: {
    provider: string;
    endpoint: string;
    model: string;
    voiceId: string;
    stylePrompt: string;
  };
  audio: { ffmpegPath: string; episodeFormat: string };
  /** 保存时必须原样带回，否则会重置记住的项目路径 */
  workspace: { lastProjectRoot?: string | null };
};

export type SettingsSection = "llm" | "tts" | "audio" | "export";
export type VoiceCenterSection = "library" | "clone";
export type CheckState = { kind: "idle" | "running" | "success" | "error"; message: string };
export type ReviewIssueType = "performance" | "pronunciation" | "timing" | "audio_quality";

export type ProductionCheckReport = {
  generatedAt: string;
  canPublish: boolean;
  totalSegments: number;
  readySegments: number;
  approvedSegments: number;
  missingAudio: number;
  rejectedSegments: number;
  unreviewedSegments: number;
  totalDurationMs: number;
  issues: Array<{
    severity: string;
    segmentId: string;
    orderIndex: number;
    speaker?: string;
    message: string;
  }>;
};

export type AssetValidationReport = {
  valid: boolean;
  checkedAssets: number;
  missingAssets: number;
  issues: { assetType: string; relativePath: string; message: string }[];
};

/** 角色在章节里的上下文概览 + 本地草稿（工坊打开时自动填充用） */
export type CharacterVoiceContext = {
  draft: string;
  lineCount: number;
  narrationCount: number;
  sampleLines: string[];
};

/** AI 精修音色描述的结果；source=draft 表示回退到了章节上下文草稿 */
export type VoiceDescriptionResult = {
  description: string;
  source: "llm" | "draft";
  warning?: string | null;
  lineCount: number;
  narrationCount: number;
};
