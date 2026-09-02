import { useState } from "react";
import type { Character, StudioSnapshot } from "../types";
import { text } from "../constants";
import { invoke } from "../utils";
import type { RunTask } from "./useNotifier";

type Params = {
  run: RunTask;
  setNotice: (message: string) => void;
  /** 保存/合并成功后由调用方把新快照灌回全局状态。 */
  hydrate: (value: StudioSnapshot) => void;
};

/**
 * 角色资料的编辑草稿与别名合并。只持有本地编辑态，
 * 持久化动作统一通过 invoke + hydrate 完成。
 */
export function useCharacters({ run, setNotice, hydrate }: Params) {
  const [mergeSourceId, setMergeSourceId] = useState("");
  const [mergeTargetId, setMergeTargetId] = useState("");
  const [editingCharacterId, setEditingCharacterId] = useState("");
  const [characterDraft, setCharacterDraft] = useState({
    canonicalName: "",
    aliases: "",
    gender: "",
    ageTimeline: "",
    notes: "",
  });

  function startEditingCharacter(character: Character) {
    setEditingCharacterId(character.id);
    setCharacterDraft({
      canonicalName: character.canonicalName,
      aliases: character.aliases.join("、"),
      gender: character.gender ?? "",
      ageTimeline: character.ageTimeline ?? "",
      notes: character.notes ?? "",
    });
  }

  function cancelEditingCharacter() {
    setEditingCharacterId("");
  }

  async function saveCharacter() {
    if (!editingCharacterId) return;
    const saved = await run(
      "保存角色",
      () =>
        invoke<StudioSnapshot>("update_character", {
          request: {
            characterId: editingCharacterId,
            canonicalName: characterDraft.canonicalName,
            aliases: characterDraft.aliases
              .split(/[、,，\n]/)
              .map((alias) => alias.trim())
              .filter(Boolean),
            gender: characterDraft.gender || undefined,
            ageTimeline: characterDraft.ageTimeline || undefined,
            notes: characterDraft.notes || undefined,
          },
        }),
      hydrate,
    );
    if (saved) cancelEditingCharacter();
  }

  async function mergeCharacters() {
    if (!mergeSourceId || !mergeTargetId || mergeSourceId === mergeTargetId) {
      setNotice("请选择不同的来源角色和目标角色");
      return;
    }
    const value = await run(
      text.mergeCharacters,
      () =>
        invoke<StudioSnapshot>("merge_characters", {
          sourceCharacterId: mergeSourceId,
          targetCharacterId: mergeTargetId,
        }),
      hydrate,
    );
    if (value) resetMerge();
  }

  function resetMerge() {
    setMergeSourceId("");
    setMergeTargetId("");
  }

  return {
    mergeSourceId,
    setMergeSourceId,
    mergeTargetId,
    setMergeTargetId,
    editingCharacterId,
    characterDraft,
    setCharacterDraft,
    startEditingCharacter,
    cancelEditingCharacter,
    saveCharacter,
    mergeCharacters,
    resetMerge,
  };
}

export type CharactersController = ReturnType<typeof useCharacters>;
