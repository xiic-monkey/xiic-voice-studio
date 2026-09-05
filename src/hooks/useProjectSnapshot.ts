import { useEffect, useMemo, useRef, useState } from "react";
import type { Chapter, Segment, StudioJob, StudioSnapshot } from "../types";
import { invoke } from "../utils";
import { createMockSnapshot, devPreviewEnabled } from "../dev/preview";

/**
 * 项目快照、章节/分段选中态、导出范围。
 * jobs 轮询做了内容比对：任务无变化时不再写 state，
 * 避免每秒把整棵树重渲染一遍。
 */
export function useProjectSnapshot(onProjectChanged?: () => void) {
  const [snapshot, setSnapshot] = useState<StudioSnapshot | null>(() =>
    devPreviewEnabled ? createMockSnapshot() : null,
  );
  const [selectedChapterId, setSelectedChapterId] = useState<string>("");
  const [selectedSegmentId, setSelectedSegmentId] = useState<string>("");
  const [exportAllChapters, setExportAllChapters] = useState(true);
  const [exportChapterIds, setExportChapterIds] = useState<string[]>([]);

  const projectChangedRef = useRef(onProjectChanged);
  projectChangedRef.current = onProjectChanged;

  const rootPath = snapshot?.project.rootPath;

  useEffect(() => {
    if (!rootPath) return;
    let disposed = false;
    let lastJobsJson = "";
    const refreshJobs = () => {
      invoke<StudioJob[]>("list_jobs")
        .then((jobs) => {
          if (disposed) return;
          const json = JSON.stringify(jobs);
          if (json === lastJobsJson) return;
          lastJobsJson = json;
          setSnapshot((current) => (current ? { ...current, jobs } : current));
        })
        .catch(() => undefined);
    };
    refreshJobs();
    const timer = window.setInterval(refreshJobs, 1000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [rootPath]);

  const chapters = snapshot?.chapters ?? [];
  const allSegments = snapshot?.segments ?? [];
  const activeChapterId = selectedChapterId || chapters[0]?.id || "";

  const visibleSegments = useMemo(
    () => allSegments.filter((segment) => !activeChapterId || segment.chapterId === activeChapterId),
    [activeChapterId, allSegments],
  );

  const selectedSegment = visibleSegments.find((segment) => segment.id === selectedSegmentId) ?? visibleSegments[0];

  const selectedReviewIssues = useMemo(
    () =>
      snapshot?.reviewIssues.filter((issue) => issue.segmentId === selectedSegment?.id && issue.status === "open") ??
      [],
    [snapshot?.reviewIssues, selectedSegment?.id],
  );

  const activeJobs = useMemo(
    () => snapshot?.jobs.filter((job) => job.status === "running" || job.status === "pending") ?? [],
    [snapshot?.jobs],
  );

  const selectedExportChapterIds = useMemo(
    () =>
      exportAllChapters
        ? chapters.map((chapter) => chapter.id)
        : exportChapterIds.filter((id) => chapters.some((chapter) => chapter.id === id)),
    [chapters, exportAllChapters, exportChapterIds],
  );

  function hydrateSnapshot(value: StudioSnapshot) {
    const projectChanged = snapshot?.project.rootPath !== value.project.rootPath;
    setSnapshot(value);
    setSelectedChapterId((current) =>
      value.chapters.some((chapter) => chapter.id === current) ? current : value.chapters[0]?.id || "",
    );
    setSelectedSegmentId((current) =>
      value.segments.some((segment) => segment.id === current) ? current : value.segments[0]?.id || "",
    );
    if (projectChanged) {
      setExportAllChapters(true);
      setExportChapterIds([]);
      projectChangedRef.current?.();
    }
  }

  function toggleExportChapter(chapterId: string) {
    if (exportAllChapters) {
      setExportAllChapters(false);
      setExportChapterIds(chapters.filter((chapter) => chapter.id !== chapterId).map((chapter) => chapter.id));
      return;
    }
    setExportChapterIds((current) => {
      const next = current.includes(chapterId)
        ? current.filter((id) => id !== chapterId)
        : [...current, chapterId];
      if (next.length === chapters.length) {
        setExportAllChapters(true);
        return [];
      }
      return next;
    });
  }

  function setExportScopeAll(all: boolean, allChapterIds: string[]) {
    setExportAllChapters(all);
    setExportChapterIds(all ? [] : allChapterIds);
  }

  function exportChapterArgs() {
    return exportAllChapters ? undefined : selectedExportChapterIds;
  }

  function setSegmentJobs(jobs: StudioJob[]) {
    setSnapshot((current) => (current ? { ...current, jobs } : current));
  }

  return {
    snapshot,
    setSnapshot,
    chapters,
    segments: visibleSegments satisfies Segment[],
    activeChapterId,
    selectedChapterId,
    setSelectedChapterId,
    selectedSegmentId,
    setSelectedSegmentId,
    selectedSegment,
    selectedReviewIssues,
    activeJobs,
    activeChapter: chapters.find((chapter) => chapter.id === activeChapterId) as Chapter | undefined,
    hydrateSnapshot,
    setSegmentJobs,
    exportScope: {
      allChapters: exportAllChapters,
      chapterIds: exportChapterIds,
      selectedIds: selectedExportChapterIds,
      toggle: toggleExportChapter,
      setAll: setExportScopeAll,
      args: exportChapterArgs,
    },
  };
}

export type ProjectSnapshotController = ReturnType<typeof useProjectSnapshot>;
