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
