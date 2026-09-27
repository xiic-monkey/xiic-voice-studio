import { useMemo, useRef, useState } from "react";
import {
  AudioLines,
  Check,
  ChevronDown,
  Download,
  FileAudio,
  FileText,
  ListFilter,
  Merge,
  Package,
  Play,
  Plus,
  RefreshCw,
  Scissors,
  Search,
  ShieldCheck,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import type { Segment, SegmentType, StudioSnapshot, Character } from "../types";
import { segmentTypeLabels, segmentTypes, text } from "../constants";
import type { SegmentEditorController } from "../hooks/useSegmentEditor";
import { useEscapeKey } from "../hooks/useEscapeKey";
import { SegmentStatus } from "./ui";
import { VirtualList } from "./VirtualList";

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
  /** 当前章节 id。为空 = 没选中章节，此时空表不该给出"添加第一句"。 */
  activeChapterId?: string;
  segments: Segment[];
  selectedSegmentId: string;
  chapterPreviewOpen: boolean;
  onCloseChapterPreview: () => void;
  onSelectSegment: (id: string) => void;
  editor: SegmentEditorController;
  onSaveSegmentDraft: (segment: Segment) => void;
  onDeleteSegment: (segment: Segment) => void;
  onSplitSegment: (segment: Segment, offset: number) => void;
  onMergeSegments: (ids: string[]) => void;
  onInsertSegment: (
    /** null = 插到章首（空章节的「添加第一句」）。 */
    afterSegment: Segment | null,
    payload: { text: string; segmentType: SegmentType; characterId?: string; speaker?: string },
  ) => void;
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
  activeChapterId,
  segments,
  selectedSegmentId,
  chapterPreviewOpen,
  onCloseChapterPreview,
  onSelectSegment,
  editor,
  onSaveSegmentDraft,
  onDeleteSegment,
  onSplitSegment,
  onMergeSegments,
  onInsertSegment,
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
        <div className="surface-title">
          <h1 title={activeChapterTitle ?? undefined}>{activeChapterTitle ?? text.scriptWorkspace}</h1>
          <span>
            {snapshot
              ? `${snapshot.project.segmentCount}${text.segmentUnit} · ${snapshot.project.characterCount}${text.characterUnit}`
              : text.localWorkbench}
          </span>
        </div>
        <div className="surface-actions">
          <ExportMenu
            chapters={chapters}
            exportScope={exportScope}
            disabled={!snapshot}
            onExport={onExport}
            onExportEpisode={onExportEpisode}
            onCheckProduction={onCheckProduction}
          />
        </div>
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
        activeChapterId={activeChapterId}
        selectedSegmentId={selectedSegmentId}
        characters={snapshot?.characters ?? []}
        onSelectSegment={onSelectSegment}
        editor={editor}
        onSaveSegmentDraft={onSaveSegmentDraft}
        onDeleteSegment={onDeleteSegment}
        onSplitSegment={onSplitSegment}
        onMergeSegments={onMergeSegments}
        onInsertSegment={onInsertSegment}
        onPlay={onPlay}
        onUpload={onUpload}
        onRegenerate={onRegenerate}
      />
    </section>
  );
}

type ScopeProps = {
  chapters: ChapterLite[];
  exportScope: ExportScopeProps;
  disabled: boolean;
  onExport: (command: string, label: string) => void;
  onExportEpisode: () => void;
  onCheckProduction: () => void;
};

/**
 * 顶栏导出菜单：导出范围（全书 / 自选章节）和各导出目标收在同一处，
 * 不再在分段表上方常驻一条范围栏。选择章节弹层支持搜索和 Shift 连选。
 */
