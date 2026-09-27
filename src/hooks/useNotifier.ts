import { useCallback, useState } from "react";
import { text } from "../constants";
import { errorMessage } from "../utils";

/**
 * 失败提示的识别词。notice 只有一条字符串通道，
 * 这里仅用于给浮层提示上色（错误=红、正常=中性），不影响文案本身。
 */
const ERROR_HINTS = [
  "失败",
  "错误",
  "异常",
  "无法",
  "不能",
  "请先",
  "还没",
  "缺少",
  "不存在",
  "已丢失",
  "无效",
  "拒绝",
  "超时",
  "不支持",
  "仅支持",
  "未配置",
  "格式无效",
  "解析失败",
];

function toneOf(message: string): "info" | "error" {
  return ERROR_HINTS.some((hint) => message.includes(hint)) ? "error" : "info";
}

/**
 * 一条可撤销操作。挂在提示浮层上：用户点「撤销」就执行 run()。
 * 过期后按钮消失——但数据本身还在后端归档里（见 DELETED_SEGMENT_RETAIN_DAYS）。
 */
export type UndoHandle = {
  /** 按钮文字，通常是「撤销」。 */
  label: string;
  /** 撤销动作。 */
  run: () => void | Promise<void>;
  /** 这个按钮存活多久（毫秒）。 */
  timeoutMs: number;
};

/** 撤销按钮的默认存活时间。比普通提示长得多：误触之后要留出反应时间。 */
export const UNDO_TIMEOUT_MS = 30_000;

/**
 * 全局忙碌态与提示条。所有异步操作统一走 run()，
 * 保证 busy / notice 行为一致，且异常不会以未捕获形式冒到 UI。
 */
export function useNotifier() {
  const [busy, setBusy] = useState("");
  const [notice, setNoticeState] = useState<string>(text.ready);
  const [noticeTone, setNoticeTone] = useState<"info" | "error">("info");
  const [undo, setUndoState] = useState<UndoHandle | null>(null);

  const setNotice = useCallback((message: string) => {
    setNoticeState(message);
    setNoticeTone(toneOf(message));
    // 新提示顶掉旧提示，旧提示上的撤销按钮自然也就失效了
    setUndoState(null);
  }, []);

  /**
   * 设置一条带「撤销」按钮的提示。
   *
   * 这里取代了删除前的确认弹窗：确认框是"每次都要读一遍、但没人真读"的成本，
   * 而撤销只在真的误触时才被用到。两者都做则是双份打扰。
   */
  const setUndo = useCallback(
    (message: string, handle: { label: string; run: () => void | Promise<void> }) => {
      setNoticeState(message);
      setNoticeTone(toneOf(message));
      setUndoState({ ...handle, timeoutMs: UNDO_TIMEOUT_MS });
    },
    [],
  );

  const clearUndo = useCallback(() => setUndoState(null), []);

  const run = useCallback(
    async function run<T>(
      label: string,
      action: () => Promise<T>,
      onDone?: (value: T) => void,
    ): Promise<T | undefined> {
      setBusy(label);
      setNotice(label);
      try {
        const value = await action();
        onDone?.(value);
        setNotice(`${label}${text.done}`);
        return value;
      } catch (error) {
        setNotice(errorMessage(error));
        return undefined;
      } finally {
        setBusy("");
      }
    },
    [setNotice],
  );

  return { busy, notice, noticeTone, setNotice, setBusy, undo, setUndo, clearUndo, run };
}

export type RunTask = ReturnType<typeof useNotifier>["run"];
