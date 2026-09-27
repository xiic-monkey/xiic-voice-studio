# 分段纠错改造方案（参考画本妖鸡 / 爱画本）

> 目标：消灭「分段错误无法纠正」这一致命缺陷。
> 结论先行：问题不在 AI 拆得不准，而在于**系统没有给「纠正分段」提供任何结构性操作**。竞品的共同做法是把分段建模成「文本范围上的软属性」，可任意拆/合/转/插；我们是「一次性生成的不可变行」。

---

## 一、当前缺陷根因（带代码定位）

### 根因 1：后端只开放「改文本 / 删行」两种写操作
- `src-tauri/src/lib.rs:368` `update_segment`：只能改 `text / segment_type / speaker / character_id / emotion`。任何改动都会把行标记为 `is_manual_edit = 1`。
- `src-tauri/src/lib.rs:424` `delete_segment`：只能删除整行。
- **没有任何** `split_segment`（在光标处拆成两句）、`merge_segments`（相邻行合并）、`insert_segment`（在两行之间插入漏掉的台词）。

后果：AI 把「A。B。」错拆成两行、或该一句却被并成一行时，用户**无法在结构上修正**——只能去改文本凑字数，或把一行删掉（连台词一起没了）。

### 根因 2：「整章重拆」是 all-or-nothing，且会丢弃手工修正
- `src-tauri/src/importer.rs:228` `seed_segments_from_chapter`：第 239–241 行，一旦章节里存在 `is_manual_edit = 1` 的分段，直接报错「章节中已有人工编辑的分段，已停止覆盖操作」。
- 该函数永远从 `chapters.raw_text` 重新推导分段（`split_segments` + `infer_segment`），**不感知你已纠正过的内容**。

后果：想用「重新画本」修掉别处的错误，会连你手工改对的地方一起冲掉；所以用户实际走投无路，只能接受错误。

### 根因 3：UI 没有任何纠错入口
- `src/components/ScriptSurface.tsx:382-458` `SegmentTable` 每行只提供：类型下拉、角色下拉、文本 textarea、情绪输入、播放/上传/重生成/删除。
- 没有「从此处拆开」「并入上句」「插入一句」的按钮，也没有「选中文本→设为某角色台词」这类竞品交互。

### 附带已知隐患（说明 AI 拆分本身也不稳）
- `src-tauri/src/importer.rs:347` `split_segments`：按行 + 120 字按句号/叹号/问号/分号断，长句切得生硬。
- `src-tauri/src/importer.rs:369` `infer_segment`：代码注释自承 bug——`斗之气：七段！` 会被误拆成说话人「斗之气」+ 台词「七段！」（以冒号切分）。这类错误恰恰最需要「事后可纠正」来兜底。

---

## 二、市面主流画本工具怎么做的（参考对象）

### 画本妖鸡（huabenyaoji.com，有声的紫襟团队 / 苏州智言慧语）
定位是「电子提词器 + 剧本管理 + 剧组协作」。其纠错心智模型：
- 脚本是一份**自由可编辑的文档**，AI「智能分轨」只是自动给角色对话上色/标名。
- 人工纠正是常态操作：拖拽章节名到正文**拆分章节**、把台词**分配给角色**、**角色合并**、**全局替换**、**插入出场**、**字数分集**。
- 关键：分段是「在文档上施加的属性」，不是一次性生成的不可变表。你随时能改。

### 爱画本（ivoice.cloud，喜马拉雅系）
更贴近我们要改的环节，编辑器交互：
- **选中正文任意文本 → 右键「设为对话」/「取消对话」**：分段是「文本范围 → 类型」的映射，且**完全可逆**（对话可退回旁白）。
- **拖角色卡到某句对话上绑定角色**；锚点里「切换角色」。
- 每句可挂**情绪 / 音效**；有**书签**定位。
- 核心是：selection-based（基于选区）的分段，而非固定行。

### 提炼出的三条通用规律
1. **分段 = 文本范围的软属性**，不是生成后定死的行。
2. **完整 CRUD**：可拆、可合、可转换类型、可重指派角色、可插入、可删。
3. **重跑 AI 不毁手工**：AI 重标只填补空白/推荐，人工已定的部分被锁定。

---

## 三、改造方案（分三档，按性价比推进）

