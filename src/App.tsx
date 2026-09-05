import { useRef, useState } from "react";
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import type { Chapter, ProductionCheckReport, SettingsSection, VoiceCenterSection } from "./types";
import { desktopRuntimeMessage } from "./constants";

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
import { AudioPlayer } from "./components/AudioPlayer";

/**
 * 编排层：只负责把各域 hook 组装起来并下发给组件，
 * 不持有业务状态、不直接调用 invoke。
 */
function App() {
  const desktopRuntime = isTauri();
  const { busy, notice, setNotice, run } = useNotifier();

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
  const [productionReport, setProductionReport] = useState<ProductionCheckReport | null>(null);
  const [chapterPreviewOpen, setChapterPreviewOpen] = useState(false);

  const project = useProjectSnapshot(() => {
    editor.clear();
    characters.resetMerge();
    setAudioPath("");
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
    setProductionReport,
  });

  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("llm");
  const [voiceCenterOpen, setVoiceCenterOpen] = useState(false);

  function leaveSettings() {
    if (settings.settingsDirty && !window.confirm("有未保存的设置更改，确定放弃并返回工作台吗？")) return;
    setSettingsOpen(false);
  }

  function previewChapter(chapter: Chapter) {
    project.setSelectedChapterId(chapter.id);
    setChapterPreviewOpen(true);
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
  const audioSrc = audioPath ? convertFileSrc(audioPath) : undefined;

  return (
    <main className="studio-shell">
      <TopBar
        snapshot={project.snapshot}
        busy={busy}
        activeJobCount={project.activeJobs.length}
        canMark={Boolean(project.activeChapterId)}
        canGenerate={project.segments.length > 0}
        onOpenProject={actions.openProject}
        onImportSource={actions.importSource}
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
          segments={project.segments}
          selectedSegmentId={selectedSegment?.id ?? ""}
          chapterPreviewOpen={chapterPreviewOpen}
          onCloseChapterPreview={() => setChapterPreviewOpen(false)}
          onSelectSegment={project.setSelectedSegmentId}
          editor={editor}
          onSaveSegmentDraft={actions.saveSegmentDraft}
          onDeleteSegment={actions.deleteSegment}
          exportScope={{
            allChapters: project.exportScope.allChapters,
            selectedIds: project.exportScope.selectedIds,
            toggle: project.exportScope.toggle,
            setAll: project.exportScope.setAll,
          }}
          onExport={actions.exportItem}
          onCheckProduction={actions.checkProductionReadiness}
          onExportEpisode={actions.exportEpisode}
          onPlay={actions.playAudio}
          onUpload={actions.uploadAudio}
          onRegenerate={(segment) => actions.generateTts([segment.id], true)}
        />

        <aside className="inspector">
          <CharacterPanel
            snapshot={project.snapshot}
            characters={characters}
            onAssignVoice={actions.assignVoice}
          />
          <VoicePanel
            snapshot={project.snapshot}
            onAssignNarrator={() => actions.assignVoice(undefined)}
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
          <strong>{selectedSegment?.speaker || "旁白"}</strong>
          <span>{selectedSegment?.text ?? "未选择分段"}</span>
        </div>
        <AudioPlayer src={audioSrc} label="当前分段播放器" />
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
      />
    </main>
  );
}

export default App;
