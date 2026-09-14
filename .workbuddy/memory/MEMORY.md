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

## 项目约定
- 分段级存储音频，批量（章节/场景）生成，保证音色一致性。
- 审听未通过、返修中、缺失音频会阻断整集与制作包导出。
- 脚本或声音配置变更后旧音频版本标记 stale，必须重新生成或重新上传。
- API Key 只进 macOS Keychain，不写入项目文件夹。
- 角色声音支持年龄时间轴（童年→少年→青年→成年→中年→老年）绑定不同音色。

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
- ⚠️ 当前状态（2026-09-12 实测）：`/Applications/xiic-voice-studio.app` **不存在**，此前记录的安装已失效/被清理；`open` 它会报文件不存在。日常预览请走上面的「Vite 常驻 + 直接拉 debug 二进制」分离方案。需要安装版时再跑下面流程重新生成。
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
