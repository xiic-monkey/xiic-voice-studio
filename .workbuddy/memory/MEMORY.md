# 项目长期备注 · xiic-voice-studio

## 一句话定位
macOS 本地优先的中文有声书 / 多角色音频剧制作工作台，Tauri 2 + React 19 + Rust，本地项目文件夹是唯一真源，云 TTS/AI 只是可插拔的外部供应商。

## 技术栈
- 前端：React 19 + Vite 7 + TypeScript，单文件 UI（src/App.tsx + App.css），lucide-react 图标。
- 桌面壳：Tauri 2，43 个 invoke 命令，CSP 白名单只放行 xiaomimimo / openai / dashscope。
- 后端：Rust 2021，rusqlite（bundled SQLite）、reqwest、docx-rs、zip、keyring、tokio。
- 音频：依赖系统 FFmpeg，做响度归一、静音裁剪、淡入淡出、整集混音与 M4B 导出。

## 目录与职责
- `src/` 前端全部代码（App.tsx 承担全部 UI 与状态）。
- `src-tauri/src/` domain / storage / importer / ai / tts / audio / settings / error / tests。
- `docs/product-brief.md` 产品与数据模型设计的权威来源。
- `docs/release.md` macOS 签名与公证的 Actions secrets 清单。

## 核心模型：角色 = 音色的唯一归属（2026-09-25 确立，权威依据 product-brief 第 4/6 节）
- `characters` 1:N `voice_profiles`（按 `age_stage`，童年→老年）。**不存在"独立可浏览的声音库"实体**；
  `VoiceCenter` 只是按说话人分组的投影视图。
- 合成时按 `segments.character_id` + `age_progress` **实时解析**音色；`segments.voice_profile_id`
  只保留一个用途——**定稿（审听通过/人工上传）时冻结"用的哪条"**，让单段返修继承生成时的音色阶段。
- 解析优先级：定稿走冻结档 → 否则按角色+阶段 → 旁白走项目旁白档 → 都没有则**报错**。
  **禁止再用 `COALESCE(v.tts_provider,'mock')` 之类的静默兜底**（那是"改音色就冒出 mock"的根因）。
- `voice_profiles.is_default = 1` 表示系统兜底的默认档（未由人确认）；人一旦设定即置 0。
  所有排序都要让非默认档优先，否则自动档会顶掉用户选择。
- 改音色 = 就地更新该角色对应阶段的档案 + 未定稿分段标 stale；**不得清空分段引用、不得把旧档案 `character_id` 解绑**。
- 新角色在标注阶段（`ai::extract_characters`）就会拿到一条默认音色，保证闸门在正常流程不会被触发。

## 音色描述自动填充的取证规则（2026-09-25 确立，`src-tauri/src/voice_context.rs`）
- **台词不是自述证据**：角色自己说的话里提到的是别人（「薰儿小姐」「对着少女略微恭声道」）。
  语料必须拆成 `lines`（台词）/ `self_narration`（与自己台词**同段**的叙述＝说话人标签）/
  `about_narration`（其它提到他的叙述）。身份（性别/年龄）只采信**名字/别名 + 叙述**；
  作风（音色质感/语气）只采信**名字/别名 + self_narration + 台词**，不取 `about_narration`。
- 性别/年龄逐层降级：**人标注字段 → 名字/别名自述 → 叙述证据（须无冲突）**。
  年龄最后一层才用 `age_timeline` 的占位值兜底，且标 `AgeSource::PlaceholderFallback`。
- **`unanimous_cue`：全部命中归并同一标签才下结论，有冲突就不写。**
  中文小说同段常同时写多人，词频分不清指谁；**性别猜错 = 音色选错**，宁可交白卷并写明"未能确定"。
- **`ai::insert_character` 写死的 `age_timeline='adult'` 是占位值不是人的判断**（gender 写 NULL），
  不能优先于文本证据。DB 的 `male`/`female`/`middle_aged` 必须归一成中文再进任何用户可见文案或提示词
  （`male`/`female` 是包含关系，必须整串比较）。
