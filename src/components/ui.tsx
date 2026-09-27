import type { ReactNode } from "react";
import { HelpCircle } from "lucide-react";
import { displayStatus } from "../utils";
import type { CheckState } from "../types";

/** 侧栏 / 检查器里的标准面板。图标统一由标题首位渲染，保证左边缘对齐。 */
export function Panel({
  title,
  icon,
  count,
  action,
  className,
  children,
}: {
  title?: ReactNode;
  icon?: ReactNode;
  count?: ReactNode;
  action?: ReactNode;
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
          {action !== undefined && <span className="panel-action">{action}</span>}
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

/**
 * 常驻说明文字的替代品：一枚「?」图标，hover / 键盘聚焦时才展开气泡。
 * 卡片里长期挂一行小字会白白抬高行高、也容易变成谁都不读的废话，
 * 说明性内容应该按需出现，而不是占用版面。
 */
export function HintTip({ label, children }: { label: string; children: ReactNode }) {
  return (
    <span className="hint-tip">
      <button type="button" className="hint-tip-trigger" aria-label={label}>
        <HelpCircle size={12} />
      </button>
      <span className="hint-tip-bubble" role="tooltip">
        {children}
      </span>
    </span>
  );
}
