import { useEffect, useRef } from "react";

/**
 * active 为 true 时监听 Escape 键，用于弹窗 / 浮层的键盘关闭。
 * 回调存进 ref，避免内联闭包导致监听器反复卸载重挂。
 */
export function useEscapeKey(active: boolean, onEscape: () => void) {
  const handlerRef = useRef(onEscape);
  handlerRef.current = onEscape;

  useEffect(() => {
    if (!active) return;
    const handle = (event: KeyboardEvent) => {
      if (event.key === "Escape") handlerRef.current();
    };
    window.addEventListener("keydown", handle);
    return () => window.removeEventListener("keydown", handle);
  }, [active]);
}
