import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { AlertTriangle, AudioLines, CheckCircle2, Loader2, RotateCcw, Save, Sparkles, X } from "lucide-react";
import type { CharacterVoiceContext, Segment, StudioSnapshot, VoiceDescriptionResult } from "../types";
import type { CharacterProfileDraft } from "../hooks/useCharacters";
import { errorMessage, invoke } from "../utils";
import { useEscapeKey } from "../hooks/useEscapeKey";
import { AudioPlayer } from "./AudioPlayer";

type Props = {
  characterId: string;
  characterName: string;
  snapshot: StudioSnapshot | null;
  ttsSettings: { provider: string; apiKey?: string; endpoint?: string; model?: string };
  llm: { baseUrl: string; model: string; apiKey: string; keySaved?: boolean };
  ttsKeySaved?: boolean;
  busy: string;
  onNotice: (message: string) => void;
  /** 保存角色资料（含角色名）。返回是否成功与失败原因，失败必须在弹窗内可见。 */
  onSaveProfile: (characterId: string, draft: CharacterProfileDraft) => Promise<{ ok: boolean; error?: string }>;
  onClose: () => void;
  onFinalized: () => void;
};

type StepStatus = { tone: "info" | "error" | "success"; message: string } | null;

/** 试听台词的推荐上限：再长只增加合成耗时与费用，对音色还原没有额外收益。 */
const VOICE_SAMPLE_MAX_CHARS = 60;
/** 低于这个长度，voicedesign 很难稳定还原音色，需要提示用户。 */
const VOICE_SAMPLE_MIN_CHARS = 20;
/**
 * 纯台词的判定：以引号开头。
 * `dialogue` 分段里会混入「叙述 + 台词」（实测「中年男子」最长那条是
 * 「望着石碑上的信息…恭声道：“萧媚，斗之气：七段！”」），把叙述送去合成
 * 会被 TTS 一并念出来。选题时优先只用引号开头的句子。
 */
