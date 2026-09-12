import { AudioLines, CheckCircle2, FileInput, FolderOpen, Loader2, Mic2, Wand2 } from "lucide-react";
import type { StudioSnapshot } from "../types";
import { text } from "../constants";

type Props = {
  snapshot: StudioSnapshot | null;
  busy: string;
  activeJobCount: number;
  canMark: boolean;
  canGenerate: boolean;
  onOpenProject: () => void;
  onImportSource: () => void;
  onMarkChapter: () => void;
  onGenerateAll: () => void;
};

export function TopBar({
  snapshot,
  busy,
  activeJobCount,
  canMark,
  canGenerate,
  onOpenProject,
  onImportSource,
  onMarkChapter,
  onGenerateAll,
}: Props) {
  return (
    <header className="topbar">
      <div className="brand">
        <AudioLines size={22} />
        <div>
          <strong>Xiic Voice Studio</strong>
          <span title={snapshot?.project.rootPath} className="brand-path">
            {snapshot ? `${snapshot.project.manifest.title} · ${snapshot.project.rootPath}` : text.noProject}
          </span>
        </div>
      </div>
      <div className="toolbar">
        <button title="打开项目" onClick={onOpenProject}>
          <FolderOpen size={17} />
          {text.open}
        </button>
        <button title="导入稿件" onClick={onImportSource} disabled={!snapshot}>
          <FileInput size={17} />
          {text.import}
        </button>
        <button title="标注当前章节" onClick={onMarkChapter} disabled={!canMark}>
          <Wand2 size={17} />
          {text.mark}
        </button>
        <button title="TTS 录制：为当前章节的所有分段生成语音，进度见右侧任务队列" onClick={onGenerateAll} disabled={!canGenerate}>
          <Mic2 size={17} />
          {text.generate}
        </button>
      </div>
      <div className="job-strip">
        {busy ? <Loader2 className="spin" size={16} /> : <CheckCircle2 size={16} />}
        <span>{activeJobCount}{text.activeTasks}</span>
      </div>
    </header>
  );
}