- 音色描述进 LLM 前必须校验照抄（>200 字 / 含引号 / 与台词重叠），不合格先严格重试，
  仍不合格**回退本地草稿并给 warning**，绝不把章节原文塞回输入框。

## 项目约定
- 分段级存储音频，批量（章节/场景）生成，保证音色一致性。
- 审听未通过、返修中、缺失音频会阻断整集与制作包导出。
- 脚本或声音配置变更后旧音频版本标记 stale，必须重新生成或重新上传。
- API Key 只进 macOS Keychain，不写入项目文件夹。
- 角色声音支持年龄时间轴（童年→少年→青年→成年→中年→老年）绑定不同音色。
- **弹窗里的失败必须可见**：底部提示条被 `.modal-backdrop`（z-index 50）盖住，等于"点了没反应"。
  弹窗内一律用内联 `.voice-flow-status`，全局配 `.app-toast`（z-index 60）；耗资源的操作先做 Key 预检。
- **验证二进制里是否含某个中文串不能用 `strings`**（只提取 ASCII），要按字节搜。
- **Mimo 语音合成是非确定性的**（2026-09-25 实测）：同一请求连跑 3 次，
  voicedesign 时长 9.28/11.52/10.24s（±12%）、md5 3/3 不同；voiceclone ±4%。
  ⇒ A/B 对比必须先测基线抖动，md5 只能用来证明「相同」，**结论依据要回到请求体本身**。
- 试听台词的默认值规则：优先取「引号开头的纯台词」池中预算内最长的一句（≤60 字），
  池内最长仍 <20 字才改用池内最长；`dialogue` 分段里可能混着旁白叙述。
- **`play()` 不会被自动播放策略拦**：wry 的 `WebViewAttributes.autoplay` 默认 `true`
  （`wry-0.55.1/src/lib.rs`），Tauri 不覆盖它 ⇒ macOS 上会执行
  `setMediaTypesRequiringUserActionForPlayback(None)`。所以「await IPC 之后再调 `play()`」可行；
  哪天改成 false，异步起播就会被手势策略拒掉。
- **"点即播放"用 `playSignal`（递增=请求播放），不要靠 `src` 变化触发播放**：
  `src` 变化只负责重置播放器。凡是"设置状态"与"执行动作"该分开的交互，都按这个模式写。
- **播放器显示的是"正在播的分段"，不是"选中的分段"**（`playerSegment = playingSegment ?? selectedSegment`）。
  试听角色固化样本时另起 `playingSample`（角色名）覆写标题，否则"样本在响、标题写着上一个分段"。
- **说明文字不常驻卡片**（2026-09-25 用户明确要求）：一行小字会把卡片撑高，且没人读。
  需要解释就用 `HintTip`（`ui.tsx`，`HelpCircle` 图标 + hover/focus 气泡）。
  气泡**必须由卡片约束宽度**（定位基准 = `position: relative` 的 `.character-card`），
  因为 `.inspector` 是 `overflow-y: auto`，横向也会被裁 —— 固定宽度气泡在 280px 紧凑右栏必撑出横向滚动条。
- **卡片右侧只放 icon 按钮**（`.icon-button.compact`，`align-self: start` 顶到右上角）：
  「改名/别名/性别/年龄」与「重新设计音色」对用户是同一件"编辑"，合并进 `character-editor` 一个入口。
- **试听"固化的样本音色"要读本地文件，不要走 `test_voice_profile`**：后者会重新请求云端合成，
  想听的是"固化下来的到底是哪个声音"，本地那份参考音频才是唯一答案。用
  `voice_asset_audio_path`（内部 `storage::voice_asset_audio_path`，**带 `is_file()` 校验**，
  库里记录 ≠ 文件还在，缺失必须回 `None` 让前端给可行动提示）。
- 别名不占独立行，也不渲染「无别名」占位；只在与名字同行且**确实有别名**时出现。
- **输入控件必须常驻标签，`placeholder` 不算标签**（一填内容就消失）：
  分段行用 `.segment-field`（label + 控件 flex 行，label `padding-top: 5px` 对齐首行文字）。
  标签化后 placeholder 里的重复前缀要删（「情绪：…」→「如 …」）。
  ⚠️ 控件从 grid 子项变 flex 子项后，原 `grid-area` / `width: auto` 全部失效，必须补 `flex: 1 1 auto; min-width: 0`。
