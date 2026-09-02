import { useCallback, useState } from "react";
import { text } from "../constants";
import { errorMessage } from "../utils";

/**
 * 全局忙碌态与提示条。所有异步操作统一走 run()，
 * 保证 busy / notice 行为一致，且异常不会以未捕获形式冒到 UI。
 */
export function useNotifier() {
  const [busy, setBusy] = useState("");
  const [notice, setNotice] = useState<string>(text.ready);

  const run = useCallback(async function run<T>(
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
  }, []);

  return { busy, notice, setNotice, setBusy, run };
}

export type RunTask = ReturnType<typeof useNotifier>["run"];
