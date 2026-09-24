import { useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { AudioLines, Loader2, Sparkles, X } from "lucide-react";
import type { Segment, StudioSnapshot } from "../types";
import { errorMessage, invoke } from "../utils";
import { useEscapeKey } from "../hooks/useEscapeKey";
import { AudioPlayer } from "./AudioPlayer";

type Props = {
  characterId: string;
  characterName: string;
  snapshot: StudioSnapshot | null;
  ttsSettings: { provider: string; apiKey?: string; endpoint?: string; model?: string };
  llm: { baseUrl: string; model: string; apiKey: string };
  busy: string;
  onNotice: (message: string) => void;
  onClose: () => void;
  onFinalized: () => void;
};

/**
 * 角色声音工坊：LLM 生成音色描述 → voicedesign 合成试听样本 →
 * 满意后把样本固化为 voiceclone 参考音频，角色音色从此确定。
 */
export function CharacterVoiceDialog({
  characterId,
  characterName,
  snapshot,
  ttsSettings,
  llm,
  busy,
  onNotice,
  onClose,
  onFinalized,
}: Props) {
  const [description, setDescription] = useState("");
  const [sampleText, setSampleText] = useState("");
  const [generatingDescription, setGeneratingDescription] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [finalizing, setFinalizing] = useState(false);
  const [assetId, setAssetId] = useState("");
  const [audioPath, setAudioPath] = useState("");

  // 与关闭按钮保持一致：任务进行中不允许误触 Esc 关闭
  useEscapeKey(true, () => {
    if (!busy) onClose();
  });

  const defaultSample = useMemo(() => {
    const dialogue = (snapshot?.segments ?? []).find(
      (segment: Segment) =>
        segment.characterId === characterId &&
        segment.segmentType === "dialogue" &&
        segment.text.trim(),
    );
    return dialogue?.text ?? `大家好，我是${characterName}。`;
  }, [snapshot, characterId, characterName]);

  async function handleGenerateDescription() {
    setGeneratingDescription(true);
    try {
      const description = await invoke<string>("generate_voice_description", {
        request: {
          characterId,
          settings: { baseUrl: llm.baseUrl, model: llm.model, apiKey: llm.apiKey || undefined },
        },
      });
      setDescription(description);
    } catch (error) {
      onNotice(errorMessage(error));
    } finally {
      setGeneratingDescription(false);
    }
  }

  async function handleGenerateSample() {
    if (!description.trim()) {
      onNotice("请先填写音色描述");
      return;
    }
    setGenerating(true);
    try {
      const result = await invoke<{ assetId: string; audioPath: string }>(
        "generate_character_voice_sample",
        {
          request: {
            characterId,
            description,
            sampleText: sampleText.trim() || defaultSample,
            settings: ttsSettings,
          },
        },
      );
      setAssetId(result.assetId);
      setAudioPath(result.audioPath);
    } catch (error) {
      onNotice(errorMessage(error));
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
      onNotice(errorMessage(error));
      setFinalizing(false);
    }
  }

  return (
    <div className="modal-backdrop">
      <section
        className="character-voice-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="character-voice-title"
      >
        <header className="voice-center-header">
          <div>
            <span>角色声音工坊</span>
            <h1 id="character-voice-title">{characterName}</h1>
          </div>
          <button className="icon-button" title="关闭" onClick={onClose} disabled={Boolean(busy)}>
            <X size={18} />
          </button>
        </header>
        <div className="character-voice-body">
          <div className="voice-flow-step">
            <div className="voice-flow-step-head">
              <strong>1 · 音色描述</strong>
              <button
                className="ghost compact"
                onClick={handleGenerateDescription}
                disabled={generatingDescription || Boolean(busy)}
              >
                {generatingDescription ? <Loader2 size={14} className="spin" /> : <Sparkles size={14} />}
                AI 根据角色生成
              </button>
            </div>
            <textarea
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder="例：青年男性，声线清亮略带沙哑，语气桀骜、语速偏快，适合争强好胜的少年角色"
              rows={3}
            />
            <div className="voice-flow-step-head">
              <strong>2 · 试听台词</strong>
            </div>
            <input
              value={sampleText}
              onChange={(event) => setSampleText(event.target.value)}
              placeholder={defaultSample}
              spellCheck={false}
            />
            <button className="primary-action" onClick={handleGenerateSample} disabled={generating || Boolean(busy)}>
              {generating ? <Loader2 size={15} className="spin" /> : <AudioLines size={15} />}
              生成声音试听
            </button>
          </div>

          <div className="voice-flow-step">
            <div className="voice-flow-step-head">
              <strong>3 · 试听与固化</strong>
            </div>
            {audioPath && (
              <AudioPlayer src={convertFileSrc(audioPath)} label="音色试听" />
            )}
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
