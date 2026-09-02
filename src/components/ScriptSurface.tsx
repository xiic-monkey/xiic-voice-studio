import { AudioLines, CheckCircle2, Download, FileAudio, Play, RefreshCw, Upload, X } from "lucide-react";
import type { Segment, SegmentType, StudioSnapshot } from "../types";
import { segmentTypeLabels, segmentTypes, text } from "../constants";
import type { SegmentEditorController } from "../hooks/useSegmentEditor";
import { SegmentStatus } from "./ui";

type ChapterLite = { id: string; title: string; rawText: string };

type ExportScopeProps = {
  allChapters: boolean;
  selectedIds: string[];
  toggle: (chapterId: string) => void;
  setAll: (all: boolean, ids: string[]) => void;
};

type Props = {
  snapshot: StudioSnapshot | null;
  chapters: ChapterLite[];
  activeChapterTitle?: string;
  activeChapterRawText?: string;
  segments: Segment[];
  selectedSegmentId: string;
  chapterPreviewOpen: boolean;
  onCloseChapterPreview: () => void;
  onSelectSegment: (id: string) => void;
  editor: SegmentEditorController;
  onSaveSegmentDraft: (segment: Segment) => void;
  exportScope: ExportScopeProps;
  onExport: (command: string, label: string) => void;
  onCheckProduction: () => void;
  onExportEpisode: () => void;
  onPlay: (segment: Segment) => void;
  onUpload: (segment: Segment) => void;
  onRegenerate: (segment: Segment) => void;
};

export function ScriptSurface({
  snapshot,
  chapters,
  activeChapterTitle,
  activeChapterRawText,
  segments,
  selectedSegmentId,
  chapterPreviewOpen,
  onCloseChapterPreview,
  onSelectSegment,
  editor,
  onSaveSegmentDraft,
  exportScope,
  onExport,
  onCheckProduction,
  onExportEpisode,
  onPlay,
  onUpload,
  onRegenerate,
}: Props) {
  return (
    <section className="script-surface">
      <div className="surface-header">
        <div>
          <h1>{activeChapterTitle ?? text.scriptWorkspace}</h1>
          <span>
            {snapshot
              ? `${snapshot.project.segmentCount}${text.segmentUnit} · ${snapshot.project.characterCount}${text.characterUnit}`
              : text.localWorkbench}
          </span>
        </div>
        <div className="surface-actions">
          <button className="surface-action" onClick={() => onExport("export_voice_script", text.exportVoiceScript)} disabled={!snapshot}>
            <Download size={16} />
            {text.script}
          </button>
          <button className="surface-action" onClick={() => onExport("export_segment_audio", text.exportSegmentAudio)} disabled={!snapshot}>
            <FileAudio size={16} />
            {text.segmentAudio}
          </button>
          <button className="surface-action" onClick={onCheckProduction} disabled={!snapshot}>
            <CheckCircle2 size={16} />
            {text.productionCheck}
          </button>
          <button className="surface-action primary-action" onClick={onExportEpisode} disabled={!snapshot}>
            <AudioLines size={16} />
            {text.episode}
          </button>
          <button className="surface-action" onClick={() => onExport("export_production_package", "导出制作包")} disabled={!snapshot}>
            <Download size={16} />
            {text.package}
          </button>
        </div>
      </div>

      <div className="export-scope" aria-label="导出章节范围">
        <span className="export-scope-label">导出范围</span>
        <label>
          <input
            type="checkbox"
            checked={exportScope.allChapters}
            onChange={(event) => exportScope.setAll(event.target.checked, chapters.map((chapter) => chapter.id))}
          />
          全书
        </label>
        {chapters.map((chapter) => (
          <label key={chapter.id} title={chapter.title}>
            <input
              type="checkbox"
              checked={exportScope.allChapters || exportScope.selectedIds.includes(chapter.id)}
              onChange={() => exportScope.toggle(chapter.id)}
            />
            <span>{chapter.title}</span>
          </label>
        ))}
        {!exportScope.selectedIds.length && <small>未选择章节</small>}
      </div>

      {chapterPreviewOpen && (
        <div className="chapter-preview">
          <div className="chapter-preview-header">
            <strong>章节原文</strong>
            <button title="关闭" onClick={onCloseChapterPreview}>
              <X size={15} />
            </button>
          </div>
          <div className="chapter-preview-body">{activeChapterRawText || "暂无原文"}</div>
        </div>
      )}

      <SegmentTable
        segments={segments}
        selectedSegmentId={selectedSegmentId}
        onSelectSegment={onSelectSegment}
        editor={editor}
        onSaveSegmentDraft={onSaveSegmentDraft}
        onPlay={onPlay}
        onUpload={onUpload}
        onRegenerate={onRegenerate}
      />
    </section>
  );
}

