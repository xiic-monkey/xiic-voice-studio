import { useState } from "react";
import { CheckCircle2, ChevronDown, ChevronRight, Library, Mic2, Pencil, RefreshCw, Save, Sparkles, Trash2, UserRound, X } from "lucide-react";
import type { StudioSnapshot, VoiceProfile } from "../types";
import { ageStages, ageStageLabels, reviewIssueLabels, text } from "../constants";
import { displayJobType, displayStatus } from "../utils";
import type { CharactersController } from "../hooks/useCharacters";
import type { ReviewIssueType } from "../types";
import { EmptyState, Panel } from "./ui";

/* ---------- 角色面板 ---------- */

type CharacterPanelProps = {
  snapshot: StudioSnapshot | null;
  characters: CharactersController;
  onAssignVoice: (character?: { id: string; canonicalName: string }) => void;
};

export function CharacterPanel({ snapshot, characters, onAssignVoice }: CharacterPanelProps) {
  const list = snapshot?.characters ?? [];
  return (
    <Panel title={text.characters} icon={<UserRound size={16} />}>
      <div className="character-list">
        {list.map((character) => (
          <div className="character-card" key={character.id}>
            <span className="swatch" style={{ backgroundColor: character.defaultColor }} />
            <div>
              <strong>{character.canonicalName}</strong>
              <small>{character.aliases.join(", ") || text.noAliases}</small>
            </div>
            <div className="character-card-actions">
              <button title="编辑角色" onClick={() => characters.startEditingCharacter(character)}>
                <Pencil size={14} />
              </button>
              <button title={text.assignVoice} onClick={() => onAssignVoice(character)}>
                <Mic2 size={15} />
              </button>
            </div>
          </div>
        ))}
        {!list.length && <p className="empty">{text.emptyCharacters}</p>}
      </div>
      {characters.editingCharacterId && (
        <div className="character-editor">
          <div className="character-editor-title">编辑角色资料</div>
          <input
            value={characters.characterDraft.canonicalName}
            placeholder="角色名"
            onChange={(event) => characters.setCharacterDraft((current) => ({ ...current, canonicalName: event.target.value }))}
          />
          <input
            value={characters.characterDraft.aliases}
            placeholder="别名，用顿号分隔"
            onChange={(event) => characters.setCharacterDraft((current) => ({ ...current, aliases: event.target.value }))}
          />
          <div className="inline">
            <input
              value={characters.characterDraft.gender}
              placeholder="性别"
              onChange={(event) => characters.setCharacterDraft((current) => ({ ...current, gender: event.target.value }))}
            />
            <input
              value={characters.characterDraft.ageTimeline}
              placeholder="年龄阶段或成长线"
              onChange={(event) => characters.setCharacterDraft((current) => ({ ...current, ageTimeline: event.target.value }))}
            />
          </div>
          <textarea
            value={characters.characterDraft.notes}
            placeholder="角色备注"
            onChange={(event) => characters.setCharacterDraft((current) => ({ ...current, notes: event.target.value }))}
          />
          <div className="inline character-editor-actions">
            <button onClick={characters.cancelEditingCharacter}>取消</button>
            <button
              className="primary-action"
              onClick={characters.saveCharacter}
              disabled={!characters.characterDraft.canonicalName.trim()}
            >
              <Save size={15} />保存角色
            </button>
          </div>
        </div>
      )}
      {list.length >= 2 && (
        <div className="merge-controls">
          <select value={characters.mergeSourceId} onChange={(event) => characters.setMergeSourceId(event.target.value)}>
            <option value="">{text.sourceCharacter}</option>
            {list.map((character) => (
              <option key={character.id} value={character.id}>{character.canonicalName}</option>
            ))}
          </select>
          <select value={characters.mergeTargetId} onChange={(event) => characters.setMergeTargetId(event.target.value)}>
            <option value="">{text.targetCharacter}</option>
            {list.map((character) => (
              <option key={character.id} value={character.id}>{character.canonicalName}</option>
            ))}
          </select>
          <button
            onClick={characters.mergeCharacters}
            disabled={!characters.mergeSourceId || !characters.mergeTargetId || characters.mergeSourceId === characters.mergeTargetId}
          >
            <UserRound size={15} />
            {text.mergeCharacters}
          </button>
        </div>
      )}
    </Panel>
  );
}

/* ---------- 声音面板 ---------- */