- **分段行是两行 grid，且每行是独立 grid 容器** ⇒ 列宽必须可预测，否则表头与数据错位：
  用固定 px 或 `minmax(min, max)`，**禁用 `max-content`**（各行内容不同 → 列宽逐行漂移）。
  对齐锚是 index / type / char / status 这几列；spacer（`minmax(0,1fr)`）与 actions 允许逐行不同。
  第一行现在 = `32 / minmax(88,100) / minmax(112,150) / spacer / minmax(96,max-content) / auto`，
  右侧七枚 24px 图标按钮（token `--control-h-xs`）：结构编辑（拆分/合并/插入）+ 竖线 `.segment-actions-divider` + 音频操作（播放/上传/重生成/删除）。
  按钮移出原容器后**每枚都要自己补 `stopPropagation`**（原容器上的那个会随之消失）。

## 新增一个 invoke 命令的落点（漏一个就出 bug）
① 后端 command fn + 请求结构体（`#[serde(rename_all = "camelCase")]`，**可选字段必须 `#[serde(default)]`**，
   否则前端不传就会反序列化失败）→ ② 注册进 `invoke_handler` → ③ 前端 hook 里
   `invoke<T>(命令名, { request })`，成功后用返回的快照 `hydrate` → ④ 组件接线
   → ⑤ **业务逻辑尽量落在 `storage.rs` 等可测层，command 只做编排与收尾动作**（这样能写回归测试；
   项目里没有直接测 tauri command 的先例）。

## 前端 UI 的视觉验证（改完别只验编译产物）
`curl http://localhost:1420/src/...` 只能证明"代码编译过了"，证明不了"界面是对的"。要看渲染：
```bash
agent-browser open "http://localhost:1420/?mock=1"     # dev 预览 mock，无需 Tauri 运行时
agent-browser screenshot ".character-panel" /tmp/p.png # 按选择器截局部
agent-browser eval "document.querySelector(...).getBoundingClientRect().width"  # 量真实尺寸
agent-browser close
```
- ⚠️ **mock 视口 1080 与真实窗口 1280 都命中 `@media (max-width: 1280px)` 紧凑断点**，
  右侧栏同为 280px —— 所以 mock 里看到的布局就是真实的（用 `osascript` 拿窗口尺寸可确认）。
- 已知坑：flex 子项空间不足时被压到 min-content（汉字变一列一个）。
  **`flex: 1` 是 `basis: 0%`，永不触发 `flex-wrap` 换行**；要换行必须 `flex: 1 1 auto`。
- ❌ **`screencapture -R` 不可靠**：窗口不在当前 Space 时会截到**别的窗口**（可能含无关隐私），
  而且 `osascript` 常拿不到 Tauri 窗口的位置（`count of windows` 返回 0）。**别再用它验证应用。**
- ✅ **正解：按窗口 ID 截图**（2026-09-25 实测通过，窗口离屏也能截，且不会带到别的窗口）：
  ```bash
  # 1) 用系统 API 枚举窗口，拿 id（比 System Events 可靠得多）
  python3 -c "
  import Quartz
  for w in Quartz.CGWindowListCopyWindowInfo(Quartz.kCGWindowListOptionAll, Quartz.kCGNullWindowID):
      if 'xiic' in (w.get('kCGWindowOwnerName') or '').lower():
          print(w.get('kCGWindowNumber'), w.get('kCGWindowOwnerPID'), dict(w.get('kCGWindowBounds', {})))
  "
  # 2) 按 id 截图（-o 去掉窗口阴影），产物是 @2x 的 2880×1840 而不是 1440×920
  screencapture -x -o -l <windowId> /tmp/win.png
  ```
- ⚠️ **同样的 owner 名可能有多个实例**（debug 二进制 + `/Applications/*.app`），
  必须用 `kCGWindowOwnerPID` 对上 `ps` 里的 pid 才能确定截的是哪一个。
