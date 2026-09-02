import { ArrowLeft, AudioLines, Bot, Download, FolderOpen, FolderSearch, Loader2, Mic2, Play, Save, Scissors, ShieldCheck, Upload } from "lucide-react";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { StudioSnapshot } from "../types";
import { text, ttsProviderLabels } from "../constants";
import type { AppSettingsController } from "../hooks/useAppSettings";
import type { SettingsSection } from "../types";
import { AudioPlayer } from "./AudioPlayer";
import { CheckIndicator, SettingsField } from "./ui";

type Props = {
  desktopRuntime: boolean;
  runtimeMessage: string;
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  onLeave: () => void;
  busy: string;
  snapshot: StudioSnapshot | null;
  settings: AppSettingsController;
  projectCoverPath: string;
  audioOutputPath: string;
  onChooseCover: () => void;
  onCheckAssets: () => void;
  onBackup: () => void;
  onOpenAudioFolder: () => void;
};

const sectionTitles: Record<SettingsSection, string> = {
  llm: "LLM 标注",
  tts: "TTS 生成",
  audio: "音频工具",
  export: "导出",
};

export function SettingsView({
  desktopRuntime,
  runtimeMessage,
  section,
  onSectionChange,
  onLeave,
  busy,
  snapshot,
  settings,
  projectCoverPath,
  audioOutputPath,
  onChooseCover,
  onCheckAssets,
  onBackup,
  onOpenAudioFolder,
}: Props) {
  return (
    <main className="settings-shell">
      <aside className="settings-sidebar">
        <div className="settings-brand">
          <AudioLines size={22} />
          <div>
            <strong>Xiic Voice Studio</strong>
            <span>设置</span>
          </div>
        </div>
        <button className="settings-back" onClick={onLeave}>
          <ArrowLeft size={16} />
          返回工作台
        </button>
        <div className="settings-group-label">设置分组</div>
        <nav className="settings-nav" aria-label="设置分组">
          {(["llm", "tts", "audio", "export"] as SettingsSection[]).map((key) => (
            <button
              key={key}
              className={section === key ? "selected" : ""}
              aria-current={section === key ? "page" : undefined}
              onClick={() => onSectionChange(key)}
            >
              <SectionIcon section={key} />
              {sectionTitles[key]}
            </button>
          ))}
        </nav>
      </aside>
      <section className="settings-main">
        <header className="settings-header">
          <div>
            <span>设置</span>
            <h1>{sectionTitles[section]}</h1>
          </div>
          <div className="settings-save">
            <span className={settings.settingsDirty ? "unsaved" : ""} role="status" aria-live="polite">
              {settings.settingsDirty ? "有未保存更改" : "设置已保存"}
            </span>
            <button className="primary-action" onClick={settings.saveApplicationSettings} disabled={!settings.settingsDirty || Boolean(busy)}>
              <Save size={16} />
              保存设置
            </button>
          </div>
        </header>
        <div className="settings-content">
          {!desktopRuntime && (
            <div className="runtime-notice" role="status">
              <ShieldCheck size={17} />
              <div>
                <strong>浏览器预览模式</strong>
                <span>{runtimeMessage}</span>
              </div>
            </div>
          )}
          {section === "llm" && <LlmForm settings={settings} />}
          {section === "tts" && <TtsForm settings={settings} busy={busy} />}
          {section === "audio" && <AudioForm settings={settings} />}
          {section === "export" && (
            <ExportForm
              snapshot={snapshot}
              settings={settings}
              projectCoverPath={projectCoverPath}
              audioOutputPath={audioOutputPath}
              onChooseCover={onChooseCover}
              onCheckAssets={onCheckAssets}
              onBackup={onBackup}
              onOpenAudioFolder={onOpenAudioFolder}
            />
          )}
        </div>
      </section>
    </main>
  );
}

function SectionIcon({ section }: { section: SettingsSection }) {
  if (section === "llm") return <Bot size={16} />;
  if (section === "tts") return <Mic2 size={16} />;
  if (section === "audio") return <Scissors size={16} />;
  return <Download size={16} />;
}

