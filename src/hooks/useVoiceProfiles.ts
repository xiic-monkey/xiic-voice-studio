import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { ProviderSettings, StudioSnapshot, VoiceProfile } from "../types";
import { ageStages, desktopRuntimeMessage } from "../constants";
import { invoke } from "../utils";
import type { RunTask } from "./useNotifier";

const defaultCloneStyle = "自然、稳定，保持参考音色特征，适合长篇有声读物";

type Params = {
  desktopRuntime: boolean;
  run: RunTask;
  setNotice: (message: string) => void;
  ttsSettings: () => ProviderSettings;
  hydrate: (value: StudioSnapshot) => void;
  /** 模仿音色创建成功后回到声音库视图。 */
  onCloneCreated?: () => void;
};

/**
 * 声音档案编辑（年龄时间轴 / 语速音高 / 表演提示）与 Mimo 音色模仿。
 * 试听音频路径只保留最新一条，避免堆叠播放器。
 */
export function useVoiceProfiles({ desktopRuntime, run, setNotice, ttsSettings, hydrate, onCloneCreated }: Params) {
  const [editingVoiceProfileId, setEditingVoiceProfileId] = useState("");
  const [voiceDraft, setVoiceDraft] = useState({
    name: "",
    ageStage: "adult",
    ttsProvider: "mimo",
    model: "",
    voiceId: "",
    voiceAssetId: "",
    speed: 1,
    pitch: 0,
    style: "",
  });
  const [cloneName, setCloneName] = useState("模仿音色");
  const [cloneCharacterId, setCloneCharacterId] = useState("");
  const [cloneAgeStage, setCloneAgeStage] = useState("adult");
  const [cloneSourcePath, setCloneSourcePath] = useState("");
  const [cloneStyle, setCloneStyle] = useState(defaultCloneStyle);
  const [cloneConsent, setCloneConsent] = useState(false);
  const [voicePreviewPath, setVoicePreviewPath] = useState("");

  async function previewVoiceProfile(profileId: string) {
    const output = await run("生成音色试听", () =>
      invoke<string>("test_voice_profile", {
        request: { profileId, settings: ttsSettings() },
      }),
    );
    if (output) setVoicePreviewPath(output);
  }

  function startEditingVoiceProfile(profile: VoiceProfile) {
    setEditingVoiceProfileId(profile.id);
    setVoiceDraft({
      name: profile.name,
      ageStage: profile.ageStage,
      ttsProvider: profile.ttsProvider,
      model: profile.model ?? "",
      voiceId: profile.voiceId,
      voiceAssetId: profile.voiceAssetId ?? "",
      speed: profile.speed,
      pitch: profile.pitch,
      style: profile.style ?? "",
    });
  }

  function cancelEditingVoiceProfile() {
    setEditingVoiceProfileId("");
  }

  async function saveVoiceProfile() {
    if (!editingVoiceProfileId) return;
    const saved = await run(
      "保存声音档案",
      () =>
        invoke<StudioSnapshot>("update_voice_profile", {
          request: {
            profileId: editingVoiceProfileId,
            name: voiceDraft.name,
            ageStage: voiceDraft.ageStage,
            ttsProvider: voiceDraft.ttsProvider,
            model: voiceDraft.model || undefined,
            voiceId: voiceDraft.voiceId,
            voiceAssetId: voiceDraft.voiceAssetId || undefined,
            speed: voiceDraft.speed,
            pitch: voiceDraft.pitch,
            style: voiceDraft.style || undefined,
          },
        }),
      hydrate,
    );
    if (saved) cancelEditingVoiceProfile();
  }

  async function deleteVoiceProfile(profile: VoiceProfile) {
    if (!window.confirm(`确定删除声音档案“${profile.name}”？已生成的人工上传音频会保留。`)) return;
    const deleted = await run(
      "删除声音档案",
      () => invoke<StudioSnapshot>("delete_voice_profile", { profileId: profile.id }),
      hydrate,
    );
    if (deleted) cancelEditingVoiceProfile();
  }

  async function chooseCloneSample() {
    if (!desktopRuntime) {
      setNotice(desktopRuntimeMessage);
      return;
    }
    const sourcePath = await open({
      multiple: false,
      title: "选择音色参考音频",
      filters: [{ name: "Mimo 参考音频", extensions: ["mp3", "wav"] }],
    });
    if (!sourcePath || Array.isArray(sourcePath)) return;
    setCloneSourcePath(sourcePath);
  }

  async function createMimoVoiceClone() {
    if (!cloneSourcePath) {
      setNotice("请选择 MP3 或 WAV 参考音频");
      return;
    }
    if (!cloneConsent) {
      setNotice("请先确认已获得声音使用授权");
      return;
    }
    const value = await run(
      "创建模仿音色",
      () =>
        invoke<StudioSnapshot>("create_mimo_voice_clone", {
          request: {
            characterId: cloneCharacterId || undefined,
            name: cloneName,
            ageStage: cloneAgeStage,
            sourcePath: cloneSourcePath,
            style: cloneStyle || undefined,
            consentConfirmed: cloneConsent,
          },
        }),
      hydrate,
    );
    if (value) {
      setCloneSourcePath("");
      setCloneConsent(false);
      onCloneCreated?.();
    }
  }

  return {
    editingVoiceProfileId,
    voiceDraft,
    setVoiceDraft,
    startEditingVoiceProfile,
    cancelEditingVoiceProfile,
    saveVoiceProfile,
    deleteVoiceProfile,
    previewVoiceProfile,
    clone: {
      name: cloneName,
      setName: setCloneName,
      characterId: cloneCharacterId,
      setCharacterId: setCloneCharacterId,
      ageStage: cloneAgeStage,
      setAgeStage: setCloneAgeStage,
      ageStages,
      sourcePath: cloneSourcePath,
      chooseSource: chooseCloneSample,
      style: cloneStyle,
      setStyle: setCloneStyle,
      consent: cloneConsent,
      setConsent: setCloneConsent,
      create: createMimoVoiceClone,
    },
    voicePreviewPath,
  };
}

export type VoiceProfilesController = ReturnType<typeof useVoiceProfiles>;
