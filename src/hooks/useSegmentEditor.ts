import { useState } from "react";
import type { Segment, SegmentDraft } from "../types";

function fallbackDraft(segment: Segment): SegmentDraft {
  return {
    text: segment.text,
    segmentType: segment.segmentType,
    speaker: segment.speaker ?? "",
    characterId: segment.characterId ?? "",
  };
}

/**
 * 分段编辑草稿。只在本地持有未提交的改动，
 * 服务端确认后按内容比对丢弃草稿，避免误删保存期间的新输入。
 */
export function useSegmentEditor() {
  const [drafts, setDrafts] = useState<Record<string, SegmentDraft>>({});

  function draftFor(segment: Segment): SegmentDraft {
    return drafts[segment.id] ?? fallbackDraft(segment);
  }

  function update(segment: Segment, patch: Partial<SegmentDraft>) {
    setDrafts((current) => ({
      ...current,
      [segment.id]: { ...(current[segment.id] ?? fallbackDraft(segment)), ...patch },
    }));
  }

  /** 保存成功后调用：若用户在此期间又改了内容，则保留草稿不清空。 */
  function drop(segmentId: string, submitted: SegmentDraft) {
    setDrafts((current) => {
      const currentDraft = current[segmentId];
      if (
        currentDraft &&
        (currentDraft.text !== submitted.text ||
          currentDraft.segmentType !== submitted.segmentType ||
          currentDraft.speaker !== submitted.speaker ||
          (currentDraft.characterId ?? "") !== (submitted.characterId ?? "") ||
        (currentDraft.emotion ?? "") !== (submitted.emotion ?? ""))
      ) {
        return current;
      }
      const next = { ...current };
      delete next[segmentId];
      return next;
    });
  }

  function clear() {
    setDrafts({});
  }

  return { drafts, draftFor, update, drop, clear };
}

export type SegmentEditorController = ReturnType<typeof useSegmentEditor>;