const SPOKEN_LINE_START = /^[“"「]/;
/** 整句都在引号内，才是干净台词；否则多半夹着旁白叙述。 */
const CLOSED_SPOKEN_LINE = /^[“"「][\s\S]*[”"」]$/;

/** 试听台词的长度与纯度提示：voicedesign 就靠这段文本学音色。 */
function describeSample(text: string): string {
  const notes = [`${text.length} 字`];
  if (text.length < VOICE_SAMPLE_MIN_CHARS) {
    notes.push("偏短，音色容易不稳，建议 30 字以上");
  }
  if (!CLOSED_SPOKEN_LINE.test(text)) {
    notes.push("可能夹着旁白叙述，试听时会一起念出来，可自行删减");
  }
  return `音色参考文本 · ${notes.join("；")}`;
}

/**
 * 角色声音工坊：章节上下文自动填充音色描述 → LLM 精修 → voicedesign 合成试听样本 →
 * 满意后把样本固化为 voiceclone 参考音频，角色音色从此确定。
 *
 * 交互原则：任何一步失败都必须在弹窗里说清楚原因。
 * 旧版本把错误写进页面底部的提示条，而提示条被弹窗遮罩盖住，表现就是"点了没反应"。
 */
export function CharacterVoiceDialog({
  characterId,
  characterName,
  snapshot,
  ttsSettings,
  llm,
  ttsKeySaved,
  busy,
  onNotice,
  onSaveProfile,
  onClose,
  onFinalized,
}: Props) {
  const [description, setDescription] = useState("");
  const [sampleText, setSampleText] = useState("");
  const [context, setContext] = useState<CharacterVoiceContext | null>(null);
  const [loadingContext, setLoadingContext] = useState(true);
  const [generatingDescription, setGeneratingDescription] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [finalizing, setFinalizing] = useState(false);
  const [assetId, setAssetId] = useState("");
  const [audioPath, setAudioPath] = useState("");
  const [status, setStatus] = useState<StepStatus>(null);
  const [showContext, setShowContext] = useState(false);
  // 用户动过描述框之后，不再用自动草稿覆盖他的输入
  const touched = useRef(false);
  // 试听台词同理：默认填充进来之后，只要用户动过就不再覆盖
  const sampleTouched = useRef(false);

  useEscapeKey(true, () => {
    if (!busy) onClose();
  });

  /* ---------- 角色名（弹窗里唯一暴露的资料编辑；其余字段只随草稿原样带回） ---------- */

  /**
   * 草稿初值 = 快照现值。界面上只暴露角色名，但后端 update_character 是整行覆盖：
   * aliases 传空会清光别名、gender/age/notes 传空会置 NULL —— 所以未编辑的字段
   * 必须按现值原样带回，绝不能传空。
   */
  function profileDraftOf(id: string, fallbackName: string): CharacterProfileDraft {
    const character = snapshot?.characters.find((item) => item.id === id);
    return {
      canonicalName: character?.canonicalName ?? fallbackName,
      aliases: character?.aliases.join("、") ?? "",
      gender: character?.gender ?? "",
      ageTimeline: character?.ageTimeline ?? "",
      notes: character?.notes ?? "",
    };
  }

  const [profileDraft, setProfileDraft] = useState<CharacterProfileDraft>(() =>
    profileDraftOf(characterId, characterName),
  );
  const [savingProfile, setSavingProfile] = useState(false);
  // 对话框换角色时不会重新挂载，草稿必须跟着角色复位（与试听台词同一套处理）
  useEffect(() => {
    setProfileDraft(profileDraftOf(characterId, characterName));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [characterId]);

  const profileDirty = profileDraft.canonicalName !== profileDraftOf(characterId, characterName).canonicalName;

  async function handleSaveProfile() {
    if (!profileDraft.canonicalName.trim()) {
      report("error", "角色名不能为空");
      return;
    }
    setSavingProfile(true);
    try {
      const outcome = await onSaveProfile(characterId, profileDraft);
      if (outcome.ok) {
        report("success", "角色名已保存，旧名字自动记为别名");
      } else {
        report("error", `保存角色名失败：${outcome.error ?? "未知错误"}`);
      }
    } finally {
      setSavingProfile(false);
    }
  }

  /**
   * 试听台词的默认值 = 「长度合适且最长」的一句台词。
   *
   * 不取第一条：首句常常只有几个字（实测「中年男子」首句 9 字、最长 110 字），
   * 参考音频太短 → voicedesign 拿不到足够的音色特征 → 固化后成片音色不稳，
   * 而固化用的是**这段试听音频**，所以这里的文本质量直接决定角色最终音色。
   *
   * 规则：预算内取最长；**若预算内最长的一句仍然偏短，宁可多花几秒用最长的那句** ——
   * 音色还原优先于合成耗时，绝不为了省时间挑一句念不全音色的短台词。
   */
  const defaultSample = useMemo(() => {
    const lines = (snapshot?.segments ?? [])
      .filter(
        (segment: Segment) =>
          segment.characterId === characterId &&
          segment.segmentType === "dialogue" &&
          segment.text.trim(),
      )
      .map((segment) => segment.text.trim());
    if (lines.length === 0) return `大家好，我是${characterName}。`;
    const longest = (pool: string[]) =>
      pool.reduce((best, line) => (line.length > best.length ? line : best));
    // 先只留纯台词；一个角色连一句引号台词都没有时才退回全部
    const spoken = lines.filter((line) => SPOKEN_LINE_START.test(line));
    const pool = spoken.length > 0 ? spoken : lines;
    const withinBudget = pool.filter((line) => line.length <= VOICE_SAMPLE_MAX_CHARS);
    const preferred = withinBudget.length > 0 ? longest(withinBudget) : longest(pool);
    return preferred.length < VOICE_SAMPLE_MIN_CHARS ? longest(pool) : preferred;
  }, [snapshot, characterId, characterName]);

  // 换角色时允许新的默认值覆盖旧角色的输入：
  // 对话框在切换角色时不会重新挂载，只能靠这里复位"用户触碰过"的标记。
  useEffect(() => {
    sampleTouched.current = false;
    touched.current = false;
  }, [characterId]);

  // 把默认台词**真正填进输入框**（而不是只当 placeholder）：
  // 灰字看起来像"还没填"，用户会不确定点生成到底用不用它。
  // 声明在上一个 effect 之后，保证「先重置触摸标记、再填充」的顺序。
  useEffect(() => {
    if (sampleTouched.current) return;
    setSampleText(defaultSample);
  }, [defaultSample]);

  const llmKeyReady = Boolean(llm.apiKey?.trim()) || Boolean(llm.keySaved);
  const ttsKeyReady = Boolean(ttsSettings.apiKey?.trim()) || Boolean(ttsKeySaved);

  /** 实际会送进合成的文本：界面上的值优先，留空则回落默认台词（与提交口径一致）。 */
  const sampleForSynthesis = sampleText.trim() || defaultSample;

  // 打开工坊即按章节上下文自动填充音色描述：没有 LLM Key 也能用。
  useEffect(() => {
    let disposed = false;
    setLoadingContext(true);
    invoke<CharacterVoiceContext>("character_voice_context", { request: { characterId } })
      .then((value) => {
        if (disposed) return;
        setContext(value);
        if (!touched.current && value.draft.trim()) {
          setDescription(value.draft);
          setStatus({
            tone: "info",
            message: `已按章节上下文自动填充（参考 ${value.lineCount} 条台词、${value.narrationCount} 段相关叙述）`,
          });
        }
      })
      .catch((error) => {
        if (disposed) return;
        setStatus({ tone: "error", message: `读取章节上下文失败：${errorMessage(error)}` });
      })
      .finally(() => {
        if (!disposed) setLoadingContext(false);
      });
    return () => {
      disposed = true;
    };
  }, [characterId]);

  function report(tone: "info" | "error" | "success", message: string) {
    setStatus({ tone, message });
    if (tone === "error") onNotice(message);
  }

  async function handleGenerateDescription() {
    if (!llmKeyReady) {
      report(
        "error",
        `还没配置 LLM API Key，AI 生成不可用。请到「设置 · LLM 标注」填入并保存（当前模型 ${llm.model}）；也可以直接用上面的章节上下文草稿。`,
      );
      return;
    }
    setGeneratingDescription(true);
    report("info", "正在按章节上下文生成音色描述…");
    try {
      const result = await invoke<VoiceDescriptionResult>("generate_voice_description", {
        request: {
          characterId,
          settings: { baseUrl: llm.baseUrl, model: llm.model, apiKey: llm.apiKey || undefined },
        },
      });
      touched.current = true;
      setDescription(result.description);
      if (result.warning) {
        report("error", result.warning);
      } else {
        report(
          "success",
          `AI 已按章节上下文生成（参考 ${result.lineCount} 条台词、${result.narrationCount} 段相关叙述）`,
        );
      }
    } catch (error) {
      report("error", errorMessage(error));
    } finally {
      setGeneratingDescription(false);
    }
  }

  async function handleGenerateSample() {
    if (!description.trim()) {
      report("error", "请先填写音色描述（可点「用上下文重填」）");
      return;
    }
    if (ttsSettings.provider !== "mimo") {
      report("error", `音色设计只支持 Mimo（voicedesign + voiceclone），当前供应商是 ${ttsSettings.provider}`);
      return;
    }
    if (!ttsKeyReady) {
      report("error", "还没配置 Mimo API Key，无法合成试听。请到「设置 · 语音生成」填入并保存。");
      return;
    }
    setGenerating(true);
    report("info", "正在用 voicedesign 合成试听样本…");
    try {
      const result = await invoke<{ assetId: string; audioPath: string }>(
        "generate_character_voice_sample",
        {
          request: {
            characterId,
            description,
            sampleText: sampleForSynthesis,
            settings: ttsSettings,
          },
        },
      );
      setAssetId(result.assetId);
      setAudioPath(result.audioPath);
      report("success", "试听样本已生成，试听满意后可固化为角色音色");
    } catch (error) {
      report("error", errorMessage(error));
    } finally {
      setGenerating(false);
    }
  }

  async function handleFinalize() {
    if (!assetId) return;
    setFinalizing(true);
    try {
      await invoke("finalize_character_voice", {
        request: { characterId, assetId },
      });
      onFinalized();
      onClose();
    } catch (error) {
      report("error", errorMessage(error));
      setFinalizing(false);
    }
  }

  return (
    <div className="modal-backdrop">
      <section
        className="character-voice-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="角色声音工坊"
      >
        <header className="voice-center-header">
          <div>
            <span>角色声音工坊 · 编辑角色</span>
            <input
              className="character-name-input"
              value={profileDraft.canonicalName}
              placeholder="角色名"
              aria-label="角色名"
              spellCheck={false}
              onChange={(event) =>
                setProfileDraft((current) => ({ ...current, canonicalName: event.target.value }))
              }
            />
          </div>
          <div className="voice-center-header-actions">
            <button
              className="ghost compact"
              title="保存角色名"
              onClick={handleSaveProfile}
              disabled={savingProfile || !profileDraft.canonicalName.trim() || !profileDirty}
            >
              {savingProfile ? <Loader2 size={14} className="spin" /> : <Save size={14} />}
              保存角色名
            </button>
            <button className="icon-button" title="关闭" onClick={onClose} disabled={Boolean(busy)}>
              <X size={18} />
            </button>
          </div>
        </header>
        <div className="character-voice-body">
          {busy && <p className="voice-flow-status">另一个任务进行中（{busy}），完成后再试。</p>}
          {status && (
            <p className={`voice-flow-status ${status.tone}`} role={status.tone === "error" ? "alert" : "status"}>
              {status.tone === "error" ? <AlertTriangle size={14} /> : null}
              {status.tone === "success" ? <CheckCircle2 size={14} /> : null}
              {status.tone === "info" && loadingContext ? <Loader2 size={14} className="spin" /> : null}
              <span>{status.message}</span>
            </p>
          )}

          {(!llmKeyReady || !ttsKeyReady) && (
            <p className="voice-flow-status error">
              <AlertTriangle size={14} />
              <span>
                缺少 API Key：
                {!llmKeyReady && " LLM（AI 生成不可用，可先用章节上下文草稿）"}
                {!ttsKeyReady && ` ${ttsSettings.provider}（无法生成试听样本）`}
                。到「设置」里填入并保存即可，不用重开窗口。
              </span>
            </p>
          )}

          <div className="voice-flow-step">
            <div className="voice-flow-step-head">
              <strong>1 · 音色描述</strong>
              <div className="voice-flow-head-actions">
                <button
                  className="ghost compact"
                  title="用章节上下文重新生成草稿"
                  onClick={() => {
                    touched.current = true;
                    setDescription(context?.draft ?? "");
                    report(
                      "info",
                      context
                        ? `已按章节上下文重填（参考 ${context.lineCount} 条台词、${context.narrationCount} 段相关叙述）`
                        : "还没有拿到章节上下文",
                    );
                  }}
                  disabled={loadingContext || !context}
                >
                  <RotateCcw size={14} />
                  用上下文重填
                </button>
                <button
                  className="ghost compact"
                  onClick={handleGenerateDescription}
                  disabled={generatingDescription || Boolean(busy)}
                >
                  {generatingDescription ? <Loader2 size={14} className="spin" /> : <Sparkles size={14} />}
                  AI 根据角色生成
                </button>
              </div>
            </div>
            <textarea
              value={description}
              onChange={(event) => {
                touched.current = true;
                setDescription(event.target.value);
              }}
              placeholder="例：青年男性，声线清亮略带沙哑，语气桀骜、语速偏快，适合争强好胜的少年角色"
              rows={3}
            />
            {context && (context.lineCount > 0 || context.narrationCount > 0) && (
              <div className="voice-context">
                <button className="ghost compact" onClick={() => setShowContext((value) => !value)}>
                  {showContext ? "收起章节上下文" : `查看章节上下文（${context.lineCount} 条台词 / ${context.narrationCount} 段叙述）`}
                </button>
                {showContext && (
                  <ul className="voice-context-lines">
                    {context.sampleLines.length === 0 && <li>没有采到该角色的台词分段。</li>}
                    {context.sampleLines.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                )}
              </div>
            )}
            <div className="voice-flow-step-head">
              <strong>2 · 试听台词</strong>
            </div>
            <input
              value={sampleText}
              onChange={(event) => {
                sampleTouched.current = true;
                setSampleText(event.target.value);
              }}
              placeholder={defaultSample}
              spellCheck={false}
            />
            <p className="voice-flow-hint">
              {sampleText.trim()
                ? describeSample(sampleForSynthesis)
                : `留空将使用该角色台词：${
                    defaultSample.length > 26 ? `${defaultSample.slice(0, 26)}…` : defaultSample
                  }`}
            </p>
            <button className="primary-action" onClick={handleGenerateSample} disabled={generating || Boolean(busy)}>
              {generating ? <Loader2 size={15} className="spin" /> : <AudioLines size={15} />}
              生成声音试听
            </button>
          </div>

          <div className="voice-flow-step">
            <div className="voice-flow-step-head">
              <strong>3 · 试听与固化</strong>
            </div>
            {audioPath && <AudioPlayer src={convertFileSrc(audioPath)} label="音色试听" />}
            {!audioPath && <p className="empty">生成试听后，可反复修改描述重新生成，满意后固化。</p>}
            <p className="voice-flow-hint">
              固化会把当前试听样本作为音色模仿的参考音频：该角色之后所有分段都参考同一个声音，音色保持一致。
            </p>
            <button
              className="primary-action"
              onClick={handleFinalize}
              disabled={!assetId || finalizing || Boolean(busy)}
            >
              {finalizing ? <Loader2 size={15} className="spin" /> : null}
              满意了，固化为角色音色
            </button>
          </div>
        </div>
      </section>
    </div>
  );
}
