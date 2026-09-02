import { type CSSProperties } from "react";
import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";
import { desktopRuntimeMessage, jobTypeLabels, statusLabels } from "./constants";

/**
 * 统一的 Tauri 调用入口。浏览器预览模式下直接给出可读错误，
 * 避免每个调用点各写一遍 isTauri 判断。
 */
export function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    return Promise.reject(new Error(desktopRuntimeMessage));
  }
  return tauriInvoke<T>(command, args);
}

export function isHttpUrl(value: string) {
  try {
    const url = new URL(value.trim());
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

export function displayStatus(value: string) {
  return statusLabels[value] ?? value;
}

export function displayJobType(value: string) {
  return jobTypeLabels[value] ?? value;
}

export function formatAudioTime(seconds: number) {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const wholeSeconds = Math.floor(seconds);
  const minutes = Math.floor(wholeSeconds / 60);
  return `${minutes}:${String(wholeSeconds % 60).padStart(2, "0")}`;
}

export function sliderProgress(value: number) {
  return { "--slider-progress": `${Math.max(0, Math.min(100, value))}%` } as CSSProperties;
}

export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}