type VoicePanelProps = {
  snapshot: StudioSnapshot | null;
  onAssignNarrator: () => void;
  onOpenVoiceCenter: () => void;
};

export function VoicePanel({ snapshot, onAssignNarrator, onOpenVoiceCenter }: VoicePanelProps) {
  const profiles = snapshot?.voiceProfiles ?? [];
  return (
    <Panel title={text.voices} icon={<Mic2 size={16} />}>
      <div className="voice-panel-actions">
        <button onClick={onAssignNarrator} disabled={!snapshot} title={text.addNarratorVoice}>
          <Sparkles size={16} />
          旁白
        </button>
        <button className="primary-action" onClick={onOpenVoiceCenter} disabled={!snapshot}>
          <Library size={16} />
          声音中心
        </button>
      </div>
      <div className="voice-list">
        {profiles.map((voice: VoiceProfile) => (
          <div className="voice-card" key={voice.id}>
            <div className="voice-card-main">
              <strong>{voice.name}</strong>
              <small>{voice.ttsProvider} · {voice.voiceId}</small>
            </div>
            <select value={voice.ageStage} disabled aria-label={`${voice.name} 的年龄阶段`}>
              {ageStages.map((stage) => (
                <option key={stage} value={stage}>{ageStageLabels[stage] ?? stage}</option>
              ))}
            </select>
          </div>
        ))}
      </div>
    </Panel>
  );
}

/* ---------- 审听面板 ---------- */

type ReviewPanelProps = {
  selectedSegmentText?: string;
  hasSegment: boolean;
  openIssues: { id: string; issueType: string; note: string }[];
  review: {
    issueType: ReviewIssueType;
    setIssueType: (value: ReviewIssueType) => void;
    note: string;
    setNote: (value: string) => void;
  };
  onApprove: () => void;
  onReject: () => void;
  onAddIssue: () => void;
};

export function ReviewPanel({
  selectedSegmentText,
  hasSegment,
  openIssues,
  review,
  onApprove,
  onReject,
  onAddIssue,
}: ReviewPanelProps) {
  return (
    <Panel title={text.review} className="review-panel">
      <div className="review-script" title={selectedSegmentText ?? ""}>
        {selectedSegmentText ?? text.noSegment}
      </div>
      <div className="inline">
        <button onClick={onApprove} disabled={!hasSegment}>
          <CheckCircle2 size={16} />
          {text.approveAudio}
        </button>
        <button onClick={onReject} disabled={!hasSegment}>
          <RefreshCw size={16} />
          {text.rejectAudio}
        </button>
      </div>
      <div className="review-note-fields">
        <select
          value={review.issueType}
          onChange={(event) => review.setIssueType(event.target.value as ReviewIssueType)}
          disabled={!hasSegment}
        >
          {Object.entries(reviewIssueLabels)
            .filter(([value]) => value !== "human_note")
            .map(([value, label]) => (
              <option key={value} value={value}>{label}问题</option>
            ))}
        </select>
        <textarea
          value={review.note}
          onChange={(event) => review.setNote(event.target.value)}
          disabled={!hasSegment}
          placeholder="记录具体的发音、表演、节奏或音质问题"
        />
      </div>
      <button onClick={onAddIssue} disabled={!hasSegment || !review.note.trim()}>
        <CheckCircle2 size={16} />
        {text.addReviewNote}
      </button>
      {openIssues.length > 0 && (
        <div className="review-issues">
          <small>当前分段未解决问题</small>
          {openIssues.map((issue) => (
            <div className="review-issue" key={issue.id}>
              <strong>{reviewIssueLabels[issue.issueType] ?? issue.issueType}</strong>
              <span>{issue.note}</span>
            </div>
          ))}
        </div>
      )}
    </Panel>
  );
}

/* ---------- 任务队列面板 ---------- */

type ProductionReportLite = {
  canPublish: boolean;
  readySegments: number;
  totalSegments: number;
  approvedSegments: number;
  missingAudio: number;
  rejectedSegments: number;
  unreviewedSegments: number;
};

type JobPanelProps = {
  snapshot: StudioSnapshot | null;
  productionReport: ProductionReportLite | null;
  onCancelJob: (jobId: string) => void;
  onRetryJob: (jobId: string) => void;
  onDeleteJob: (jobId: string) => void;
  onClearFinishedJobs: () => void;
};

type Job = NonNullable<StudioSnapshot["jobs"]>[number];

