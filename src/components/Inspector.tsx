import { useEffect, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Combine,
  Library,
  Mic2,
  Pencil,
  RefreshCw,
  Save,
  Trash2,
  UserRound,
  X,
} from "lucide-react";
import type { StudioSnapshot, VoiceProfile, VoiceStability } from "../types";
import { MIMO_PRESET_VOICES, reviewIssueLabels, text } from "../constants";
import { displayJobType, displayStatus } from "../utils";
import type { CharactersController } from "../hooks/useCharacters";
import type { ReviewIssueType } from "../types";
import { useEscapeKey } from "../hooks/useEscapeKey";
import { EmptyState, Panel } from "./ui";

/* ---------- 角色面板 ---------- */

type CharacterPanelProps = {
  snapshot: StudioSnapshot | null;
  characters: CharactersController;
  busy: string;
  onSetCharacterVoice: (characterId: string, voiceId: string) => void;
  onDesignVoice: (characterId: string, characterName: string) => void;
  onFinalizeCharacterVoice: (characterId: string) => void;
};

/** 角色声音的稳定性：预置 ID 和克隆样本可保证一致；描述式音色每次生成可能不同 */
function voiceStability(profile: VoiceProfile | undefined): VoiceStability {
  if (!profile) return "none";
  if (profile.voiceAssetId) return "clone";
  if (MIMO_PRESET_VOICES.includes(profile.voiceId)) return "preset";
  return "design";
}

/** 角色音色选择：预置音色 ID 直接绑定（稳定）；描述式的当前值保留为选项并配警告 */
function CharacterVoiceSelect({
  characterId,
  voiceId,
  busy,
  onSave,
}: {
  characterId: string;
  voiceId: string;
  busy: string;
  onSave: (characterId: string, voiceId: string) => void;
}) {
  const isPreset = MIMO_PRESET_VOICES.includes(voiceId);
  return (
    <select
      className="character-voice-input"
      aria-label="角色音色"
      value={voiceId}
      disabled={Boolean(busy)}
      onClick={(event) => event.stopPropagation()}
      onChange={(event) => onSave(characterId, event.target.value)}
    >
      {voiceId === "" && <option value="">未绑定音色</option>}
      {!isPreset && voiceId !== "" && <option value={voiceId}>当前描述（音色不稳定）</option>}
      {MIMO_PRESET_VOICES.filter((preset) => preset !== "mimo_default").map((preset) => (
        <option key={preset} value={preset}>
          {preset}
        </option>
      ))}
    </select>
  );
}

export function CharacterPanel({ snapshot, characters, busy, onSetCharacterVoice, onDesignVoice, onFinalizeCharacterVoice }: CharacterPanelProps) {
  const [mergeOpen, setMergeOpen] = useState(false);
  useEscapeKey(mergeOpen, () => setMergeOpen(false));
  const list = snapshot?.characters ?? [];
  const profiles = snapshot?.voiceProfiles ?? [];
  return (
    <Panel
      title={text.characters}
      icon={<UserRound size={16} />}
      className="character-panel"
      action={
        <button
          className="icon-button compact"
          title="合并角色"
          onClick={() => setMergeOpen((open) => !open)}
        >
          <Combine size={14} />
        </button>
      }
    >
      {mergeOpen && (
        <>
          <div className="popover-overlay" onClick={() => setMergeOpen(false)} />
          <div className="character-merge-popover">
            <div className="merge-popover-title">合并角色</div>
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
              className="primary-action compact"
              onClick={characters.mergeCharacters}
              disabled={!characters.mergeSourceId || !characters.mergeTargetId || characters.mergeSourceId === characters.mergeTargetId}
            >
              <UserRound size={14} />
              合并
            </button>
            <p className="merge-hint">来源角色的分段与别名会并入目标角色</p>
          </div>
        </>
      )}
      <div className="character-list">
        {list.map((character) => {
          const boundProfile = profiles.find((voice) => voice.characterId === character.id);
          const stability = voiceStability(boundProfile);
          return (
          <div className="character-card" key={character.id}>
            <span className="swatch" style={{ backgroundColor: character.defaultColor }} />
            <div>
              <strong>{character.canonicalName}</strong>
              <small>{character.aliases.join(", ") || text.noAliases}</small>
              {stability === "clone" ? (
                <small className="character-voice-fixed">已固化（克隆音色，音色稳定）</small>
              ) : (
                <CharacterVoiceSelect
                  characterId={character.id}
                  voiceId={boundProfile?.voiceId ?? ""}
                  busy={busy}
                  onSave={onSetCharacterVoice}
                />
              )}
              {stability === "design" && (
                <div className="voice-stability-warn">
                  <span>描述式音色每次生成可能不同</span>
                  <div className="voice-stability-actions">
                    <button className="ghost compact" onClick={() => onDesignVoice(character.id, character.canonicalName)}>
                      生成样本
                    </button>
                    <button className="ghost compact" onClick={() => onFinalizeCharacterVoice(character.id)}>
                      固化音色
                    </button>
                  </div>
                </div>
              )}
            </div>
            <div className="character-card-actions">
              <button title="编辑角色" onClick={() => characters.startEditingCharacter(character)}>
                <Pencil size={14} />
              </button>
            </div>
          </div>
          );
        })}
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
    </Panel>
  );
}


