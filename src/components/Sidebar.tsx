import { BookOpen, Eye, Save, Settings, Trash2 } from "lucide-react";
import type { Chapter } from "../types";
import { text } from "../constants";
import { displayStatus } from "../utils";
import { Panel } from "./ui";

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

      <Panel title={text.chapters} className="chapters-panel">
        <div className="chapter-list">
          {chapters.map((chapter) => (
            <div key={chapter.id} className={`chapter-item ${chapter.id === activeChapterId ? "selected" : ""}`}>
              <button className="chapter-select" onClick={() => onSelectChapter(chapter.id)}>
                <strong>{chapter.title}</strong>
                <small>{displayStatus(chapter.scriptStatus)}</small>
              </button>
              <div className="chapter-item-actions">
                <button title="查看章节原文" onClick={() => onPreviewChapter(chapter)}>
                  <Eye size={14} />
                </button>
                <button title="删除章节" onClick={() => onDeleteChapter(chapter)}>
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
          {!chapters.length && <p className="empty">{text.emptyChapters}</p>}
        </div>
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