function ExportMenu({ chapters, exportScope, disabled, onExport, onExportEpisode, onCheckProduction }: ScopeProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [search, setSearch] = useState("");
  const lastCheckedIndexRef = useRef<number | null>(null);

  // Esc 逐层收起：先关章节选择弹层，再关导出菜单
  useEscapeKey(menuOpen, () => (pickerOpen ? closePicker() : closeMenu()));

  const filteredChapters = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return chapters;
    return chapters.filter((chapter) => chapter.title.toLowerCase().includes(query));
  }, [chapters, search]);

  const selectedIdSet = useMemo(() => new Set(exportScope.selectedIds), [exportScope.selectedIds]);
  const isSelected = (chapter: ChapterLite) => exportScope.allChapters || selectedIdSet.has(chapter.id);

  const summary = exportScope.allChapters
    ? `全书 · ${chapters.length} 章`
    : `已选 ${exportScope.selectedIds.length} / ${chapters.length} 章`;

  function handleCheck(chapter: ChapterLite, index: number, shiftKey: boolean) {
    const anchor = lastCheckedIndexRef.current;
    if (shiftKey && anchor !== null && anchor !== index) {
      const [from, to] = anchor < index ? [anchor, index] : [index, anchor];
      const rangeIds = filteredChapters.slice(from, to + 1).map((item) => item.id);
      const base = exportScope.allChapters ? chapters.map((item) => item.id) : exportScope.selectedIds;
      exportScope.setAll(false, Array.from(new Set([...base, ...rangeIds])));
      lastCheckedIndexRef.current = index;
      return;
    }
    exportScope.toggle(chapter.id);
    lastCheckedIndexRef.current = index;
  }

  function closePicker() {
    setPickerOpen(false);
    setSearch("");
    lastCheckedIndexRef.current = null;
  }

  function closeMenu() {
    setMenuOpen(false);
    closePicker();
  }

  /** 菜单项点击后先收起菜单再执行导出 */
  function run(action: () => void) {
    return () => {
      closeMenu();
      action();
    };
  }

  const scopeSection = (
    <div className="export-menu-scope">
      <span className="export-menu-label">导出范围</span>
      <div className="scope-pills">
        <button
          className={exportScope.allChapters ? "scope-pill selected" : "scope-pill"}
          onClick={() => exportScope.setAll(true, chapters.map((chapter) => chapter.id))}
        >
          {exportScope.allChapters && <Check size={13} />}
          全书
        </button>
        <div className="scope-picker-anchor">
          <button
            className={!exportScope.allChapters && exportScope.selectedIds.length ? "scope-pill selected" : "scope-pill"}
            onClick={() => (pickerOpen ? closePicker() : setPickerOpen(true))}
          >
            <ListFilter size={13} />
            选择章节
            {!exportScope.allChapters && exportScope.selectedIds.length > 0 && (
              <span className="scope-pill-count">{exportScope.selectedIds.length}</span>
            )}
          </button>
          {pickerOpen && (
            <>
              <div className="popover-overlay above-menu" onClick={closePicker} />
              <div className="chapter-picker">
                <div className="chapter-picker-toolbar">
                  <div className="chapter-picker-search">
                    <Search size={14} />
                    <input
                      value={search}
                      onChange={(event) => setSearch(event.target.value)}
                      placeholder="搜索章节标题"
                      autoFocus
                    />
                  </div>
                  <button
                    className="ghost compact"
                    onClick={() => exportScope.setAll(false, filteredChapters.map((chapter) => chapter.id))}
                  >
                    选中结果
                  </button>
                  <button className="ghost compact" onClick={() => exportScope.setAll(true, [])}>
                    全书
                  </button>
                </div>
                <VirtualList
                  className="chapter-picker-list"
                  items={filteredChapters}
                  rowHeight={32}
                  resetKey={search}
                  emptyState={<p className="empty">没有匹配的章节</p>}
                  renderItem={(chapter, index) => (
                    <label className="chapter-picker-row" key={chapter.id} title={chapter.title}>
                      <input
                        type="checkbox"
                        checked={isSelected(chapter)}
                        onChange={(event) =>
                          handleCheck(chapter, index, event.nativeEvent instanceof MouseEvent && event.nativeEvent.shiftKey)
                        }
                      />
                      <span>{chapter.title}</span>
                    </label>
                  )}
                />
                <div className="chapter-picker-footer">
                  <span>{summary}</span>
                  <button className="primary-action compact" onClick={closePicker}>
                    完成
                  </button>
                </div>
              </div>
            </>
          )}
        </div>
        <span className="export-menu-summary">{summary}</span>
      </div>
    </div>
  );

  return (
    <div className="export-menu-anchor">
      <button
        className="surface-action primary-action"
        onClick={() => (menuOpen ? closeMenu() : setMenuOpen(true))}
        disabled={disabled}
      >
        <Download size={16} />
        导出
        <ChevronDown size={14} className={menuOpen ? "flip" : undefined} />
      </button>
      {menuOpen && (
        <>
          <div className="popover-overlay" onClick={closeMenu} />
          <div className="export-menu">
            {scopeSection}
            <div className="export-menu-items">
              <button onClick={run(() => onExport("export_voice_script", text.exportVoiceScript))}>
                <FileText size={16} />
                <span className="export-menu-item-main">
                  <strong>{text.script}</strong>
                  <small>导出标注完成的配音文稿</small>
                </span>
              </button>
              <button onClick={run(() => onExport("export_segment_audio", text.exportSegmentAudio))}>
                <FileAudio size={16} />
                <span className="export-menu-item-main">
                  <strong>{text.segmentAudio}</strong>
                  <small>按范围导出各分段音频</small>
                </span>
              </button>
              <button onClick={run(onExportEpisode)}>
                <AudioLines size={16} />
                <span className="export-menu-item-main">
                  <strong>{text.episode}</strong>
                  <small>合并为一个整集音频文件</small>
                </span>
              </button>
              <button onClick={run(() => onExport("export_production_package", "导出制作包"))}>
                <Package size={16} />
                <span className="export-menu-item-main">
                  <strong>{text.package}</strong>
                  <small>打包脚本、音频与制作清单</small>
                </span>
              </button>
              <button onClick={run(onCheckProduction)}>
                <ShieldCheck size={16} />
                <span className="export-menu-item-main">
                  <strong>{text.productionCheck}</strong>
                  <small>检查缺失音频与审听状态</small>
                </span>
              </button>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

type RowProps = {
  segments: Segment[];
  activeChapterId?: string;
  selectedSegmentId: string;
  characters: Character[];
  onSelectSegment: (id: string) => void;
  editor: SegmentEditorController;
  onSaveSegmentDraft: (segment: Segment) => void;
  onDeleteSegment: (segment: Segment) => void;
  onSplitSegment: (segment: Segment, offset: number) => void;
  onMergeSegments: (ids: string[]) => void;
  onInsertSegment: (
    /** null = 插到章首（空章节的「添加第一句」）。 */
    afterSegment: Segment | null,
    payload: { text: string; segmentType: SegmentType; characterId?: string; speaker?: string },
  ) => void;
  onPlay: (segment: Segment) => void;
  onUpload: (segment: Segment) => void;
  onRegenerate: (segment: Segment) => void;
};

/** 说话人色点：优先按角色归属取角色色，再按说话人名匹配，无归属用中性色 */
function speakerDotColor(segment: Segment, characters: Character[]): string | null {
  return (
    characters.find((character) => character.id === segment.characterId)?.defaultColor ??
    (segment.speaker
      ? characters.find((character) => character.canonicalName === segment.speaker)?.defaultColor ?? null
      : null)
  );
}

/** 没有光标时选一个合理的默认拆分点：优先取最后一个句末标点之后，否则取中段。 */
function defaultSplitOffset(value: string): number {
  const marks = ["。", "！", "？", "；", "，"];
  let last = -1;
  for (const mark of marks) {
    const index = value.lastIndexOf(mark);
    if (index > last) last = index;
  }
  if (last >= 0 && last < value.length - 1) return last + 1;
  return Math.max(1, Math.floor(value.length / 2));
}

/** 两行式分段表 + 分段纠错工具（拆分 / 合并下一句 / 插入一句）。 */
function SegmentTable({
  segments,
  activeChapterId,
  selectedSegmentId,
  characters,
  onSelectSegment,
  editor,
  onSaveSegmentDraft,
  onDeleteSegment,
  onSplitSegment,
  onMergeSegments,
  onInsertSegment,
  onPlay,
  onUpload,
  onRegenerate,
}: RowProps) {
  const caretRef = useRef<Record<string, number>>({});
  const [insertingId, setInsertingId] = useState<string | null>(null);
  const [insertText, setInsertText] = useState("");
  const [insertType, setInsertType] = useState<SegmentType>("narration");
  // 空章节的插入框：章里没有分段时，行内的「+ 插入」无处可点，
  // 这是"删完之后加不回来"的那个真空档。
  const [emptyInsertOpen, setEmptyInsertOpen] = useState(false);

  function handleSplit(segment: Segment) {
    const draft = editor.draftFor(segment);
    const offset = caretRef.current[segment.id] ?? defaultSplitOffset(draft.text);
    const run = async () => {
      if (editor.drafts[segment.id]) {
        await onSaveSegmentDraft(segment);
      }
      onSplitSegment(segment, offset);
    };
    run();
  }

  function handleMergeNext(segment: Segment, index: number) {
    const next = segments[index + 1];
    if (!next) return;
    onMergeSegments([segment.id, next.id]);
  }

  function openInsert(segment: Segment) {
    setInsertType(segment.segmentType);
    setInsertText("");
    setInsertingId(segment.id);
  }

  function confirmInsert(segment: Segment) {
    const value = insertText.trim();
    if (!value) {
      setInsertingId(null);
      return;
    }
    const characterId = insertType === "dialogue" ? segment.characterId ?? undefined : undefined;
    const speaker = insertType === "dialogue" ? segment.speaker ?? undefined : undefined;
    onInsertSegment(segment, { text: value, segmentType: insertType, characterId, speaker });
    setInsertingId(null);
  }

  /** 空章节的第一句：没有"上一段"可指，直接插到章首，所以不继承任何说话人。 */
  function confirmEmptyInsert() {
    const value = insertText.trim();
    setEmptyInsertOpen(false);
    if (!value) return;
    onInsertSegment(null, { text: value, segmentType: insertType });
  }

  return (
    <div className="segment-table">
      <div className="segment-row segment-head">
        <span className="segment-index">{text.index}</span>
        <span className="segment-type">{text.type}</span>
        <span className="segment-char">{text.character}</span>
        <span className="segment-status">{text.status}</span>
        <span className="segment-actions">{text.tools}</span>
      </div>
      {segments.map((segment, index) => {
        const draft = editor.draftFor(segment);
        const classes = segment.id === selectedSegmentId ? "segment-row active" : "segment-row";
        const isInserting = insertingId === segment.id;
        const isLast = index >= segments.length - 1;
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
            <div className="segment-char-box">
              <span
                className="speaker-dot"
                style={{ background: speakerDotColor(segment, characters) ?? "var(--border-strong)" }}
              />
              <select
                className="segment-char"
                value={draft.characterId ?? ""}
                onClick={(event) => event.stopPropagation()}
                onChange={(event) => editor.update(segment, { characterId: event.target.value })}
                onBlur={() => onSaveSegmentDraft(segment)}
              >
              <option value="">{text.narrator}</option>
              {characters.map((character) => (
                <option value={character.id} key={character.id}>
                  {character.canonicalName}
                </option>
              ))}
              </select>
            </div>
            <span className="segment-status">
              <SegmentStatus audioStatus={segment.audioStatus} reviewStatus={segment.reviewStatus} />
            </span>
            <div className="segment-actions">
              {/* 结构编辑：改的是这一段怎么拆、怎么合，不是它的声音 */}
              <button
                title="从光标处拆分此分段（Ctrl/⌘+Enter）"
                onClick={(event) => { event.stopPropagation(); handleSplit(segment); }}
              >
                <Scissors size={13} />
              </button>
              <button
                title="与下一句合并"
                disabled={isLast}
                onClick={(event) => { event.stopPropagation(); handleMergeNext(segment, index); }}
              >
                <Merge size={13} />
              </button>
              <button
                title="在下方插入一句"
                onClick={(event) => { event.stopPropagation(); openInsert(segment); }}
              >
                <Plus size={13} />
              </button>
              <span className="segment-actions-divider" aria-hidden="true" />
              {/* 音频操作 */}
              <button title={text.playLatestAudio} onClick={(event) => { event.stopPropagation(); onPlay(segment); }}>
                <Play size={13} />
              </button>
              <button title={text.uploadSegmentAudio} onClick={(event) => { event.stopPropagation(); onUpload(segment); }}>
                <Upload size={13} />
              </button>
              <button title={text.regenerateSegment} onClick={(event) => { event.stopPropagation(); onRegenerate(segment); }}>
                <RefreshCw size={13} />
              </button>
              <button
                title="删除分段"
                onClick={(event) => { event.stopPropagation(); onDeleteSegment(segment); }}
              >
                <Trash2 size={13} />
              </button>
            </div>
            <div className="segment-text-box">
              {/* 两个框都常驻标签：填了内容之后，光靠 placeholder 就分不清谁是谁了 */}
              <div className="segment-field">
                <span className="segment-field-label">{text.content}</span>
                <textarea
                  className="segment-text"
                  value={draft.text}
                  onChange={(event) => editor.update(segment, { text: event.target.value })}
                  onClick={(event) => {
                    event.stopPropagation();
                    caretRef.current[segment.id] = event.currentTarget.selectionStart;
                  }}
                  onKeyUp={(event) => {
                    caretRef.current[segment.id] = event.currentTarget.selectionStart;
                  }}
                  onSelect={(event) => {
                    caretRef.current[segment.id] = event.currentTarget.selectionStart;
                  }}
                  onBlur={() => onSaveSegmentDraft(segment)}
                  onKeyDown={(event) => {
                    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                      event.preventDefault();
                      caretRef.current[segment.id] = event.currentTarget.selectionStart;
                      handleSplit(segment);
                    }
                  }}
                />
              </div>
              <div className="segment-field">
                <span className="segment-field-label">{text.emotion}</span>
                <input
                  className="segment-emotion"
                  value={draft.emotion ?? ""}
                  onChange={(event) => editor.update(segment, { emotion: event.target.value })}
                  onBlur={() => onSaveSegmentDraft(segment)}
                  placeholder={text.emotionPlaceholder}
                  onClick={(event) => event.stopPropagation()}
                />
              </div>
              {isInserting && (
                <div className="segment-insert-box" onClick={(event) => event.stopPropagation()}>
                  <textarea
                    className="segment-text"
                    autoFocus
                    placeholder="输入要插入的台词 / 旁白 / 音效"
                    value={insertText}
                    onChange={(event) => setInsertText(event.target.value)}
                  />
                  <div className="segment-insert-bar">
                    <select value={insertType} onChange={(event) => setInsertType(event.target.value as SegmentType)}>
                      {segmentTypes.map((type) => (
                        <option value={type} key={type}>
                          {segmentTypeLabels[type]}
                        </option>
                      ))}
                    </select>
                    <button className="primary-action compact" onClick={() => confirmInsert(segment)}>
                      插入
                    </button>
                    <button className="ghost compact" onClick={() => setInsertingId(null)}>
                      取消
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
        );
      })}
      {!segments.length && (
        <div className="empty-table">
          <p className="empty-table-text">{text.emptySegments}</p>
          {/* 没有活跃章节就没有"章首"可插，此时不给按钮，免得点了没反应 */}
          {!!activeChapterId &&
            (emptyInsertOpen ? (
              <div className="segment-insert-box empty-insert-box">
                <textarea
                  className="segment-text"
                  autoFocus
                  placeholder={text.addFirstSegmentPlaceholder}
                  value={insertText}
                  onChange={(event) => setInsertText(event.target.value)}
                />
                <div className="segment-insert-bar">
                  <select
                    value={insertType}
                    onChange={(event) => setInsertType(event.target.value as SegmentType)}
                  >
                    {segmentTypes.map((type) => (
                      <option value={type} key={type}>
                        {segmentTypeLabels[type]}
                      </option>
                    ))}
                  </select>
                  <button className="primary-action compact" onClick={confirmEmptyInsert}>
                    {text.addSegment}
                  </button>
                  <button className="ghost compact" onClick={() => setEmptyInsertOpen(false)}>
                    取消
                  </button>
                </div>
              </div>
            ) : (
              <button
                className="primary-action compact"
                onClick={() => {
                  setInsertText("");
                  setInsertType("narration");
                  setEmptyInsertOpen(true);
                }}
              >
                <Plus size={14} />
                {text.addFirstSegment}
              </button>
            ))}
        </div>
      )}
    </div>
  );
}
