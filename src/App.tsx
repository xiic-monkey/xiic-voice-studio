import { useEffect, useRef, useState } from "react";
import { Undo2 } from "lucide-react";
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import type { Chapter, ProductionCheckReport, Segment, SettingsSection, VoiceCenterSection } from "./types";
import { desktopRuntimeMessage, text } from "./constants";

import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/layout.css";
import "./styles/components.css";

import { useNotifier } from "./hooks/useNotifier";
import { useAppSettings } from "./hooks/useAppSettings";
import { useSegmentEditor } from "./hooks/useSegmentEditor";
import { useCharacters } from "./hooks/useCharacters";
import { useVoiceProfiles } from "./hooks/useVoiceProfiles";
import { useProjectSnapshot, type ProjectSnapshotController } from "./hooks/useProjectSnapshot";
import { useStudioActions } from "./hooks/useStudioActions";

import { TopBar } from "./components/TopBar";
import { Sidebar } from "./components/Sidebar";
import { ScriptSurface } from "./components/ScriptSurface";
import { CharacterPanel, JobPanel, ReviewPanel, VoicePanel } from "./components/Inspector";
import { SettingsView } from "./components/SettingsView";
import { VoiceCenter } from "./components/VoiceCenter";
import { ChapterSplitDialog } from "./components/ChapterSplitDialog";
import { CharacterVoiceDialog } from "./components/CharacterVoiceDialog";
import { AudioPlayer } from "./components/AudioPlayer";

/**
 * 编排层：只负责把各域 hook 组装起来并下发给组件，
 * 不持有业务状态、不直接调用 invoke。
 */