type RowProps = {
  segments: Segment[];
  selectedSegmentId: string;
  onSelectSegment: (id: string) => void;
  editor: SegmentEditorController;
  onSaveSegmentDraft: (segment: Segment) => void;
  onPlay: (segment: Segment) => void;
  onUpload: (segment: Segment) => void;
  onRegenerate: (segment: Segment) => void;
};

/** 两行式分段表：上排窄控件、下排全文，1080 最小窗口不再溢出。 */
function SegmentTable({
  segments,
  selectedSegmentId,
  onSelectSegment,
  editor,
  onSaveSegmentDraft,
  onPlay,
  onUpload,
  onRegenerate,
}: RowProps) {
  return (
    <div className="segment-table">
      <div className="segment-row segment-head">
        <span className="segment-index">{text.index}</span>
        <span className="segment-type">{text.type}</span>
        <span className="segment-speaker">{text.speaker}</span>
        <span className="segment-status">{text.status}</span>
        <span className="segment-actions">{text.tools}</span>
        <span className="segment-text">{text.content}</span>
      </div>
      {segments.map((segment) => {
        const draft = editor.draftFor(segment);
        const classes = segment.id === selectedSegmentId ? "segment-row active" : "segment-row";
        return (
          <div className={classes} key={segment.id} onClick={() => onSelectSegment(segment.id)}>
            <span className="segment-index">{segment.orderIndex + 1}</span>
            <select
              className="segment-type"
              value={draft.segmentType}
              onChange={(event) => editor.update(segment, { segmentType: event.target.value as SegmentType })}
              onBlur={() => onSaveSegmentDraft(segment)}
            >
              {segmentTypes.map((type) => (
                <option value={type} key={type}>
                  {segmentTypeLabels[type]}
                </option>
              ))}
            </select>
            <input
              className="segment-speaker"
              value={draft.speaker}
              onChange={(event) => editor.update(segment, { speaker: event.target.value })}
              onBlur={() => onSaveSegmentDraft(segment)}
              placeholder={text.narrator}
            />
            <span className="segment-status">
              <SegmentStatus audioStatus={segment.audioStatus} reviewStatus={segment.reviewStatus} />
            </span>
            <div className="segment-actions">
              <button title={text.playLatestAudio} onClick={(event) => { event.stopPropagation(); onPlay(segment); }}>
                <Play size={15} />
              </button>
              <button title={text.uploadSegmentAudio} onClick={(event) => { event.stopPropagation(); onUpload(segment); }}>
                <Upload size={15} />
              </button>
              <button title={text.regenerateSegment} onClick={(event) => { event.stopPropagation(); onRegenerate(segment); }}>
                <RefreshCw size={15} />
              </button>
            </div>
            <textarea
              className="segment-text"
              value={draft.text}
              onChange={(event) => editor.update(segment, { text: event.target.value })}
              onBlur={() => onSaveSegmentDraft(segment)}
            />
          </div>
        );
      })}
      {!segments.length && <div className="empty-table">{text.emptySegments}</div>}
    </div>
  );
}