### Tier 1 — 直接消灭致命缺陷（必做，本方案核心）
新增三条后端命令 + 对应 UI 入口，让「分段边界错误」可在结构上修正，且修正结果带 `is_manual_edit = 1` 永久存活、不被重拆冲掉。

后端（`src-tauri/src/lib.rs` 新增，复用 `storage.rs`）：
- `split_segment(segment_id, offset)`：在字符偏移处把一行拆成两行。
  - 两段继承原 `segment_type / speaker / character_id / voice_profile_id`；
  - 两段 `order_index` 重新编号；原音频失效（两段 `audio_status = 'missing'`，调用 `audio::invalidate_segment_audio`）；
  - 两段 `is_manual_edit = 1`。
  - 拆点优先吸附到最近句末标点；允许任意偏移兜底。
- `merge_segments(segment_ids)`：按序合并相邻行。
  - 文本用空行/句号衔接；类型/说话人/角色取首段（或首个非 narration）；
  - 重新编号、`is_manual_edit = 1`、音频失效。
- `insert_segment(chapter_id, after_segment_id, text, type, character?)`：在两行间插入漏掉的台词，重编号。

前端（`src/components/ScriptSurface.tsx` + `useStudioActions.ts`）：
- 每行 hover 出现「拆分 / 合并下一句 / 插入一句」按钮。
- textarea 内 `Ctrl/Cmd+Enter` = 从光标处拆分；`Ctrl/Cmd+Backspace` 在行首 = 与上一句合并（可选）。
- 保留现有类型/角色/情绪编辑。

> 这一档做完，「AI 拆错」就不再是致命缺陷，而是「随手一拆一合就能修」。

### Tier 2 — 重拆时保留人工修正（强烈建议）
改 `seed_segments_from_chapter` 的 all-or-nothing 为「协调模式」：
- 新增 `resegment_chapter(chapter_id, preserve_manual: bool)`。
- `preserve_manual = true`：跳过 `is_manual_edit = 1` 的行（锁定），仅对非人工区段重新推导；提供「整章重画（丢弃人工修正）」作为显式二次确认的危险选项。
- 这样「重新画本」变成「AI 只补空白、不毁手工」，对齐画本妖鸡/爱画本的第三条规律。

### Tier 3 — 选区式画本编辑器（路线图，对标爱画本）
把固定行表格升级为「可选中正文的编辑器」：选中文本 → 设为对话/旁白/音效、指派角色、挂情绪音效、取消对话。工作量最大，但体验最接近竞品。可作为后续大版本。

---

## 四、验收标准
- 任意一行可一键拆成两句、可一键与邻行合并、可在任意位置插入一句；操作后顺序、音频失效、导出链路正常。
- 手工拆/合/插产生的行，再次「重新画本」时不被冲掉（Tier 2）。
- 现有「改文本/类型/角色/情绪/删除/重生成/上传」全部不受影响。

## 五、建议落地顺序
1. Tier 1 后端三命令（storage 层加 `split/merge/insert` 助手，lib 注册 invoke）。
2. Tier 1 前端按钮 + 快捷键。
3. Tier 2 协调重拆。
4. Tier 3 选区编辑器（独立排期）。

## 六、实现进度
- **Tier 1 已实现（2026-09-25）**：
  - 后端 `storage.rs`：`split_segment_at` / `merge_segments` / `insert_segment_after` / `get_segment` / `renumber_chapter_segments`；`lib.rs` 注册 `split_segment` / `merge_segments` / `insert_segment` 三条命令。
  - 前端 `ScriptSurface.tsx`：每行新增「拆分 / 合并下一句 / 插入」工具条；textarea 内 `Ctrl/⌘+Enter` 从光标处拆开；「插入」展开内联编辑器（选类型 + 输入文本）。`useStudioActions.ts` 对应 `splitSegment / mergeSegmentsAction / insertSegmentAfter`。
  - 校验：`cargo check` + `tsc --noEmit` 通过；`cargo test` 44 passed。
- **Tier 2 / Tier 3 待办**：重拆保留人工修正、选区式编辑器（路线图）。
- 附带修复：仓库测试目标原先因 `lib.rs:913/939` 误用未导入的 `Connection` 而无法编译（`cargo test` 直接失败），已改为 `rusqlite::Connection` 与全文一致，测试恢复可运行。