function App() {
  const desktopRuntime = isTauri();
  const { busy, notice, noticeTone, setNotice, undo, setUndo, clearUndo, run } = useNotifier();
  // 全局提示同时以浮层呈现：弹窗遮罩会盖住底部提示条，
  // 只提示在提示条里 = 用户点弹窗里的按钮失败时"看不到任何反应"。
  const [dismissedNotice, setDismissedNotice] = useState("");
  const toastVisible = notice !== text.ready && notice !== dismissedNotice;
  useEffect(() => {
    if (!toastVisible) return;
    // 带「撤销」按钮的提示要多留一会儿，否则用户还没反应过来按钮就没了
    const timer = setTimeout(() => {
      setDismissedNotice(notice);
      clearUndo();
    }, undo ? undo.timeoutMs : 9000);
    return () => clearTimeout(timer);
  }, [notice, toastVisible, undo, clearUndo]);

  const editor = useSegmentEditor();
  const settings = useAppSettings({ desktopRuntime, run, setNotice });

  // characters / voices 的 hydrate 需要转发给后面创建的 project hook，用 ref 解开创建顺序依赖。
  const projectRef = useRef<ProjectSnapshotController | null>(null);
  const hydrate = (value: Parameters<ProjectSnapshotController["hydrateSnapshot"]>[0]) =>
    projectRef.current?.hydrateSnapshot(value);

  const characters = useCharacters({ run, setNotice, hydrate });
  const [voiceCenterSection, setVoiceCenterSection] = useState<VoiceCenterSection>("library");
  const voices = useVoiceProfiles({
    desktopRuntime,
    run,
    setNotice,
    ttsSettings: () => settings.tts.settings,
    hydrate,
    onCloneCreated: () => setVoiceCenterSection("library"),
  });

  const [audioPath, setAudioPath] = useState("");
  // 递增即"立刻播放"：分段列表点播放按钮后，底部播放器直接出声
  const [audioPlaySignal, setAudioPlaySignal] = useState(0);
  // 底部播放器展示的是"正在播的那条"，不是"选中的那条"——两者经常不是同一个
  const [playingSegment, setPlayingSegment] = useState<Segment | null>(null);
  // 非空表示播放器里装的是"某角色的固化样本"，而不是分段音频
  const [playingSample, setPlayingSample] = useState("");
  const [productionReport, setProductionReport] = useState<ProductionCheckReport | null>(null);
  const [chapterPreviewOpen, setChapterPreviewOpen] = useState(false);

  const project = useProjectSnapshot(() => {
    editor.clear();
    characters.resetMerge();
    setAudioPath("");
    setPlayingSegment(null);
    setPlayingSample("");
    setProductionReport(null);
    setChapterPreviewOpen(false);
  });
  projectRef.current = project;

  const actions = useStudioActions({
    desktopRuntime,
    run,
    setNotice,
    project,
    settings,
    editor,
    setAudioPath,
    onRequestAudioPlayback: () => setAudioPlaySignal((value) => value + 1),
    setPlayingSample,
    setProductionReport,
    // 删除后的「撤销」直接挂在提示浮层上，不必再开一个弹窗
    onUndoable: (message, action) =>
      setUndo(message, { label: text.undoDelete, run: action }),
  });

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("llm");
  const [voiceCenterOpen, setVoiceCenterOpen] = useState(false);
  const [importDialogPath, setImportDialogPath] = useState<string | null>(null);
  const [voiceDesignCharacter, setVoiceDesignCharacter] = useState<{ id: string; name: string } | null>(null);

  /** 打开角色声音工坊：描述设计 → 试听 → 固化为该角色的音色 */
  function openVoiceDesign(characterId: string, characterName: string) {
    setVoiceDesignCharacter({ id: characterId, name: characterName });
  }

  // 启动时自动恢复上次打开的项目；只跑一次，失败静默。
  const restoreRef = useRef(false);
  useEffect(() => {
    if (restoreRef.current || desktopRuntime === false) return;
    restoreRef.current = true;
    void actions.openLastProject();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function beginImport() {
    const path = await actions.beginImport();
    if (path) setImportDialogPath(path);
  }

  function confirmImport(pattern: string | null) {
    const path = importDialogPath;
    setImportDialogPath(null);
    if (path) actions.importWithPattern(path, pattern);
  }

  function leaveSettings() {
    if (settings.settingsDirty && !window.confirm("有未保存的设置更改，确定放弃并返回工作台吗？")) return;
    setSettingsOpen(false);
  }

  function previewChapter(chapter: Chapter) {
    project.setSelectedChapterId(chapter.id);
    setChapterPreviewOpen(true);
  }

  /** 浮层提示：z-index 高于 modal-backdrop，弹窗里的失败也能被看到 */
  function topToast() {
    if (!toastVisible) return null;
    // undo 非空就一定是这条提示的（setNotice 会把上一枚撤销按钮清掉），
    // 所以不必再比对文案。
    const handle = undo;
    return (
      <div className={`app-toast ${noticeTone}`} role={noticeTone === "error" ? "alert" : "status"}>
        <span className="app-toast-text">{notice}</span>
        {handle && (
          <button
            className="ghost compact app-toast-undo"
            onClick={() => {
              clearUndo();
              setDismissedNotice(notice);
              void handle.run();
            }}
          >
            <Undo2 size={13} />
            {handle.label}
          </button>
        )}
        <button
          className="ghost compact"
          onClick={() => {
            clearUndo();
            setDismissedNotice(notice);
          }}
        >
          知道了
        </button>
      </div>
    );
  }

  if (settingsOpen) {
    return (
      <SettingsView
        desktopRuntime={desktopRuntime}
        runtimeMessage={desktopRuntimeMessage}
        section={settingsSection}
        onSectionChange={setSettingsSection}
        onLeave={leaveSettings}
        busy={busy}
        notice={notice}
        snapshot={project.snapshot}
        settings={settings}
        projectCoverPath={actions.projectCoverPath}
        audioOutputPath={actions.audioOutputPath}
        onChooseCover={actions.chooseProjectCover}
        onCheckAssets={actions.checkProjectAssets}
        onBackup={actions.backupProject}
        onOpenAudioFolder={actions.openAudioOutputFolder}
      />
    );
  }

  const activeChapter = project.activeChapter;
  const selectedSegment = project.selectedSegment;
  // 播放器里装的是哪条音频，就显示哪条；用户只是"选中"另一条时，播放器不该跟着变
  const playerSegment = playingSegment ?? selectedSegment;
  const audioSrc = audioPath ? convertFileSrc(audioPath) : undefined;
  const playerSpeaker = playingSample || playerSegment?.speaker || "旁白";
  const playerText = playingSample ? text.voiceSamplePlaying : playerSegment?.text ?? "未选择分段";

  return (
    <main className="studio-shell">
      <TopBar
        snapshot={project.snapshot}
        busy={busy}
        activeJobCount={project.activeJobs.length}
        canMark={Boolean(project.activeChapterId)}
        canGenerate={project.segments.length > 0}
        onOpenProject={actions.openProject}
        onImportSource={beginImport}
        onMarkChapter={actions.markChapter}
        onGenerateAll={() => actions.generateTts(project.segments.map((segment) => segment.id))}
      />

      <section className="workspace">
        <Sidebar
          chapters={project.chapters}
          activeChapterId={project.activeChapterId}
          projectTitle={actions.projectTitle}
          author={actions.author}
          onProjectTitleChange={actions.setProjectTitle}
          onAuthorChange={actions.setAuthor}
          onCreateProject={actions.createProject}
          onSelectChapter={project.setSelectedChapterId}
          onPreviewChapter={previewChapter}
          onDeleteChapter={actions.deleteChapter}
          onOpenSettings={() => setSettingsOpen(true)}
        />

        <ScriptSurface
          snapshot={project.snapshot}
          chapters={project.chapters}
          activeChapterTitle={activeChapter?.title}
          activeChapterRawText={activeChapter?.rawText}
          activeChapterId={activeChapter?.id}
          segments={project.segments}
          selectedSegmentId={selectedSegment?.id ?? ""}
          chapterPreviewOpen={chapterPreviewOpen}
          onCloseChapterPreview={() => setChapterPreviewOpen(false)}
          onSelectSegment={project.setSelectedSegmentId}
          editor={editor}
          onSaveSegmentDraft={actions.saveSegmentDraft}
          onDeleteSegment={actions.deleteSegment}
          onSplitSegment={actions.splitSegment}
          onMergeSegments={actions.mergeSegmentsAction}
          onInsertSegment={actions.insertSegmentAfter}
          exportScope={{
            allChapters: project.exportScope.allChapters,
            selectedIds: project.exportScope.selectedIds,
            toggle: project.exportScope.toggle,
            setAll: project.exportScope.setAll,
          }}
          onExport={actions.exportItem}
          onCheckProduction={actions.checkProductionReadiness}
          onExportEpisode={actions.exportEpisode}
          onPlay={(segment) => {
            setPlayingSample("");
            setPlayingSegment(segment);
            void actions.playAudio(segment);
          }}
          onUpload={actions.uploadAudio}
          onRegenerate={(segment) => actions.generateTts([segment.id], true)}
        />

        <aside className="inspector">
          <CharacterPanel
            snapshot={project.snapshot}
            characters={characters}
            busy={busy}
            onDesignVoice={openVoiceDesign}
            onPreviewVoiceSample={actions.previewVoiceAsset}
          />
          <VoicePanel
            snapshot={project.snapshot}
            busy={busy}
            onSetNarratorVoice={actions.setNarratorVoice}
            onOpenVoiceCenter={() => setVoiceCenterOpen(true)}
          />
          <ReviewPanel
            selectedSegmentText={selectedSegment?.text}
            hasSegment={Boolean(selectedSegment)}
            openIssues={project.selectedReviewIssues}
            review={{
              issueType: actions.review.issueType,
              setIssueType: actions.review.setIssueType,
              note: actions.review.note,
              setNote: actions.review.setNote,
            }}
            onApprove={() => actions.setAudioReviewStatus("approved")}
            onReject={() => actions.setAudioReviewStatus("rejected")}
            onAddIssue={actions.addReviewIssue}
          />
          <JobPanel
            snapshot={project.snapshot}
            productionReport={productionReport}
            onCancelJob={actions.cancelJob}
            onRetryJob={actions.retryJob}
            onDeleteJob={actions.deleteJob}
            onClearFinishedJobs={actions.clearFinishedJobs}
          />
        </aside>
      </section>

      <footer className="player">
        <div>
          <strong>{playerSpeaker}</strong>
          <span>{playerText}</span>
        </div>
        <AudioPlayer
          src={audioSrc}
          playSignal={audioPlaySignal}
          label="当前分段播放器"
          onPlaybackBlocked={() => setNotice("浏览器拦下了自动播放，请点一下播放器的播放键")}
        />
        <span className="notice">{notice}</span>
      </footer>

      <VoiceCenter
        open={voiceCenterOpen}
        section={voiceCenterSection}
        snapshot={project.snapshot}
        busy={busy}
        voices={voices}
        onClose={() => setVoiceCenterOpen(false)}
        onSwitchSection={setVoiceCenterSection}
        onDesignVoice={openVoiceDesign}
      />

      {voiceDesignCharacter && (
        <CharacterVoiceDialog
          characterId={voiceDesignCharacter.id}
          characterName={voiceDesignCharacter.name}
          snapshot={project.snapshot}
          ttsSettings={settings.tts.settings}
          llm={{
            baseUrl: settings.llm.baseUrl,
            model: settings.llm.model,
            apiKey: settings.llm.apiKey,
            keySaved: settings.llm.keySaved,
          }}
          ttsKeySaved={settings.tts.keySaved}
          busy={busy}
          onNotice={setNotice}
          onSaveProfile={async (characterId, draft) => {
            const outcome = await characters.saveCharacterProfile(characterId, draft);
            // 改名成功后同步弹窗标题用的名字，defaultSample 的兜底台词也跟着换
            if (outcome.ok) {
              setVoiceDesignCharacter((current) =>
                current && current.id === characterId ? { ...current, name: draft.canonicalName } : current,
              );
            }
            return outcome;
          }}
          onClose={() => setVoiceDesignCharacter(null)}
          onFinalized={() => {
            setVoiceDesignCharacter(null);
            setNotice("角色音色已固化（克隆模式），重新生成后生效");
          }}
        />
      )}

      {topToast()}

      {importDialogPath && (
        <ChapterSplitDialog
          sourcePath={importDialogPath}
          llm={{
            baseUrl: settings.llm.baseUrl,
            model: settings.llm.model,
            apiKey: settings.llm.apiKey,
            keySaved: settings.llm.keySaved,
          }}
          busy={busy}
          onNotice={setNotice}
          onClose={() => setImportDialogPath(null)}
          onConfirm={confirmImport}
        />
      )}
    </main>
  );
}

export default App;
