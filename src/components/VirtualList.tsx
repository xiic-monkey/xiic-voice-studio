import { useEffect, useRef, useState, type ReactNode } from "react";

type VirtualListProps<T> = {
  items: T[];
  /** 行高固定才能做窗口化，调用方负责让行内容撑满这个高度 */
  rowHeight: number;
  overscan?: number;
  className?: string;
  /** 变化时把滚动位置重置回顶部，比如切换搜索关键字 */
  resetKey?: string;
  renderItem: (item: T, index: number) => ReactNode;
  emptyState?: ReactNode;
};

/**
 * 轻量窗口化列表：只渲染视口内的行。
 * 章节动辄上千条，全量渲染会卡，且没有引入虚拟列表依赖的必要。
 */
export function VirtualList<T>({
  items,
  rowHeight,
  overscan = 8,
  className,
  resetKey,
  renderItem,
  emptyState,
}: VirtualListProps<T>) {
  const scrollerRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);

  useEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => setViewportHeight(el.clientHeight));
    observer.observe(el);
    setViewportHeight(el.clientHeight);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const el = scrollerRef.current;
    if (el) el.scrollTop = 0;
    setScrollTop(0);
  }, [resetKey]);

  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const end = Math.min(items.length, Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan);
  const visible = items.slice(start, end);

  return (
    <div
      ref={scrollerRef}
      className={className}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
    >
      {!items.length && emptyState}
      {items.length > 0 && (
        <>
          {start > 0 && <div style={{ height: start * rowHeight, flexShrink: 0 }} />}
          {visible.map((item, index) => renderItem(item, start + index))}
          {end < items.length && <div style={{ height: (items.length - end) * rowHeight, flexShrink: 0 }} />}
        </>
      )}
    </div>
  );
}