/* ---------- 声音面板 ---------- */

type VoicePanelProps = {
  snapshot: StudioSnapshot | null;
  busy: string;
  onSetNarratorVoice: (voiceId: string) => void;
  onOpenVoiceCenter: () => void;
};

export function VoicePanel({ snapshot, busy, onSetNarratorVoice, onOpenVoiceCenter }: VoicePanelProps) {
  const narratorProfile = (snapshot?.voiceProfiles ?? []).find((voice) => !voice.characterId);
  return (
    <Panel title={text.voices} icon={<Mic2 size={16} />}>
      <div className="voice-panel-actions">
        <button className="primary-action" onClick={onOpenVoiceCenter} disabled={!snapshot}>
          <Library size={16} />
          音色模仿
        </button>
      </div>
      <div className="voice-list">
        <div className="voice-card">
          <div className="voice-card-main">
            <strong>旁白</strong>
            <small>叙述、转场等非角色分段统一使用</small>
            <NarratorVoiceInput
              voiceId={narratorProfile?.voiceId ?? ""}
              busy={busy}
              onSave={onSetNarratorVoice}
            />
          </div>
        </div>
      </div>
    </Panel>
  );
}

function NarratorVoiceInput({
  voiceId,
  busy,
  onSave,
}: {
  voiceId: string;
  busy: string;
  onSave: (voiceId: string) => void;
}) {
  const [draft, setDraft] = useState(voiceId);
  useEffect(() => setDraft(voiceId), [voiceId]);
  return (
    <input
      className="character-voice-input"
      value={draft}
      placeholder="旁白音色 ID"
      spellCheck={false}
      disabled={Boolean(busy)}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        if (draft !== voiceId) onSave(draft);
      }}
    />
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

/** 从任务 payload 解读触发来源，让队列卡片可读 */
function describeJobTrigger(job: Job): string | null {
  try {
    const payload = JSON.parse(job.payloadJson ?? "null") as Record<string, unknown> | null;
    if (!payload) return null;
    if (job.jobType === "tts_batch" && Array.isArray(payload.segmentIds)) {
      return `${payload.segmentIds.length} 个分段`;
    }
    if (job.jobType === "mark_chapter") return "整章标注";
    return null;
  } catch {
    return null;
  }
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
                <strong>{displayJobType(job.jobType)}{describeJobTrigger(job) ? ` · ${describeJobTrigger(job)}` : ""}</strong>
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

      {!active.length && !attention.length && !done.length && (
        <EmptyState>生成语音、标注章节或导出成品时，任务进度会显示在这里</EmptyState>
      )}
    </Panel>
  );
}
