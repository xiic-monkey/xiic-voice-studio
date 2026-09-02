import type { ReactNode } from "react";
import { displayStatus } from "../utils";
import type { CheckState } from "../types";

/** 侧栏 / 检查器里的标准面板。图标统一由标题首位渲染，保证左边缘对齐。 */
export function Panel({
  title,
  icon,
  count,
  className,
  children,
}: {
  title?: ReactNode;
  icon?: ReactNode;
  count?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section className={className ? `panel ${className}` : "panel"}>
      {title !== undefined && (
        <div className="panel-title">
          {icon}
          <span>{title}</span>
          {count !== undefined && <span className="count">{count}</span>}
        </div>
      )}
      {children}
    </section>
  );
}

/**
 * 音频状态与审听状态拆成两枚徽章。
 * 原先挤在同一个胶囊里，共用一套配色，无法区分是缺音频还是需要返修。
 */
export function SegmentStatus({ audioStatus, reviewStatus }: { audioStatus: string; reviewStatus: string }) {
  return (
    <div className="status-group">
      <span className={`status ${audioStatus}`}>{displayStatus(audioStatus)}</span>
      <span className={`status ${reviewStatus}`}>{displayStatus(reviewStatus)}</span>
    </div>
  );
}

export function CheckIndicator({ state }: { state: CheckState }) {
  return (
    <div className={`check-indicator ${state.kind}`} role="status" aria-live="polite">
      {state.message}
    </div>
  );
}

export function SettingsField({
  id,
  label,
  hint,
  wide = false,
  children,
}: {
  id: string;
  label: string;
  hint?: string;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={wide ? "settings-field settings-field-wide" : "settings-field"}>
      <div className="settings-field-label">
        <label htmlFor={id}>{label}</label>
        {hint && <span>{hint}</span>}
      </div>
      {children}
    </div>
  );
}

export function EmptyState({ children }: { children: ReactNode }) {
  return <div className="empty-state">{children}</div>;
}
