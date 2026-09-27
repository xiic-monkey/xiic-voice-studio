import type { Chapter, DeletedSegment, Segment, StudioSnapshot } from "../types";

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

/**
 * 预览里的可变分段与删除归档。
 *
 * 删除 / 撤销是**有状态**的流程，纯夹具函数表达不了
 * "删掉之后它不在列表里、点了撤销又回来"。所以这里留一份预览自己的状态，
 * 只服务于 `?mock=1` 下的 UI 调试，与真实项目数据无关。
 */
let mockSegments: Segment[] | null = null;
let mockArchived: { summary: DeletedSegment; segment: Segment }[] = [];

/** `?mock=1` 时的浏览器预览数据，方便不带桌面后端调试 UI。 */
export function createMockSnapshot(): StudioSnapshot {
  const chapters = createMockChapters();
  mockSegments ??= createMockSegments(chapters[0].id);
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
    // 每次返回当前引用：删除/撤销后是**新数组**，React 才会重渲染
    segments: mockSegments,
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
        isDefault: true,
      },
      {
        id: "mock-voice-2",
        characterId: "mock-char-1",
        name: "萧炎 声音",
        ageStage: "adult",
        ttsProvider: "mimo",
        model: "mimo-v2.5-tts-voicedesign",
        voiceId: "清亮略带沙哑的少年男声",
        speed: 1,
        pitch: 1,
        isDefault: true,
      },
      {
        id: "mock-voice-3",
        characterId: "mock-char-2",
        name: "药老 声音",
        ageStage: "elderly",
        ttsProvider: "mimo",
        model: "mimo-v2.5-tts-voiceclone",
        voiceId: "clone:mock-asset",
        voiceAssetId: "mock-asset-1",
        speed: 1,
        pitch: 1,
        isDefault: false,
      },
    ],
    narratorProfileId: "mock-voice-1",
    voiceAssets: [],
    reviewIssues: [],
    deletedSegments: mockArchived.map((item) => item.summary),
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

/**
 * 浏览器预览下的假后端。
 *
 * 浏览器里没有 Tauri，所有 invoke 都会失败——角色声音工坊因此完全无法预览。
 * 这里给"描述设计 → 试听 → 固化"这条链路上的命令提供夹具，
 * 让 `?mock=1` 能走通完整流程（不需要任何 API Key）。
 *
 * 返回 `undefined` 表示这条命令没有夹具，交给正常路径处理（即给出可读的运行时错误）。
 */
export function mockInvoke(command: string, args?: Record<string, unknown>): Promise<unknown> | undefined {
  if (!devPreviewEnabled) return undefined;
  switch (command) {
    case "character_voice_context":
      // 夹具刻意模仿真实输出：有据的结论 + 明确声明没把握的部分
      return delay({
        draft:
          "中年男性角色；语气漠然、客气；语速中等、节奏平稳（台词平均 13 字，共 8 条）。（依据 8 条台词、3 段叙述自动生成：文中未直接描写嗓音质感。可点「AI 根据角色生成」精修。）",
        lineCount: 8,
        narrationCount: 3,
        sampleLines: [
          "斗之力，三段！",
          "萧炎，斗之力，三段！级别：低级！",
          "下一个，萧媚！",
          "斗之气：七段！",
        ],
      });
    case "generate_voice_description":
      return delay({
        description:
          "中年男性，中低音区，声线偏干、略带沙哑；语气公事公办、漠然克制，念成绩时为短促宣告式，语速中等偏快，字尾收得干脆。",
        source: "llm",
        warning: null,
        lineCount: 8,
        narrationCount: 3,
      });
    case "generate_character_voice_sample":
      return delay({ assetId: "mock-designed-asset", audioPath: "" });
    case "finalize_character_voice":
      return delay({ ok: true });
    case "delete_segment": {
      const segmentId = String(args?.segmentId ?? "");
      const index = (mockSegments ?? []).findIndex((segment) => segment.id === segmentId);
      if (index < 0) return delay(Promise.reject(new Error("预览数据里没有这个分段")));
      const segment = (mockSegments ?? [])[index];
      // 用新数组而不是 splice：原地改数组引用不变，React 不会重渲染
      mockSegments = (mockSegments ?? []).filter((item) => item.id !== segmentId);
      mockArchived = [
        {
          segment,
          summary: {
            id: segment.id,
            projectId: "mock-project",
            chapterId: segment.chapterId,
            positionIndex: segment.orderIndex,
            textPreview: segment.text,
            deletedAt: new Date().toISOString(),
          },
        },
        ...mockArchived,
      ];
      return delay(createMockSnapshot());
    }
    case "restore_segment": {
      const segmentId = String(args?.segmentId ?? "");
      const archived = mockArchived.find((item) => item.summary.id === segmentId);
      if (!archived) return delay(Promise.reject(new Error("这条删除记录已经不在了")));
      mockArchived = mockArchived.filter((item) => item.summary.id !== segmentId);
      mockSegments = [...(mockSegments ?? []), archived.segment].sort(
        (left, right) => left.orderIndex - right.orderIndex,
      );
      return delay(createMockSnapshot());
    }
    default:
      void args;
      return undefined;
  }
}

/** 夹具也走一段延迟，让加载态/禁用态在预览里看得见。 */
function delay<T>(value: T, ms = 600): Promise<T> {
  return new Promise((resolve) => setTimeout(() => resolve(value), ms));
}