- ⚠️ **`kCGWindowListOptionOnScreenOnly` 只列当前 Space 的窗口**；后台拉起的实例窗口会漏掉，
  看起来像"没有窗口"。诊断"窗口去哪了"要用 `kCGWindowListOptionAll`。
- ⚠️ **别靠肉眼看截图量尺寸**：本次目测主区左右边界得 218/866，实测 248/999（差 30%），
  差点按错的宽度定列宽预算。要量就用 `eval getBoundingClientRect()`，
  或用 PIL 扫 `--accent` 色竖条定位 active 行的上下边界。
- ⚠️ **截图里"看着没边框"往往是错觉**：24px 小按钮边框 `#dfe4eb` 太浅，缩放截图里会和图标糊在一起。
  判定样式差异要用 computed style 逐项断言（`borderTopWidth / borderTopColor / backgroundColor`）。
- 分段表第一行实测宽 720px（窗口 1280，`rowW=752` 减内边距 32），mock 与真实应用一致。

## 构建与验证
```bash
pnpm install && pnpm tauri dev
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test  --manifest-path src-tauri/Cargo.toml
```
无 Apple Developer 签名时产物为 ad hoc 未公证包，仅供本机运行。
- Git 提交身份（2026-09-15 配置，**仅本仓库 local**）：`xiic-monkey <24518439+xiic-monkey@users.noreply.github.com>`（GitHub noreply 邮箱）。此前仓库级与全局均未配置身份，直接 `git commit` 会失败。

## 本地预览启动（实测坑）
- 直接 `pnpm tauri dev` 在后台任务里跑，进程组会被回收（约 1 分钟后整体退出，窗口起不来）。
- 稳定做法：分开常驻——先 `pnpm dev`（Vite，前端在 http://localhost:1420，HMR 实时），再直接拉起已编译二进制 `./src-tauri/target/debug/xiic-voice-studio`（窗口落在用户 GUI 会话）。
- 前端改动走 Vite HMR 自动热更；Rust 侧改动需重新 `cargo build` 并重启该二进制。
- macOS 启动时会打一行 `error messaging the mach port for IMKCFRunLoopWakeUpReliable`，是输入法相关无害告警，不致命。

## 发布包覆盖本地安装（xiic-voice-studio）
- ⚠️ **当前状态（2026-09-25 实测）：`/Applications/xiic-voice-studio.app` 存在，且用户会打开它。**
  此前"不存在"的结论已过时。它的二进制构建于 **2026-09-12 22:45**，早于最近所有改动——
  **用户在启动台打开的可能是两周前的旧版本，看不到新改动**。
  排查"我改好了但用户说没变"时，**第一件事是确认用户看的是哪个实例**：
  ```bash
  ps aux | grep -i voice-studio | grep -v grep     # 看有没有 /Applications/... 的进程
  stat -f "%Sm" /Applications/xiic-voice-studio.app  # 看安装版有多旧
  ```
  日常迭代仍走上面的「Vite 常驻 + debug 二进制」分离方案；**改动要真正交付给用户时必须重新打包安装版**。
- ⚠️ 旧版二进制**不认识 `deleted_segments` 表**（迁移是 `CREATE TABLE IF NOT EXISTS`，不会冲突），
  但旧版的 `delete_segment` 是**硬删**——用旧版删除仍然不可逆。
- 构建：`pnpm tauri build`（会自动跑 `pnpm build` 产 `dist`，再嵌入 Rust release 二进制）。
- 产物：`src-tauri/target/release/bundle/macos/xiic-voice-studio.app`。
- 覆盖安装到启动台：
  ```bash
  rm -rf "/Applications/xiic-voice-studio.app"
  ditto "src-tauri/target/release/bundle/macos/xiic-voice-studio.app" "/Applications/xiic-voice-studio.app"
  xattr -dr com.apple.quarantine "/Applications/xiic-voice-studio.app"
  open "/Applications/xiic-voice-studio.app"
  ```
- 注意：Launch Services 可能缓存旧的 debug bundle（`src-tauri/target/debug/bundle/macos/xiic-voice-studio.app`），若 `open -a xiic-voice-studio` 拉起的是旧路径，应显式用 `/Applications/xiic-voice-studio.app` 的绝对路径打开。