function LlmForm({ settings }: { settings: AppSettingsController }) {
  const llm = settings.llm;
  return (
    <section className="settings-form">
      <p className="settings-description">用于章节脚本标注、角色抽取和别名建议。兼容 OpenAI Chat Completions 接口。</p>
      <div className="settings-fields">
        <div className="settings-fields-heading">
          <div className="settings-fields-heading-icon"><Bot size={16} /></div>
          <div>
            <strong>连接配置</strong>
            <span>为章节标注提供模型接口</span>
          </div>
        </div>
        <div className="settings-field-grid">
          <SettingsField id="llm-base-url" label={text.llmBaseUrl} hint="OpenAI 兼容">
            <input id="llm-base-url" type="url" value={llm.baseUrl} onChange={(event) => llm.changeBaseUrl(event.target.value)} />
          </SettingsField>
          <SettingsField id="llm-model" label={text.llmModel}>
            <input id="llm-model" value={llm.model} onChange={(event) => llm.changeModel(event.target.value)} />
          </SettingsField>
          <SettingsField id="llm-api-key" label={text.llmApiKey} wide>
            <input id="llm-api-key" type="password" value={llm.apiKey} onChange={(event) => llm.changeApiKey(event.target.value)} autoComplete="off" />
          </SettingsField>
        </div>
        <div className="credential-row">
          <small>{llm.keySaved ? text.apiKeySaved : text.noApiKeySaved}</small>
          <div className="inline">
            <button onClick={llm.deleteKey} disabled={!llm.keySaved}>{text.deleteApiKey}</button>
            <button onClick={llm.saveKey} disabled={!llm.apiKey.trim()}>
              <Save size={16} />{text.saveApiKey}
            </button>
          </div>
        </div>
      </div>
      <div className="settings-test-row">
        <CheckIndicator state={llm.check} />
        <button onClick={llm.test} disabled={llm.check.kind === "running"}>
          {llm.check.kind === "running" ? <Loader2 className="spin" size={16} /> : <Play size={16} />}
          测试连接
        </button>
      </div>
    </section>
  );
}

function TtsForm({ settings, busy }: { settings: AppSettingsController; busy: string }) {
  const tts = settings.tts;
  return (
    <section className="settings-form settings-form-tts">
      <p className="settings-description">配置有声读物的语音生成服务。Mimo 支持预置音色和 VoiceDesign 文字设计。</p>
      <div className="settings-fields">
        <div className="settings-fields-heading">
          <div className="settings-fields-heading-icon"><Mic2 size={16} /></div>
          <div>
            <strong>语音服务</strong>
            <span>连接供应商并设置默认生成参数</span>
          </div>
        </div>
        <div className="settings-field-grid">
          <SettingsField id="tts-provider" label={text.ttsProvider}>
            <select id="tts-provider" value={tts.provider} onChange={(event) => tts.changeProvider(event.target.value)}>
              {Object.entries(ttsProviderLabels).map(([value, label]) => (
                <option key={value} value={value}>{label}</option>
              ))}
            </select>
          </SettingsField>
          <SettingsField id="tts-api-key" label={text.ttsApiKey}>
            <input id="tts-api-key" type="password" value={tts.apiKey} onChange={(event) => tts.changeApiKey(event.target.value)} autoComplete="off" />
          </SettingsField>
        </div>
        <div className="credential-row">
          <small>{tts.provider === "mock" ? "Mock 不需要密钥" : tts.keySaved ? text.apiKeySaved : text.noApiKeySaved}</small>
          <div className="inline">
            <button onClick={tts.deleteKey} disabled={tts.provider === "mock" || !tts.keySaved}>{text.deleteApiKey}</button>
            <button onClick={tts.saveKey} disabled={tts.provider === "mock" || !tts.apiKey.trim()}>
              <Save size={16} />{text.saveApiKey}
            </button>
          </div>
        </div>
        <div className="settings-fields-section">
          <div className="settings-section-heading">
            <strong>连接参数</strong>
            <span>接口地址和模型</span>
          </div>
          <div className="settings-field-grid">
            <SettingsField id="tts-endpoint" label={text.ttsEndpoint}>
              <input id="tts-endpoint" type="url" value={tts.endpoint} onChange={(event) => tts.changeEndpoint(event.target.value)} />
            </SettingsField>
            <SettingsField id="tts-model" label={text.ttsModel}>
              <input id="tts-model" value={tts.model} onChange={(event) => tts.changeModel(event.target.value)} />
            </SettingsField>
          </div>
        </div>
        <div className="settings-fields-section">
          <div className="settings-section-heading">
            <strong>音色与表现</strong>
            <span>用于测试和批量生成的默认值</span>
          </div>
          <div className="settings-field-grid">
            <SettingsField id="tts-voice" label={text.ttsVoiceId} hint="支持音色 ID 或文字描述" wide>
              <div className="settings-voice-row">
                <input id="tts-voice" value={tts.voiceId} onChange={(event) => tts.changeVoiceId(event.target.value)} />
                <button onClick={tts.loadVoices} disabled={Boolean(busy)}><Mic2 size={16} />{text.loadVoices}</button>
                {tts.catalog.length > 0 && (
                  <select aria-label="选择音色" value={tts.voiceId} onChange={(event) => tts.changeVoiceId(event.target.value)}>
                    {tts.catalog.map((voice) => (
                      <option key={voice.provider + ":" + voice.voiceId} value={voice.voiceId}>{voice.name}</option>
                    ))}
                  </select>
                )}
              </div>
            </SettingsField>
            <SettingsField id="tts-style" label={text.ttsStylePrompt} wide>
              <textarea id="tts-style" value={tts.stylePrompt} onChange={(event) => tts.changeStylePrompt(event.target.value)} />
            </SettingsField>
          </div>
        </div>
      </div>
      <div className="settings-test-row">
        <CheckIndicator state={tts.check} />
        <button onClick={tts.test} disabled={tts.check.kind === "running"}>
          {tts.check.kind === "running" ? <Loader2 className="spin" size={16} /> : <Play size={16} />}
          {text.testTts}
        </button>
      </div>
      {tts.testPath && (
        <div className="tts-test-result">
          <small title={tts.testPath}>{text.ttsTestReady}</small>
          <AudioPlayer src={convertFileSrc(tts.testPath)} label="TTS 测试音频" />
          <button title={text.openTtsTestAudio} onClick={tts.openTestAudio}><FolderOpen size={15} />{text.openTtsTestAudio}</button>
        </div>
      )}
    </section>
  );
}

