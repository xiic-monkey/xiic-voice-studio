import { FolderSearch, Library, Loader2, Mic2, Pencil, Play, Save, ShieldCheck, Trash2, X } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { StudioSnapshot, VoiceAsset, VoiceProfile } from "../types";
import { ageStageLabels, text } from "../constants";
import type { VoiceProfilesController } from "../hooks/useVoiceProfiles";
import { useEscapeKey } from "../hooks/useEscapeKey";
import { AudioPlayer } from "./AudioPlayer";

type Props = {
  open: boolean;
  section: "library" | "clone";
  snapshot: StudioSnapshot | null;
  busy: string;
  voices: VoiceProfilesController;
  onClose: () => void;
  onSwitchSection: (section: "library" | "clone") => void;
  onDesignVoice: (characterId: string, characterName: string) => void;
};

export function VoiceCenter({ open, section, snapshot, busy, voices, onClose, onSwitchSection, onDesignVoice }: Props) {
  useEscapeKey(open, onClose);
  if (!open) return null;
  const profiles = snapshot?.voiceProfiles ?? [];
  const assets = snapshot?.voiceAssets ?? [];
  const assetById = new Map<string, VoiceAsset>(assets.map((asset) => [asset.id, asset]));
  const characterName = (characterId?: string) =>
    characterId
      ? snapshot?.characters.find((character) => character.id === characterId)?.canonicalName ?? "未知角色"
      : "旁白";
  // 音色的唯一真源是角色，所以这里按说话人分组展示，而不是平铺成一张"声音库"清单。
  const groups = new Map<string, VoiceProfile[]>();
  for (const profile of profiles) {
    const key = profile.characterId ?? "";
    const bucket = groups.get(key);
    if (bucket) bucket.push(profile);
    else groups.set(key, [profile]);
  }
  const orderedGroups = [...groups.entries()].sort(([left], [right]) => {
    if (left === "" && right !== "") return -1;
    if (right === "" && left !== "") return 1;
    return characterName(left || undefined).localeCompare(characterName(right || undefined), "zh-Hans-CN");
  });

  return (
    <div className="modal-backdrop">
      <section className="voice-center" role="dialog" aria-modal="true" aria-labelledby="voice-center-title">
        <header className="voice-center-header">
          <div>
            <span>每个说话人的音色归属</span>
            <h1 id="voice-center-title">声音中心</h1>
          </div>
          <button className="icon-button" title="关闭声音中心" onClick={onClose}>
            <X size={18} />
          </button>
        </header>
        <div className="voice-center-body">
          <nav className="voice-center-nav" aria-label="声音中心分组">
            <button
              className={section === "library" ? "selected" : ""}
              aria-current={section === "library" ? "page" : undefined}
              onClick={() => onSwitchSection("library")}
            >
              <Library size={16} />
              音色总览
            </button>
            <button
              className={section === "clone" ? "selected" : ""}
              aria-current={section === "clone" ? "page" : undefined}
              onClick={() => onSwitchSection("clone")}
            >
              <Mic2 size={16} />
              音色模仿
            </button>
          </nav>

          <div className="voice-center-content">
            {section === "library" ? (
              <section className="voice-library">
                <div className="section-heading">
                  <div>
                    <h2>角色音色总览</h2>
                    <span>
                      {orderedGroups.length} 个说话人 · {profiles.length} 个音色档案 · {assets.length} 个本地音色资产
                    </span>
                  </div>
                  <button className="primary-action" onClick={() => onSwitchSection("clone")}>
                    <Mic2 size={16} />用参考音频克隆
                  </button>
                </div>
                <div className="voice-library-list">
                  {orderedGroups.map(([characterId, group]) => (
                    <div className="voice-group" key={characterId || "__narrator"}>
                      <div className="voice-group-head">
                        <strong>{characterName(characterId || undefined)}</strong>
                        <span>
                          {group.length > 1 ? `${group.length} 个年龄阶段档位` : "1 个音色"}
                        </span>
                        {group.some((profile) => profile.isDefault) && (
                          <span className="voice-default-badge" title="系统自动兜底的音色，还没有人确认过">
                            {text.defaultUnconfirmed}
                          </span>
                        )}
                        {characterId && (
                          <button
                            className="ghost compact"
                            onClick={() => onDesignVoice(characterId, characterName(characterId))}
                          >
                            {text.designVoice}
                          </button>
                        )}
                      </div>
                      {group.map((profile) => {
                        const asset = profile.voiceAssetId ? assetById.get(profile.voiceAssetId) : undefined;
                        const editing = voices.editingVoiceProfileId === profile.id;
                        return (
                          <div className="voice-library-entry" key={profile.id}>
                            <article className="voice-library-row">
                              <div className="voice-library-icon"><Mic2 size={17} /></div>
                              <div className="voice-library-main">
                                <strong>{profile.name}</strong>
                                <span>{ageStageLabels[profile.ageStage] ?? profile.ageStage}</span>
                              </div>
                              <div className="voice-library-meta">
                                <span className={`voice-kind ${asset ? "clone" : ""}`}>
                                  {asset ? "克隆音色" : profile.model?.includes("voicedesign") ? "设计音色" : "预置音色"}
                                </span>
                                <small>{profile.model || profile.ttsProvider}</small>
                              </div>
                              <div className="voice-library-source">
                                <span>{asset?.sourceFileName || profile.voiceId}</span>
                                {asset?.consentConfirmed && <small><ShieldCheck size={13} />已确认授权</small>}
                              </div>
                              <div className="voice-library-actions">
                                <button
                                  className="icon-button"
                                  title={`试听 ${profile.name}`}
                                  onClick={() => voices.previewVoiceProfile(profile.id)}
                                  disabled={Boolean(busy)}
                                >
                                  {busy === "生成音色试听" ? <Loader2 className="spin" size={15} /> : <Play size={15} />}
                                </button>
                                <button className="icon-button" title="编辑音色档案" onClick={() => voices.startEditingVoiceProfile(profile)}>
                                  <Pencil size={15} />
                                </button>
                                <button
                                  className="icon-button"
                                  title={`删除 ${profile.name}`}
                                  onClick={() => voices.deleteVoiceProfile(profile)}
                                  disabled={Boolean(busy)}
                                >
                                  <Trash2 size={15} />
                                </button>
                              </div>
                            </article>
                            {editing && <VoiceEditor profile={profile} voices={voices} />}
                          </div>
                        );
                      })}
                    </div>
                  ))}
                  {!profiles.length && <div className="voice-library-empty">当前项目还没有音色档案</div>}
                </div>
                {voices.voicePreviewPath && (
                  <div className="voice-preview">
                    <AudioPlayer src={convertFileSrc(voices.voicePreviewPath)} label="项目音色试听" />
                    <span title={voices.voicePreviewPath}>项目音色试听</span>
                  </div>
                )}
              </section>
            ) : (
              <CloneForm snapshot={snapshot} voices={voices} onBackToLibrary={() => onSwitchSection("library")} />
            )}
          </div>
        </div>
      </section>
    </div>
  );
}

