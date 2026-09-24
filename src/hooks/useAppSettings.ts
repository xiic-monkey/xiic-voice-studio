import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { defaultAppSettings, desktopRuntimeMessage, providerDefaults, text } from "../constants";
import type { AppSettings, CheckState, ProviderSettings, VoiceInfo } from "../types";
import { errorMessage, invoke, isHttpUrl } from "../utils";
import type { RunTask } from "./useNotifier";

const idleCheck = (message: string): CheckState => ({ kind: "idle", message });

type Params = {
  desktopRuntime: boolean;
  run: RunTask;
  setNotice: (message: string) => void;
};

/**
 * LLM / TTS / 音频 / 导出四组设置的全部状态与操作。
 * 校验、连接测试、密钥存取、供应商切换都收敛在这里。
 */
export function useAppSettings({ desktopRuntime, run, setNotice }: Params) {
  const [llmBaseUrl, setLlmBaseUrl] = useState("https://api.openai.com/v1");
  const [llmModel, setLlmModel] = useState("gpt-4.1-mini");
  const [llmApiKey, setLlmApiKey] = useState("");
  const [llmKeySaved, setLlmKeySaved] = useState(false);
  // 钥匙串中的当前密钥（明文基线）：用于判断"输入框里的密钥"是否改动过
  const [savedLlmKey, setSavedLlmKey] = useState<string | null>(null);
  const [ttsProvider, setTtsProvider] = useState("mimo");
  const [ttsApiKey, setTtsApiKey] = useState("");
  const [ttsKeySaved, setTtsKeySaved] = useState(false);
  const [savedTtsKey, setSavedTtsKey] = useState<string | null>(null);
  const [ttsEndpoint, setTtsEndpoint] = useState(providerDefaults.mimo.endpoint);
  const [ttsModel, setTtsModel] = useState(providerDefaults.mimo.model);
  const [ttsVoiceId, setTtsVoiceId] = useState(providerDefaults.mimo.voiceId);
  const [ttsStylePrompt, setTtsStylePrompt] = useState(providerDefaults.mimo.stylePrompt);
  const [ffmpegPath, setFfmpegPath] = useState("");
  const [episodeFormat, setEpisodeFormat] = useState("m4b");
  // workspace 不是表单字段：加载后原样持有、保存时原样带回，
  // 否则 dirty 判断永远为真，且保存会重置记住的项目路径。
  const [workspace, setWorkspace] = useState<AppSettings["workspace"]>({ lastProjectRoot: null });
  const [ttsTestPath, setTtsTestPath] = useState("");
  const [voiceCatalog, setVoiceCatalog] = useState<VoiceInfo[]>([]);
  const [savedSettingsJson, setSavedSettingsJson] = useState(() => JSON.stringify(defaultAppSettings));
  const [llmCheck, setLlmCheck] = useState<CheckState>(idleCheck("尚未测试"));
  const [ttsCheck, setTtsCheck] = useState<CheckState>(idleCheck("尚未测试"));
  const [ffmpegCheck, setFfmpegCheck] = useState<CheckState>(idleCheck("尚未检测"));

  useEffect(() => {
    // 浏览器预览模式的提示由 SettingsView 静态横幅负责，
    // 这里不再写 notice，避免与该横幅重复出现两遍同文案。
    if (!desktopRuntime) return;
    invoke<AppSettings>("load_app_settings")
      .then((value) => {
        setLlmBaseUrl(value.llm.baseUrl);
        setLlmModel(value.llm.model);
        setTtsProvider(value.tts.provider);
        setTtsEndpoint(value.tts.endpoint);
        setTtsModel(value.tts.model);
        setTtsVoiceId(value.tts.voiceId);
        setTtsStylePrompt(value.tts.stylePrompt);
        setFfmpegPath(value.audio.ffmpegPath);
        setEpisodeFormat(value.audio.episodeFormat);
        setWorkspace(value.workspace ?? { lastProjectRoot: null });
        setSavedSettingsJson(JSON.stringify(value));
      })
      .catch((error) => setNotice(errorMessage(error)));
    invoke<string | null>("get_provider_api_key", { provider: "llm-openai-compatible" })
      .then((value) => {
        setLlmApiKey(value ?? "");
        setLlmKeySaved(Boolean(value));
        setSavedLlmKey(value ?? null);
      })
      .catch(() => {
        setLlmApiKey("");
        setLlmKeySaved(false);
        setSavedLlmKey(null);
      });
  }, [desktopRuntime, setNotice]);

  useEffect(() => {
    if (!desktopRuntime) return;
    let disposed = false;
    setTtsApiKey("");
    setTtsKeySaved(false);
    setSavedTtsKey(null);
    invoke<string | null>("get_provider_api_key", { provider: ttsProvider })
      .then((value) => {
        if (disposed) return;
        setTtsApiKey(value ?? "");
        setTtsKeySaved(Boolean(value));
        setSavedTtsKey(value ?? null);
      })
      .catch(() => {
        if (disposed) return;
        setTtsApiKey("");
        setTtsKeySaved(false);
        setSavedTtsKey(null);
      });
    return () => {
      disposed = true;
    };
  }, [desktopRuntime, ttsProvider]);

  function resetLlmCheck() {
    setLlmCheck(idleCheck("尚未测试"));
  }

  function resetTtsCheck() {
    setTtsCheck(idleCheck("尚未测试"));
    setTtsTestPath("");
  }

  function resetFfmpegCheck() {
    setFfmpegCheck(idleCheck("尚未检测"));
  }

  function validateLlmSettings() {
    if (!llmBaseUrl.trim()) return "LLM 基础地址不能为空";
    if (!isHttpUrl(llmBaseUrl)) return "LLM 基础地址必须是有效的 HTTP 或 HTTPS 地址";
    if (!llmModel.trim()) return "LLM 模型不能为空";
    return undefined;
  }

  function validateTtsSettings() {
    if (!ttsProvider.trim()) return "TTS 供应商不能为空";
    if (!ttsModel.trim()) return "TTS 模型不能为空";
    if (ttsProvider !== "mock" && !ttsEndpoint.trim()) return "TTS Base URL 不能为空";
    if (ttsEndpoint.trim() && !isHttpUrl(ttsEndpoint)) return "TTS Base URL 必须是有效的 HTTP 或 HTTPS 地址";
    return undefined;
  }

  function validateApplicationSettings() {
    return (
      validateLlmSettings() ??
      validateTtsSettings() ??
      (["wav", "mp3", "m4b"].includes(episodeFormat) ? undefined : "整集格式只能是 WAV、MP3 或 M4B")
    );
  }

  function appSettingsValue(): AppSettings {
    return {
      schemaVersion: 1,
      llm: { baseUrl: llmBaseUrl, model: llmModel },
      tts: { provider: ttsProvider, endpoint: ttsEndpoint, model: ttsModel, voiceId: ttsVoiceId, stylePrompt: ttsStylePrompt },
      audio: { ffmpegPath, episodeFormat },
      workspace,
    };
  }

  function ttsSettings(): ProviderSettings {
    return {
      provider: ttsProvider,
      apiKey: ttsApiKey || undefined,
      endpoint: ttsEndpoint || undefined,
      model: ttsModel || undefined,
    };
  }

  async function testLlm() {
    const validationError = validateLlmSettings();
    if (validationError) {
      setLlmCheck({ kind: "error", message: validationError });
      setNotice(validationError);
      return;
    }
    setLlmCheck({ kind: "running", message: "正在连接…" });
    try {
      const message = await invoke<string>("test_llm", {
        settings: { baseUrl: llmBaseUrl, model: llmModel, apiKey: llmApiKey || undefined },
      });
      setLlmCheck({ kind: "success", message });
      setNotice(message);
    } catch (error) {
      const message = errorMessage(error);
      setLlmCheck({ kind: "error", message });
      setNotice(message);
    }
  }

  async function testTts() {
    const validationError = validateTtsSettings();
    if (validationError) {
      setTtsCheck({ kind: "error", message: validationError });
      setNotice(validationError);
      return;
    }
    setTtsTestPath("");
    setTtsCheck({ kind: "running", message: "正在生成试听…" });
    try {
      const output = await invoke<string>("test_tts", {
        request: {
          settings: ttsSettings(),
          voiceId: ttsVoiceId || (providerDefaults[ttsProvider] ?? providerDefaults.mock).voiceId,
          style: ttsStylePrompt || undefined,
        },
      });
      setTtsTestPath(output);
      setTtsCheck({ kind: "success", message: "连接成功，试听音频已生成" });
      setNotice(`${text.ttsTestReady}：${output}`);
    } catch (error) {
      const message = errorMessage(error);
      setTtsCheck({ kind: "error", message });
      setNotice(message);
    }
  }

  async function checkFfmpeg(path = ffmpegPath) {
    setFfmpegCheck({ kind: "running", message: "正在检测…" });
    try {
      const output = await invoke<string>("check_ffmpeg", { ffmpegPath: path.trim() || undefined });
      setFfmpegCheck({ kind: "success", message: output });
      setNotice(output);
    } catch (error) {
      const message = errorMessage(error);
      setFfmpegCheck({ kind: "error", message });
      setNotice(message);
    }
  }

  async function chooseFfmpeg() {
    if (!desktopRuntime) {
      setNotice(desktopRuntimeMessage);
      return;
    }
    const selected = await open({ multiple: false, title: "选择 FFmpeg 可执行文件" });
    if (!selected || Array.isArray(selected)) return;
    setFfmpegPath(selected);
    await checkFfmpeg(selected);
  }

  async function loadVoices() {
    const value = await run(text.loadingVoices, () => invoke<VoiceInfo[]>("list_voices", { settings: ttsSettings() }));
    if (value) {
      setVoiceCatalog(value);
      setNotice(`${text.voiceCatalogLoaded}：${value.length}`);
    }
  }

  async function saveLlmApiKey() {
    const saved = await run(text.savingApiKey, () =>
      invoke("save_provider_api_key", { provider: "llm-openai-compatible", apiKey: llmApiKey }),
    );
    if (saved !== undefined) {
      setLlmKeySaved(Boolean(llmApiKey.trim()));
      setSavedLlmKey(llmApiKey.trim() || null);
    }
  }

  async function deleteLlmApiKey() {
    const deleted = await run(text.deletingApiKey, () =>
      invoke("delete_provider_api_key", { provider: "llm-openai-compatible" }),
    );
    if (deleted !== undefined) {
      setLlmApiKey("");
      setLlmKeySaved(false);
      setSavedLlmKey(null);
    }
  }

  async function saveTtsApiKey() {
    const saved = await run(text.savingApiKey, () =>
      invoke("save_provider_api_key", { provider: ttsProvider, apiKey: ttsApiKey }),
    );
    if (saved !== undefined) {
      setTtsKeySaved(Boolean(ttsApiKey.trim()));
      setSavedTtsKey(ttsApiKey.trim() || null);
    }
  }

  async function deleteTtsApiKey() {
    const deleted = await run(text.deletingApiKey, () =>
      invoke("delete_provider_api_key", { provider: ttsProvider }),
    );
    if (deleted !== undefined) {
      setTtsApiKey("");
      setTtsKeySaved(false);
      setSavedTtsKey(null);
    }
  }

  async function saveApplicationSettings() {
    const validationError = validateApplicationSettings();
    if (validationError) {
      setNotice(validationError);
      return;
    }
    const value = appSettingsValue();
    await run(
      "保存设置",
      async () => {
        const saved = await invoke<AppSettings>("save_app_settings", { value });
        // 密钥跟随「保存设置」一起写入钥匙串，避免用户只点一个按钮漏存密钥
        const llmKey = llmApiKey.trim();
        if (llmKey && llmKey !== savedLlmKey) {
          await invoke("save_provider_api_key", { provider: "llm-openai-compatible", apiKey: llmKey });
          setLlmKeySaved(true);
          setSavedLlmKey(llmKey);
        }
        const ttsKey = ttsApiKey.trim();
        if (ttsProvider !== "mock" && ttsKey && ttsKey !== savedTtsKey) {
          await invoke("save_provider_api_key", { provider: ttsProvider, apiKey: ttsKey });
          setTtsKeySaved(true);
          setSavedTtsKey(ttsKey);
        }
        return saved;
      },
      (saved) => {
        setSavedSettingsJson(JSON.stringify(saved));
        setNotice("设置已保存，API Key 已写入系统钥匙串");
      },
    );
  }

  async function openTtsTestAudio() {
    if (!ttsTestPath) return;
    await run(text.openTtsTestAudio, () => openPath(ttsTestPath));
  }

  function changeTtsProvider(provider: string) {
    if (provider === ttsProvider) return;
    const defaults = providerDefaults[provider] ?? providerDefaults.mock;
    const currentDefaults = providerDefaults[ttsProvider] ?? providerDefaults.mock;
    const hasCustomTtsValues =
      Boolean(ttsApiKey.trim()) ||
      ttsEndpoint !== currentDefaults.endpoint ||
      ttsModel !== currentDefaults.model ||
      ttsVoiceId !== currentDefaults.voiceId ||
      ttsStylePrompt !== currentDefaults.stylePrompt;
    if (hasCustomTtsValues && !window.confirm("切换供应商会重置当前 TTS 配置，确定继续吗？")) return;
    setVoiceCatalog([]);
    setTtsApiKey("");
    setTtsKeySaved(false);
    resetTtsCheck();
    setTtsProvider(provider);
    setTtsEndpoint(defaults.endpoint);
    setTtsModel(defaults.model);
    setTtsVoiceId(defaults.voiceId);
    setTtsStylePrompt(defaults.stylePrompt);
  }

    // 密钥改动也算"未保存"：填了密钥（或改动过）就让「保存设置」可点
    const llmKeyDirty = Boolean(llmApiKey.trim()) && llmApiKey.trim() !== savedLlmKey;
    const ttsKeyDirty =
      ttsProvider !== "mock" && Boolean(ttsApiKey.trim()) && ttsApiKey.trim() !== savedTtsKey;
    return {
      llm: {
        baseUrl: llmBaseUrl,
        model: llmModel,
        apiKey: llmApiKey,
        keySaved: llmKeySaved,
        check: llmCheck,
        changeBaseUrl: (value: string) => {
          setLlmBaseUrl(value);
          resetLlmCheck();
        },
        changeModel: (value: string) => {
          setLlmModel(value);
          resetLlmCheck();
        },
        changeApiKey: (value: string) => {
          setLlmApiKey(value);
          resetLlmCheck();
        },
        test: testLlm,
        saveKey: saveLlmApiKey,
        deleteKey: deleteLlmApiKey,
      },
    tts: {
      provider: ttsProvider,
      apiKey: ttsApiKey,
      keySaved: ttsKeySaved,
      endpoint: ttsEndpoint,
      model: ttsModel,
      voiceId: ttsVoiceId,
      stylePrompt: ttsStylePrompt,
      check: ttsCheck,
      testPath: ttsTestPath,
      catalog: voiceCatalog,
      settings: ttsSettings(),
      changeProvider: changeTtsProvider,
      changeApiKey: (value: string) => {
        setTtsApiKey(value);
        resetTtsCheck();
      },
      changeEndpoint: (value: string) => {
        setTtsEndpoint(value);
        resetTtsCheck();
      },
      changeModel: (value: string) => {
        setTtsModel(value);
        resetTtsCheck();
      },
      changeVoiceId: (value: string) => {
        setTtsVoiceId(value);
        resetTtsCheck();
      },
      changeStylePrompt: (value: string) => {
        setTtsStylePrompt(value);
        resetTtsCheck();
      },
      test: testTts,
      saveKey: saveTtsApiKey,
      deleteKey: deleteTtsApiKey,
      loadVoices,
      openTestAudio: openTtsTestAudio,
    },
    audio: {
      ffmpegPath,
      episodeFormat,
      check: ffmpegCheck,
      changeFfmpegPath: (value: string) => {
        setFfmpegPath(value);
        resetFfmpegCheck();
      },
      setEpisodeFormat,
      chooseFfmpeg,
      checkFfmpeg,
    },
    settingsDirty:
      savedSettingsJson !== JSON.stringify(appSettingsValue()) || llmKeyDirty || ttsKeyDirty,
    saveApplicationSettings,
    ttsSettings,
  };
}

export type AppSettingsController = ReturnType<typeof useAppSettings>;