/**
 * 按"队列"语义分组：进行中在最上、需要处理的失败任务居中、
 * 已完成的默认折叠成一行摘要。后端按创建时间倒序返回。
 */
function groupJobs(jobs: Job[]) {
  const active = jobs.filter((job) => job.status === "running" || job.status === "pending");
  const attention = jobs.filter((job) => job.status === "failed" || job.status === "canceled");
  const done = jobs.filter(
    (job) => job.status !== "running" && job.status !== "pending" && job.status !== "failed" && job.status !== "canceled",
  );
  return { active, attention, done };
}

/** 后端目前只支持重试语音生成任务 */
function canRetryJob(job: Job) {
  return job.jobType === "tts_batch";
}

export function JobPanel({ snapshot, productionReport, onCancelJob, onRetryJob, onDeleteJob, onClearFinishedJobs }: JobPanelProps) {
  const [historyOpen, setHistoryOpen] = useState(false);
  const { active, attention, done } = groupJobs(snapshot?.jobs ?? []);

  function handleDelete(job: Job) {
    if (window.confirm(`删除这条“${displayJobType(job.jobType)}”任务记录？`)) onDeleteJob(job.id);
  }

  function handleClearFinished() {
    const total = attention.length + done.length;
    if (window.confirm(`清空全部 ${total} 条已结束的任务记录？`)) onClearFinishedJobs();
  }

  return (
    <Panel title={text.queue} className="jobs-panel">
      {productionReport && (
        <div className="readiness-summary">
          <strong>{text.readiness}：{productionReport.canPublish ? text.publishReady : text.publishBlocked}</strong>
          <small>
            音频 {productionReport.readySegments}/{productionReport.totalSegments} · 通过 {productionReport.approvedSegments} · 缺失 {productionReport.missingAudio} · 返修 {productionReport.rejectedSegments} · 未审 {productionReport.unreviewedSegments}
          </small>
        </div>
      )}

      {active.length > 0 && (
        <div className="job-section">
          <span className="job-section-label">进行中 · {active.length}</span>
          {active.map((job) => (
            <div className="job-card" key={job.id}>
              <div className="job-card-main">
                <strong>{displayJobType(job.jobType)}</strong>
                <div className="job-progress">
                  <span style={{ width: `${Math.round(job.progress * 100)}%` }} />
                </div>
                <small>{displayStatus(job.status)} · {Math.round(job.progress * 100)}%</small>
              </div>
              <div className="job-actions">
                <button title={text.cancelJob} onClick={() => onCancelJob(job.id)}>
                  <X size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {attention.length > 0 && (
        <div className="job-section">
          <span className="job-section-label job-section-label-alert">需要处理 · {attention.length}</span>
          {attention.map((job) => (
            <div className="job-card job-card-alert" key={job.id}>
              <div className="job-card-main">
                <strong>{displayJobType(job.jobType)}</strong>
                <small>{displayStatus(job.status)}{job.error ? " · " + job.error : ""}</small>
                {!canRetryJob(job) && <small>请在工作台重新执行该操作</small>}
              </div>
              <div className="job-actions">
                {canRetryJob(job) && (
                  <button title={text.retryJob} onClick={() => onRetryJob(job.id)}>
                    <RefreshCw size={14} />
                  </button>
                )}
                <button title="删除记录" onClick={() => handleDelete(job)}>
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {done.length > 0 && (
        <div className="job-section">
          <div className="job-history-row">
            <button className="job-history-toggle" onClick={() => setHistoryOpen((open) => !open)}>
              {historyOpen ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
              已完成 · {done.length} 个任务
            </button>
            <button className="icon-button compact job-clear-button" title="清空已结束任务记录" onClick={handleClearFinished}>
              <Trash2 size={13} />
            </button>
          </div>
          {historyOpen &&
            done.map((job) => (
              <div className="job-card job-card-done" key={job.id}>
                <div className="job-card-main">
                  <strong>{displayJobType(job.jobType)}</strong>
                  <small>{displayStatus(job.status)} · {Math.round(job.progress * 100)}%</small>
                </div>
                <div className="job-actions">
                  <button title="删除记录" onClick={() => handleDelete(job)}>
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))}
        </div>
      )}

      {!active.length && !attention.length && !done.length && !productionReport && (
        <EmptyState>暂无任务</EmptyState>
      )}
    </Panel>
  );
}