function VoiceEditor({ profile, voices }: { profile: VoiceProfile; voices: VoiceProfilesController }) {
  const draft = voices.voiceDraft;
  const change = (patch: Partial<typeof draft>) => voices.setVoiceDraft((current) => ({ ...current, ...patch }));
  return (
    <div className="voice-editor">
      <div className="voice-editor-grid">
        <label>名称<input value={draft.name} onChange={(event) => change({ name: event.target.value })} /></label>
        <label>
          年龄阶段
          <select value={draft.ageStage} onChange={(event) => change({ ageStage: event.target.value })}>
            {Object.entries(ageStageLabels).map(([stage, label]) => (
              <option key={stage} value={stage}>{label}</option>
            ))}
          </select>
        </label>
        <label>音色标识<input value={draft.voiceId} onChange={(event) => change({ voiceId: event.target.value })} /></label>
        <label>模型<input value={draft.model} onChange={(event) => change({ model: event.target.value })} /></label>
        <label>语速<input type="number" min="0.5" max="2" step="0.05" value={draft.speed} onChange={(event) => change({ speed: Number(event.target.value) })} /></label>
        <label>音高<input type="number" min="-12" max="12" step="0.5" value={draft.pitch} onChange={(event) => change({ pitch: Number(event.target.value) })} /></label>
      </div>
      <label>表演提示<textarea value={draft.style} onChange={(event) => change({ style: event.target.value })} /></label>
      <div className="voice-editor-actions">
        <button onClick={() => voices.deleteVoiceProfile(profile)}>删除档案</button>
        <span />
        <button onClick={voices.cancelEditingVoiceProfile}>取消</button>
        <button className="primary-action" onClick={voices.saveVoiceProfile} disabled={!draft.name.trim() || !draft.voiceId.trim()}>
          <Save size={15} />保存
        </button>
      </div>
    </div>
  );
}

function CloneForm({
  snapshot,
  voices,
  onBackToLibrary,
}: {
  snapshot: StudioSnapshot | null;
  voices: VoiceProfilesController;
  onBackToLibrary: () => void;
}) {
  const clone = voices.clone;
  return (
    <section className="clone-form">
      <div className="section-heading">
        <div>
          <h2>Mimo 音色模仿</h2>
          <span>mimo-v2.5-tts-voiceclone</span>
        </div>
      </div>
      <div className="clone-form-grid">
        <label htmlFor="clone-name">音色名称</label>
        <input id="clone-name" value={clone.name} onChange={(event) => clone.setName(event.target.value)} />

        <label htmlFor="clone-character">绑定角色</label>
        <select id="clone-character" value={clone.characterId} onChange={(event) => clone.setCharacterId(event.target.value)}>
          <option value="">旁白</option>
          {(snapshot?.characters ?? []).map((character) => (
            <option key={character.id} value={character.id}>{character.canonicalName}</option>
          ))}
        </select>

        <label htmlFor="clone-age-stage">年龄阶段</label>
        <select id="clone-age-stage" value={clone.ageStage} onChange={(event) => clone.setAgeStage(event.target.value)}>
          {Object.entries(ageStageLabels).map(([stage, label]) => (
            <option key={stage} value={stage}>{label}</option>
          ))}
        </select>

        <label htmlFor="clone-source">参考音频</label>
        <div className="clone-source-row">
          <input id="clone-source" value={clone.sourcePath} readOnly placeholder="选择 MP3 或 WAV，编码后不超过 10 MB" />
          <button title="选择参考音频" onClick={clone.chooseSource}><FolderSearch size={16} /></button>
        </div>

        <label htmlFor="clone-style">表演提示</label>
        <textarea id="clone-style" value={clone.style} onChange={(event) => clone.setStyle(event.target.value)} />
      </div>
      <label className="consent-check">
        <input type="checkbox" checked={clone.consent} onChange={(event) => clone.setConsent(event.target.checked)} />
        <ShieldCheck size={16} />
        我已获得该声音用于本项目合成与发布的授权
      </label>
      <div className="clone-actions">
        <button onClick={onBackToLibrary}>取消</button>
        <button className="primary-action" onClick={clone.create} disabled={!clone.name.trim() || !clone.sourcePath || !clone.consent}>
          <Mic2 size={16} />
          创建模仿音色
        </button>
      </div>
    </section>
  );
}
