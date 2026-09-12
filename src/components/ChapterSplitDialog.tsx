import { useEffect, useMemo, useState } from "react";
import { Loader2, Sparkles, Wand2, X } from "lucide-react";
import type { SplitPreview, SplitRuleDto } from "../types";
import { text } from "../constants";
import { errorMessage, invoke } from "../utils";

type Mode = "heuristic" | "rule" | "custom";

type LlmInfo = { baseUrl: string; model: string; apiKey: string; keySaved: boolean };

type Props = {
  sourcePath: string;
  llm: LlmInfo;
  busy: string;
  onNotice: (message: string) => void;
  onClose: () => void;
  onConfirm: (pattern: string | null) => void;
};

const PREVIEW_DELAY_MS = 300;

export function ChapterSplitDialog({ sourcePath, llm, busy, onNotice, onClose, onConfirm }: Props) {
  const [rules, setRules] = useState<SplitRuleDto[]>([]);
  const [mode, setMode] = useState<Mode>("heuristic");
  const [selectedRuleId, setSelectedRuleId] = useState<string>("");
  const [customRegex, setCustomRegex] = useState("");
  const [hint, setHint] = useState("");
  const [preview, setPreview] = useState<SplitPreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [parsing, setParsing] = useState(false);

  // 载入内置拆章规则；默认选中第一条，保证「选择规则」模式有初始值。
  useEffect(() => {
    invoke<SplitRuleDto[]>("list_chapter_rules")
      .then((value) => {
        setRules(value);
        if (value.length > 0) setSelectedRuleId(value[0].id);
      })
      .catch((error) => onNotice(errorMessage(error)));
  }, [onNotice]);

  const activePattern = useMemo<string | null>(() => {
    if (mode === "heuristic") return null;
    if (mode === "rule") {
      return rules.find((rule) => rule.id === selectedRuleId)?.pattern ?? null;
    }
    return customRegex.trim() ? customRegex.trim() : null;
  }, [mode, selectedRuleId, customRegex, rules]);

  // 模式或正则变化时，防抖拉取拆章预览。
  useEffect(() => {
    if (mode === "custom" && !customRegex.trim()) {
      setPreview(null);
      setPreviewError(null);
      return;
    }
    setPreviewLoading(true);
    setPreviewError(null);
    const handle = setTimeout(() => {
      invoke<SplitPreview>("preview_chapter_split", {
        request: { sourcePath, chapterPattern: activePattern },
      })
        .then(setPreview)
        .catch((error) => {
          setPreviewError(errorMessage(error));
          setPreview(null);
        })
        .finally(() => setPreviewLoading(false));
    }, PREVIEW_DELAY_MS);
    return () => clearTimeout(handle);
  }, [mode, selectedRuleId, customRegex, sourcePath, activePattern]);

  async function handleAiParse() {
    if (!llm.keySaved && !llm.apiKey.trim()) {
      onNotice(text.splitNeedLlm);
      return;
    }
    setParsing(true);
    try {
      const pattern = await invoke<string>("detect_chapter_rule", {
        request: {
          sourcePath,
          hint: hint.trim() || null,
          settings: { baseUrl: llm.baseUrl, model: llm.model, apiKey: llm.apiKey || undefined },
        },
      });
      setCustomRegex(pattern);
      setMode("custom");
      onNotice(`AI 已生成正则：${pattern}`);
    } catch (error) {
      onNotice(errorMessage(error));
    } finally {
      setParsing(false);
    }
  }

  const ruleSourceLabel =
    preview?.ruleSource === "heuristic"
      ? text.splitHeuristicSource
      : preview?.ruleSource === "pattern"
        ? text.splitPatternSource
        : preview?.ruleSource === "single"
          ? text.splitSingleSource
          : "";

  const canConfirm =
    !busy &&
    (mode === "heuristic" ||
      (mode === "rule" && Boolean(activePattern)) ||
      (mode === "custom" && Boolean(activePattern) && !previewError));

  function handleConfirm() {
    if (!canConfirm) return;
    onConfirm(activePattern);
  }

  return (
    <div className="modal-backdrop">
      <section className="split-dialog" role="dialog" aria-modal="true" aria-labelledby="split-dialog-title">
        <header className="split-dialog-header">
          <div>
            <span>稿件导入</span>
            <h1 id="split-dialog-title">{text.splitDialogTitle}</h1>
          </div>
          <button className="icon-button" title="关闭" onClick={onClose} disabled={Boolean(busy)}>
            <X size={18} />
          </button>
        </header>

        <div className="split-dialog-body">
          <div className="split-options">
            <RuleOption
              selected={mode === "heuristic"}
              title={text.splitHeuristic}
              hint={text.splitHeuristicHint}
              onSelect={() => setMode("heuristic")}
            />

            <RuleOption
              selected={mode === "rule"}
              title={text.splitPickRule}
              onSelect={() => setMode("rule")}
            >
              {mode === "rule" && (
                <div className="rule-list">
                  {rules.map((rule) => (
                    <label key={rule.id} className={`rule-item ${selectedRuleId === rule.id ? "selected" : ""}`}>
                      <input
                        type="radio"
                        name="split-rule"
                        value={rule.id}
                        checked={selectedRuleId === rule.id}
                        onChange={() => setSelectedRuleId(rule.id)}
                      />
                      <span className="rule-label">{rule.label}</span>
                      <span className="rule-desc">{rule.description}</span>
                      <code className="rule-pattern">{rule.pattern}</code>
                    </label>
                  ))}
                </div>
              )}
            </RuleOption>

            <RuleOption
              selected={mode === "custom"}
              title={text.splitCustom}
              hint={text.splitCustomHint}
              onSelect={() => setMode("custom")}
            >
              {mode === "custom" && (
                <div className="custom-regex-area">
                  <label className="custom-field">
                    <span>{text.splitFormatHintLabel}</span>
                    <input
                      type="text"
                      value={hint}
                      placeholder={text.splitFormatHintPlaceholder}
                      onChange={(event) => setHint(event.target.value)}
                    />
                  </label>
                  <button
                    className="ai-parse"
                    onClick={handleAiParse}
                    disabled={parsing || Boolean(busy)}
                  >
                    {parsing ? <Loader2 size={15} className="spin" /> : <Sparkles size={15} />}
                    {text.splitAiParse}
                  </button>
                  <span className="custom-hint">{text.splitAiParseHint}</span>
                  <textarea
                    className="custom-regex-input"
                    value={customRegex}
                    placeholder="^第[〇零0-9０-９一二三四五六七八九十百千两]+\s*[章节回卷集部篇]"
                    spellCheck={false}
                    rows={3}
                    onChange={(event) => setCustomRegex(event.target.value)}
                  />
                </div>
              )}
            </RuleOption>
          </div>

          <div className="split-preview">
            <div className="split-preview-head">
              <h2>{text.splitPreview}</h2>
              {previewLoading && <Loader2 size={15} className="spin" />}
            </div>

            {previewError && <div className="split-preview-error">{previewError}</div>}

            {!previewError && preview && (
              <>
                <div className="split-preview-meta">
                  <span className={`split-source-badge ${preview.ruleSource}`}>{ruleSourceLabel}</span>
                  <span>
                    预计拆出 <strong>{preview.chapterCount}</strong> 章
                  </span>
                </div>

                <div className="split-preview-section">
                  <span className="split-preview-label">{text.splitMatchedLines}</span>
                  {preview.matchedLines.length === 0 ? (
                    <p className="split-empty-match">{text.splitNoMatch}</p>
                  ) : (
                    <ol className="split-matched-list">
                      {preview.matchedLines.map((line, index) => (
                        <li key={index}>{line}</li>
                      ))}
                    </ol>
                  )}
                </div>
              </>
            )}

            {!previewError && !preview && !previewLoading && mode === "custom" && (
              <p className="split-empty-match">{text.splitCustomHint}</p>
            )}
          </div>
        </div>

        <footer className="split-dialog-footer">
          <button className="ghost" onClick={onClose} disabled={Boolean(busy)}>
            {text.cancel}
          </button>
          <button className="primary-action" onClick={handleConfirm} disabled={!canConfirm}>
            {busy ? <Loader2 size={15} className="spin" /> : <Wand2 size={15} />}
            {text.splitConfirmImport}
          </button>
        </footer>
      </section>
    </div>
  );
}

function RuleOption({
  selected,
  title,
  hint,
  onSelect,
  children,
}: {
  selected: boolean;
  title: string;
  hint?: string;
  onSelect: () => void;
  children?: React.ReactNode;
}) {
  return (
    <div className={`rule-option ${selected ? "selected" : ""}`}>
      <button className="rule-option-head" onClick={onSelect} type="button">
        <span className={`rule-radio ${selected ? "on" : ""}`} />
        <span className="rule-option-title">{title}</span>
      </button>
      {hint && <p className="rule-option-hint">{hint}</p>}
      {children}
    </div>
  );
}