function AudioForm({ settings }: { settings: AppSettingsController }) {
  const audio = settings.audio;
  return (
    <section className="settings-form">
      <p className="settings-description">FFmpeg 用于人工音频转码、静音处理和整集拼接。留空时自动使用系统 PATH。</p>
      <div className="settings-fields">
        <div className="settings-fields-heading">
          <div className="settings-fields-heading-icon"><Scissors size={16} /></div>
          <div>
            <strong>音频工具</strong>
            <span>配置本机音频处理依赖</span>
          </div>
        </div>
        <div className="settings-field-grid">
          <SettingsField id="ffmpeg-path" label={text.ffmpegPath} hint="留空则使用系统 PATH" wide>
            <div className="inline path-display-row">
              <input
                id="ffmpeg-path"
                value={audio.ffmpegPath}
                onChange={(event) => audio.changeFfmpegPath(event.target.value)}
                placeholder="自动检测系统 FFmpeg"
              />
              <button title="选择 FFmpeg" onClick={audio.chooseFfmpeg}><FolderSearch size={16} /></button>
            </div>
          </SettingsField>
        </div>
      </div>
      <div className="settings-test-row">
        <CheckIndicator state={audio.check} />
        <div className="inline">
          <button onClick={() => { audio.changeFfmpegPath(""); void audio.checkFfmpeg(""); }}>自动检测</button>
          <button onClick={() => void audio.checkFfmpeg()} disabled={audio.check.kind === "running"}>
            <Scissors size={16} />验证
          </button>
        </div>
      </div>
    </section>
  );
}

function ExportForm({
  snapshot,
  settings,
  projectCoverPath,
  audioOutputPath,
  onChooseCover,
  onCheckAssets,
  onBackup,
  onOpenAudioFolder,
}: {
  snapshot: StudioSnapshot | null;
  settings: AppSettingsController;
  projectCoverPath: string;
  audioOutputPath: string;
  onChooseCover: () => void;
  onCheckAssets: () => void;
  onBackup: () => void;
  onOpenAudioFolder: () => void;
}) {
  return (
    <section className="settings-form">
      <p className="settings-description">制作音频按项目隔离保存。导出前请在工作台运行发布检查，确保所有分段均有可用音频。</p>
      <div className="settings-fields">
        <div className="settings-fields-heading">
          <div className="settings-fields-heading-icon"><Download size={16} /></div>
          <div>
            <strong>导出设置</strong>
            <span>选择格式并管理当前项目文件</span>
          </div>
        </div>
        <div className="settings-field-grid">
          <SettingsField id="episode-format" label={text.episodeFormat}>
            <select
              id="episode-format"
              value={settings.audio.episodeFormat}
              onChange={(event) => settings.audio.setEpisodeFormat(event.target.value)}
            >
              <option value="m4b">M4B（有声书）</option>
              <option value="mp3">MP3</option>
              <option value="wav">WAV</option>
            </select>
          </SettingsField>
          <SettingsField id="project-cover" label={text.projectCover} hint="M4B 可选" wide>
            <div className="inline path-display-row">
              <input id="project-cover" className="path-display" value={projectCoverPath} readOnly placeholder="支持 JPG / PNG" />
              <button title={text.chooseCover} onClick={onChooseCover} disabled={!snapshot}><Upload size={16} />{text.chooseCover}</button>
            </div>
          </SettingsField>
          <SettingsField id="audio-output" label="当前项目音频目录" wide>
            <div className="inline path-display-row">
              <input id="audio-output" className="path-display" value={audioOutputPath} readOnly placeholder="打开项目后显示" />
              <button title={text.openAudioFolder} onClick={onOpenAudioFolder} disabled={!audioOutputPath}><FolderOpen size={16} /></button>
            </div>
          </SettingsField>
        </div>
      </div>
      <div className="settings-test-row">
        <span className="check-indicator">备份包含项目数据库、脚本、声音资产、音频和导出文件。</span>
        <div className="inline">
          <button onClick={onCheckAssets} disabled={!snapshot}><ShieldCheck size={16} />检查资产</button>
          <button onClick={onBackup} disabled={!snapshot}><Download size={16} />备份项目</button>
        </div>
      </div>
      <div className="export-note">
        <Download size={18} />
        <div><strong>生产导出</strong><span>配音脚本、角色表、分段音频、整集音频和制作包均从工作台导出。</span></div>
      </div>
    </section>
  );
}
