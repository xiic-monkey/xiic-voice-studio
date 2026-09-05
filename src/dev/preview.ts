import type { Chapter, Segment, StudioSnapshot } from "../types";

function isDevPreview() {
  return typeof window !== "undefined" && new URLSearchParams(window.location.search).has("mock");
}

function createMockChapters(): Chapter[] {
  const titles = [
    "陨落的天才",
    "斗气大陆",
    "客人",
    "云岚宗",
    "聚气散",
    "炼药师",
    "休！",
    "神秘的老者",
    "药老",
    "借钱",
    "坊市",
    "离他远点",
    "墨铁片",
    "虔诚的道歉",
  ];
  return Array.from({ length: 1623 }, (_, index) => {
    const number = index + 1;
    const title = titles[index % titles.length] ?? "无名章节";
    return {
      id: `mock-chapter-${number}`,
      title: `第${number}章 ${title}`,
      orderIndex: index,
      scriptStatus: number % 5 === 0 ? "marked" : "imported",
      rawText: `第${number}章的原文内容……（开发预览数据）`,
    };
  });
}

function createMockSegments(chapterId: string): Segment[] {
  return (
    [
      { text: "斗之力，三段！", segmentType: "transition", speaker: "旁白" },
      { text: "望着测验魔石碑上面闪亮得甚至有些刺眼的五个大字，少年面无表情。", segmentType: "narration", speaker: "旁白" },
      { text: "萧炎，斗之力，三段。级别：低级！", segmentType: "dialogue", speaker: "测验员" },
      { text: "成功的背后，究竟隐藏着多少艰辛？", segmentType: "inner_monologue", speaker: "萧炎" },
    ] as const
  ).map((item, index) => ({
    id: `mock-segment-${index}`,
    chapterId,
    orderIndex: index,
    text: item.text,
    segmentType: item.segmentType,
    speaker: item.speaker,
    audioStatus: "missing",
    reviewStatus: "unreviewed",
    isManualEdit: false,
  }));
}

/** `?mock=1` 时的浏览器预览数据，方便不带桌面后端调试 UI。 */
export function createMockSnapshot(): StudioSnapshot {
  const chapters = createMockChapters();
  return {
    project: {
      manifest: {
        title: "斗破苍穹（开发预览）",
        author: "天蚕土豆",
        language: "zh",
        productionType: "audiobook",
      },
      rootPath: "/Users/demo/VoiceProjects/斗破苍穹",
      chapterCount: chapters.length,
      segmentCount: 4,
      characterCount: 2,
    },
    chapters,
    segments: createMockSegments(chapters[0].id),
    characters: [
      { id: "mock-char-1", canonicalName: "萧炎", aliases: [], defaultColor: "#276ef1" },
      { id: "mock-char-2", canonicalName: "药老", aliases: ["药尘"], defaultColor: "#35a766" },
    ],
    voiceProfiles: [
      {
        id: "mock-voice-1",
        name: "Mimo 旁白声音",
        ageStage: "adult",
        ttsProvider: "mimo",
        voiceId: "mock-narrator",
        speed: 1,
        pitch: 1,
      },
    ],
    voiceAssets: [],
    reviewIssues: [],
    jobs: [
      { id: "job-1", jobType: "tts_batch", status: "running", progress: 0.2 },
      { id: "job-2", jobType: "tts_batch", status: "pending", progress: 0 },
      { id: "job-3", jobType: "mark_chapter", status: "failed", progress: 0, error: "LLM 标注响应不是有效 JSON：EOF while parsing a value at line 1 column 0" },
      { id: "job-4", jobType: "tts_batch", status: "failed", progress: 0, error: "语音生成失败：请先在设置中保存 Mimo API Key" },
      { id: "job-5", jobType: "tts_batch", status: "canceled", progress: 0, error: "用户已取消" },
      { id: "job-6", jobType: "tts_batch", status: "succeeded", progress: 1 },
      { id: "job-7", jobType: "tts_batch", status: "succeeded", progress: 1 },
      { id: "job-8", jobType: "mark_chapter", status: "succeeded", progress: 1 },
    ],
  };
}

export const devPreviewEnabled = isDevPreview();
