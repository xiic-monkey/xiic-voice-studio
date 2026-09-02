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
  reviewIssues: ReviewIssue[];
  jobs: StudioJob[];
};

export type ProviderSettings = {
  provider: string;
  apiKey?: string;
  endpoint?: string;
  model?: string;
};

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
