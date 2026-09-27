import { useState } from "react";
import type { StudioSnapshot } from "../types";
import { text } from "../constants";
import { errorMessage, invoke } from "../utils";
import type { RunTask } from "./useNotifier";

type Params = {
  run: RunTask;
  setNotice: (message: string) => void;
  /** 保存/合并成功后由调用方把新快照灌回全局状态。 */
  hydrate: (value: StudioSnapshot) => void;
};

/** 角色资料草稿。在角色声音工坊里编辑，保存时整体提交（后端按整行覆盖）。 */
export type CharacterProfileDraft = {
  canonicalName: string;
  aliases: string;
  gender: string;
  ageTimeline: string;
  notes: string;
};

/**
 * 角色资料的编辑草稿与别名合并。只持有本地编辑态，
 * 持久化动作统一通过 invoke + hydrate 完成。
 */
export function useCharacters({ run, setNotice, hydrate }: Params) {
  const [mergeSourceId, setMergeSourceId] = useState("");
  const [mergeTargetId, setMergeTargetId] = useState("");
  const [addingCharacter, setAddingCharacter] = useState(false);
  const [newCharacterName, setNewCharacterName] = useState("");

  function startAddingCharacter() {
    setAddingCharacter(true);
    setNewCharacterName("");
  }

  function cancelAddingCharacter() {
    setAddingCharacter(false);
    setNewCharacterName("");
  }

  /**
   * 手动新建角色。只收名字 —— 别名/性别/年龄留到建好后用「编辑角色」补，
   * 这样这个入口只承担"把一个角色登记进来"这一件事。
   */
  async function createCharacter() {
    const name = newCharacterName.trim();
    if (!name) {
      setNotice("请先填角色名");
      return;
    }
    const value = await run(
      text.addCharacter,
      () => invoke<StudioSnapshot>("create_character", { request: { canonicalName: name } }),
      hydrate,
    );
    if (value) {
      cancelAddingCharacter();
      setNotice(
        `已新增角色「${name}」，并给了它一条默认音色。到剧本里把它的台词说话人改成它，或先给它定音色。`,
      );
    }
  }

  /**
   * 保存角色资料（角色声音工坊内调用）。
   *
   * 后端 update_character 是整行覆盖：aliases 传空会清光全部别名，
   * gender/age/notes 传空会置 NULL —— 所以调用方必须把快照里的现值
   * 原样带回来，只改真正要改的字段（工坊里的草稿就是按现值初始化的）。
   *
   * 返回 { ok, error }：弹窗内失败必须内联可见，不能只靠被遮罩盖住的提示条。
   */
  async function saveCharacterProfile(
    characterId: string,
    draft: CharacterProfileDraft,
  ): Promise<{ ok: boolean; error?: string }> {
    let outcome: { ok: boolean; error?: string } = { ok: false };
    await run("保存角色资料", async () => {
      try {
        const saved = await invoke<StudioSnapshot>("update_character", {
          request: {
            characterId,
            canonicalName: draft.canonicalName,
            aliases: draft.aliases
              .split(/[、,，\n]/)
              .map((alias) => alias.trim())
              .filter(Boolean),
            gender: draft.gender || undefined,
            ageTimeline: draft.ageTimeline || undefined,
            notes: draft.notes || undefined,
          },
        });
        hydrate(saved);
        outcome = { ok: true };
      } catch (error) {
        outcome = { ok: false, error: errorMessage(error) };
        // 重新抛出让 run 走统一失败通道；弹窗内会再用返回值内联展示
        throw error;
      }
    });
    return outcome;
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
    addingCharacter,
    newCharacterName,
    setNewCharacterName,
    startAddingCharacter,
    cancelAddingCharacter,
    createCharacter,
    saveCharacterProfile,
    mergeCharacters,
    resetMerge,
  };
}

export type CharactersController = ReturnType<typeof useCharacters>;
