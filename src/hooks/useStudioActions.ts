import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import type {
  Chapter,
  ProductionCheckReport,
  ReviewIssueType,
  Segment,
  SegmentDraft,
  StudioJob,
  StudioSnapshot,
} from "../types";
import { desktopRuntimeMessage, providerDefaults, text } from "../constants";
import { invoke } from "../utils";
import type { RunTask } from "./useNotifier";
import type { AppSettingsController } from "./useAppSettings";
import type { ProjectSnapshotController } from "./useProjectSnapshot";
import type { SegmentEditorController } from "./useSegmentEditor";

type Params = {
  desktopRuntime: boolean;
  run: RunTask;
  setNotice: (message: string) => void;
  project: ProjectSnapshotController;
  settings: AppSettingsController;
  editor: SegmentEditorController;
  setAudioPath: (path: string) => void;
  setProductionReport: (report: ProductionCheckReport | null) => void;
};

/**
 * 工作台的全部业务动作：项目生命周期、分段保存、TTS 任务、
 * 审听、导出与项目资产管理。组件层只调用，不感知 invoke 细节。
 */
export function useStudioActions({
  desktopRuntime,
  run,
  setNotice,
  project,
  settings,
  editor,
  setAudioPath,
  setProductionReport,
}: Params) {
  const [projectTitle, setProjectTitle] = useState("新有声项目");
  const [author, setAuthor] = useState("");
  const [reviewIssueType, setReviewIssueType] = useState<ReviewIssueType>("performance");
  const [reviewNote, setReviewNote] = useState(text.reviewNote);
  const [audioOutputPath, setAudioOutputPath] = useState("");
  const [projectCoverPath, setProjectCoverPath] = useState("");

  const rootPath = project.snapshot?.project.rootPath;

  useEffect(() => {
    if (!rootPath) {
      setAudioOutputPath("");
      setProjectCoverPath("");
      return;
    }
    invoke<string>("get_audio_output_directory")
      .then(setAudioOutputPath)
      .catch(() => setAudioOutputPath(""));
    invoke<string | null>("get_project_cover")
      .then((path) => setProjectCoverPath(path ?? ""))
      .catch(() => setProjectCoverPath(""));
  }, [rootPath]);

  async function createProject() {
    if (!desktopRuntime) {
      setNotice(desktopRuntimeMessage);
      return;
    }
    const root = await save({ title: text.createProjectDialog, defaultPath: projectTitle });
    if (!root) return;
    await run(
      text.createProject,
      () =>
        invoke<StudioSnapshot>("create_project", {
          request: { rootPath: root, title: projectTitle, author: author || undefined },
        }),
      project.hydrateSnapshot,
    );
  }

  async function openProject() {
    if (!desktopRuntime) {
      setNotice(desktopRuntimeMessage);
      return;
    }
    const root = await open({ directory: true, multiple: false, title: text.openProjectDialog });
    if (!root || Array.isArray(root)) return;
    await run(text.openingProject, () => invoke<StudioSnapshot>("open_project", { rootPath: root }), project.hydrateSnapshot);
  }

  async function importSource() {
    if (!desktopRuntime) {
      setNotice(desktopRuntimeMessage);
      return;
    }
    const sourcePath = await open({
      multiple: false,
      filters: [{ name: text.sourceFilter, extensions: ["txt", "docx"] }],
    });
    if (!sourcePath || Array.isArray(sourcePath)) return;
    await run(
      text.importingSource,
      () => invoke<StudioSnapshot>("import_source", { request: { sourcePath } }),
      project.hydrateSnapshot,
    );
  }

  async function deleteChapter(chapter: Chapter) {
    if (!desktopRuntime || !project.snapshot) return;
    if (!window.confirm(`确定删除“${chapter.title}”吗？章节下的分段、音频和审听记录也会被删除。`)) return;
    await run("删除章节", () => invoke<StudioSnapshot>("delete_chapter", { chapterId: chapter.id }), project.hydrateSnapshot);
  }

  async function markChapter() {
    if (!project.activeChapterId) return;
    await run(
      text.markingChapter,
      () =>
        invoke<StudioSnapshot>("mark_chapter", {
          chapterId: project.activeChapterId,
          settings: {
            baseUrl: settings.llm.baseUrl,
            apiKey: settings.llm.apiKey || undefined,
            model: settings.llm.model,
          },
        }),
      project.hydrateSnapshot,
    );
  }

  async function saveSegment(segment: Segment, patch: Partial<Segment>) {
    const speaker = Object.prototype.hasOwnProperty.call(patch, "speaker")
      ? patch.speaker?.trim() || undefined
      : segment.speaker;
    const submittedDraft: SegmentDraft = {
      text: patch.text ?? segment.text,
      segmentType: patch.segmentType ?? segment.segmentType,
      speaker: speaker ?? "",
    };
    const saved = await run(
      text.savingSegment,
      () =>
        invoke<StudioSnapshot>("update_segment", {
          request: {
            segmentId: segment.id,
            text: patch.text ?? segment.text,
            segmentType: patch.segmentType ?? segment.segmentType,
            speaker,
            emotion: patch.emotion ?? segment.emotion,
            soundCue: patch.soundCue ?? segment.soundCue,
            anchor: patch.anchor ?? segment.anchor,
          },
        }),
      project.hydrateSnapshot,
    );
    if (saved) editor.drop(segment.id, submittedDraft);
  }

  async function saveSegmentDraft(segment: Segment) {
    const draft = editor.drafts[segment.id];
    if (!draft) return;
    await saveSegment(segment, { text: draft.text, segmentType: draft.segmentType, speaker: draft.speaker });
  }

  async function deleteSegment(segment: Segment) {
    if (!window.confirm(`删除分段 ${segment.orderIndex + 1}？其音频记录和审听备注会一并删除，磁盘上的音频文件也会清理。`)) return;
    const saved = await run(
      "删除分段",
      () => invoke<StudioSnapshot>("delete_segment", { segmentId: segment.id }),
      project.hydrateSnapshot,
    );
    if (saved) editor.drop(segment.id, editor.draftFor(segment));
  }

  async function assignVoice(character?: { id: string; canonicalName: string }) {
    const defaults = providerDefaults[settings.tts.provider] ?? providerDefaults.mock;
    await run(
      text.assigningVoice,
      () =>
        invoke<StudioSnapshot>("assign_voice_profile", {
          request: {
            characterId: character?.id,
            name: character
              ? `${character.canonicalName}${settings.tts.provider === "mimo" ? text.mimoVoice : text.adultVoice}`
              : text.narratorVoice,
            ageStage: "adult",
            ttsProvider: settings.tts.provider,
            model: settings.tts.model || undefined,
            voiceId: settings.tts.voiceId || defaults.voiceId,
            voiceAssetId: undefined,
            speed: 1,
            pitch: 0,
            style: settings.tts.provider === "mimo" ? settings.tts.stylePrompt : character ? "dialogue" : "narration",
          },
        }),
      project.hydrateSnapshot,
    );
  }

  async function generateTts(segmentIds: string[], forceRegenerate = false) {
    const value = await invoke<StudioSnapshot>("enqueue_tts_batch", {
      request: { segmentIds, settings: settings.ttsSettings(), forceRegenerate },
    }).catch((error) => {
      setNotice(error instanceof Error ? error.message : String(error));
      return undefined;
    });
    if (value) {
      project.hydrateSnapshot(value);
      setNotice(text.queued);
    }
  }

  async function cancelJob(jobId: string) {
    await run(
      text.cancelJob,
      () => invoke<StudioJob[]>("cancel_job", { jobId }),
      (jobs) => project.setSegmentJobs(jobs),
    );
  }

  async function retryJob(jobId: string) {
    await run(
      text.retryJob,
      () => invoke<StudioJob[]>("retry_job", { jobId }),
      (jobs) => project.setSegmentJobs(jobs),
    );
  }

  async function deleteJob(jobId: string) {
    await run(
      "删除任务记录",
      () => invoke<StudioJob[]>("delete_job", { jobId }),
      (jobs) => project.setSegmentJobs(jobs),
    );
  }

  async function clearFinishedJobs() {
    await run(
      "清空已结束任务",
      () => invoke<StudioJob[]>("clear_finished_jobs"),
      (jobs) => project.setSegmentJobs(jobs),
    );
  }

  async function playAudio(segment?: Segment) {
    if (!segment) return;
    const path = await run(text.loadingAudio, () =>
      invoke<string | null>("play_segment_audio", { segmentId: segment.id }),
    );
    if (path) setAudioPath(path);
  }

  async function uploadAudio(segment?: Segment) {
    if (!segment) return;
    if (!desktopRuntime) {
      setNotice(desktopRuntimeMessage);
      return;
    }
    const sourcePath = await open({
      multiple: false,
      filters: [{ name: "音频文件", extensions: ["wav", "mp3", "m4a", "aac", "flac", "ogg"] }],
    });
    if (!sourcePath || Array.isArray(sourcePath)) return;
    await run(
      text.uploadingAudio,
      () =>
        invoke<StudioSnapshot>("upload_segment_audio", {
          request: {
            segmentId: segment.id,
            sourcePath,
            ffmpegPath: settings.audio.ffmpegPath || undefined,
          },
        }),
      project.hydrateSnapshot,
    );
  }

  async function setAudioReviewStatus(status: "approved" | "rejected") {
    if (!project.selectedSegment) return;
    await run(
      status === "approved" ? text.approvingAudio : text.rejectingAudio,
      () =>
        invoke<StudioSnapshot>("set_segment_audio_status", {
          request: { segmentId: project.selectedSegment!.id, status },
        }),
      project.hydrateSnapshot,
    );
  }

  async function addReviewIssue() {
    if (!project.selectedSegment) return;
    if (!reviewNote.trim()) {
      setNotice("请填写审听备注");
      return;
    }
    await run(
      text.addingReviewIssue,
      () =>
        invoke<StudioSnapshot>("review_segment_audio", {
          request: {
            segmentId: project.selectedSegment!.id,
            issueType: reviewIssueType,
            note: reviewNote.trim(),
          },
        }),
      project.hydrateSnapshot,
    );
  }

  async function exportItem(command: string, label: string) {
    if (!project.exportScope.selectedIds.length) {
      setNotice("至少选择一个章节后再导出");
      return;
    }
    const output = await run(label, () => invoke<string>(command, { chapterIds: project.exportScope.args() }));
    if (output) setNotice(`${label}: ${output}`);
  }

  async function checkProductionReadiness() {
    if (!project.exportScope.selectedIds.length) {
      setNotice("至少选择一个章节后再检查发布");
      return;
    }
    await run(
      text.checkingProduction,
      () => invoke<ProductionCheckReport>("check_production_readiness", { chapterIds: project.exportScope.args() }),
      (report) => {
        setProductionReport(report);
        setNotice(
          `${text.checkingProduction}: ${report.canPublish ? text.publishReady : text.publishBlocked} · ${report.issues.length} 个问题`,
        );
      },
    );
  }

  async function exportEpisode() {
    if (!project.exportScope.selectedIds.length) {
      setNotice("至少选择一个章节后再导出");
      return;
    }
    const output = await run(text.exportingEpisode, () =>
      invoke<string>("export_episode", {
        ffmpegPath: settings.audio.ffmpegPath || undefined,
        chapterIds: project.exportScope.args(),
        format: settings.audio.episodeFormat,
      }),
    );
    if (output) setNotice(`${text.exportedEpisode}: ${output}`);
  }

  async function chooseProjectCover() {
    if (!desktopRuntime) {
      setNotice(desktopRuntimeMessage);
      return;
    }
    const sourcePath = await open({
      multiple: false,
      title: "选择项目封面",
      filters: [{ name: "图片封面", extensions: ["jpg", "jpeg", "png"] }],
    });
    if (!sourcePath || Array.isArray(sourcePath)) return;
    const saved = await run(text.chooseCover, () => invoke<string>("set_project_cover", { sourcePath }));
    if (saved) {
      setProjectCoverPath(saved);
      setNotice(text.coverSaved);
    }
  }

  async function checkProjectAssets() {
    const report = await run(text.checkingAssets, () => invoke<{ valid: boolean; checkedAssets: number; missingAssets: number }>("check_project_assets"));
    if (report) {
      setNotice(report.valid ? `${text.assetsValid}：${report.checkedAssets} 项` : `发现 ${report.missingAssets} 个缺失资产`);
    }
  }

  async function backupProject() {
    const output = await run(text.backingUpProject, () => invoke<string>("export_project_backup"));
    if (output) setNotice(`${text.backupReady}：${output}`);
  }

  async function openAudioOutputFolder() {
    if (!audioOutputPath) return;
    await run(text.openAudioFolder, () => openPath(audioOutputPath));
  }

  return {
    projectTitle,
    setProjectTitle,
    author,
    setAuthor,
    createProject,
    openProject,
    importSource,
    deleteChapter,
    markChapter,
    saveSegment,
    saveSegmentDraft,
    deleteSegment,
    assignVoice,
    generateTts,
    cancelJob,
    retryJob,
    deleteJob,
    clearFinishedJobs,
    playAudio,
    uploadAudio,
    setAudioReviewStatus,
    addReviewIssue,
    review: {
      issueType: reviewIssueType,
      setIssueType: setReviewIssueType,
      note: reviewNote,
      setNote: setReviewNote,
    },
    exportItem,
    checkProductionReadiness,
    exportEpisode,
    chooseProjectCover,
    checkProjectAssets,
    backupProject,
    openAudioOutputFolder,
    audioOutputPath,
    projectCoverPath,
  };
}

export type StudioActionsController = ReturnType<typeof useStudioActions>;
