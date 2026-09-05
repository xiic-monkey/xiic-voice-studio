import { useMemo, useState } from "react";
import { BookOpen, Eye, Save, Search, Settings, Trash2 } from "lucide-react";
import type { Chapter } from "../types";
import { text } from "../constants";
import { displayStatus } from "../utils";
import { Panel } from "./ui";
import { VirtualList } from "./VirtualList";

type Props = {
  chapters: Chapter[];
  activeChapterId: string;
  projectTitle: string;
  author: string;
  onProjectTitleChange: (value: string) => void;
  onAuthorChange: (value: string) => void;
  onCreateProject: () => void;
  onSelectChapter: (id: string) => void;
  onPreviewChapter: (chapter: Chapter) => void;
  onDeleteChapter: (chapter: Chapter) => void;
  onOpenSettings: () => void;
};

export function Sidebar({
  chapters,
  activeChapterId,
  projectTitle,
  author,
  onProjectTitleChange,
  onAuthorChange,
  onCreateProject,
  onSelectChapter,
  onPreviewChapter,
  onDeleteChapter,
  onOpenSettings,
}: Props) {
  const [search, setSearch] = useState("");

  const filteredChapters = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return chapters;
    return chapters.filter((chapter) => chapter.title.toLowerCase().includes(query));
  }, [chapters, search]);

  return (
    <aside className="sidebar">
      <Panel title={text.project} icon={<BookOpen size={16} />} className="create-panel">
        <input value={projectTitle} onChange={(event) => onProjectTitleChange(event.target.value)} />
        <input value={author} placeholder={text.author} onChange={(event) => onAuthorChange(event.target.value)} />
        <button className="primary-action" onClick={onCreateProject}>
          <Save size={16} />
          {text.createLocalProject}
        </button>
      </Panel>

      <Panel title={text.chapters} className="chapters-panel" count={`${chapters.length} 章`}>
        <div className="chapter-search">
          <Search size={14} />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="搜索章节"
          />
        </div>
        <VirtualList
          className="chapter-list"
          items={filteredChapters}
          rowHeight={36}
          resetKey={search}
          emptyState={<p className="empty">{chapters.length ? "没有匹配的章节" : text.emptyChapters}</p>}
          renderItem={(chapter) => (
            <div key={chapter.id} className={`chapter-item ${chapter.id === activeChapterId ? "selected" : ""}`}>
              <button className="chapter-select" title={chapter.title} onClick={() => onSelectChapter(chapter.id)}>
                <strong>{chapter.title}</strong>
              </button>
              <span className="chapter-meta">{displayStatus(chapter.scriptStatus)}</span>
              <div className="chapter-item-actions">
                <button className="icon-button compact" title="查看章节原文" onClick={() => onPreviewChapter(chapter)}>
                  <Eye size={14} />
                </button>
                <button className="icon-button compact" title="删除章节" onClick={() => onDeleteChapter(chapter)}>
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          )}
        />
      </Panel>

      <div className="sidebar-footer">
        <button className="settings-button" onClick={onOpenSettings}>
          <Settings size={16} />
          设置
        </button>
      </div>
    </aside>
  );
}
