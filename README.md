# Xiic Voice Studio

Xiic Voice Studio 是面向中文有声书和多角色音频剧的 macOS 本地制作工作台。项目文件夹是脚本、角色、声音配置、审听记录、音频资产和导出清单的持久来源。

## 当前能力

- 创建和打开本地项目，导入 TXT / DOCX
- 按章节拆分脚本，支持启发式标注和 OpenAI-compatible 结构化 AI 标注
- 编辑分段、角色抽取、别名合并和角色声音分配
- Mimo `MiMo-V2.5-TTS-VoiceDesign` / `MiMo-V2.5-TTS` 生成、Mock 演示和后台任务队列
- 单段试听、人工音频上传、重新生成、版本化审听和审听备注
- 发布检查、配音脚本、角色脚本、分段音频、整集 WAV / MP3 / M4B 和制作包导出
- API Key 保存到 macOS Keychain，不写入项目文件夹

## 运行

需要 macOS、Node.js、pnpm、Rust 和系统 FFmpeg。

```bash
pnpm install
pnpm tauri dev
```

发布构建：

```bash
pnpm tauri build
```

未配置 Apple Developer 签名时，生成的 macOS 包是本机可运行但未公证的 ad hoc 包；正式分发还需要配置 Developer ID 和 notarization。

## Mimo 配置

在左侧“服务配置”中选择 `Mimo`，填写：

- TTS Base URL：`https://api.xiaomimimo.com/v1`
- TTS 模型：`mimo-v2.5-tts-voicedesign` 或 `mimo-v2.5-tts`
- API Key：保存到系统 Keychain

先创建或打开一个项目，再点击“测试 TTS”验证音色和声音设计提示。生成任务支持取消、失败重试和单段重新生成。

## 音频与发布

项目内音频保存在：

```text
<项目文件夹>/assets/audio/<project-id>/
```

项目封面放在 `<项目文件夹>/assets/source/cover.jpg`、`cover.jpeg` 或 `cover.png`。M4B 导出会将封面、项目标题、作者和章节标记写入成品。

整集导出和制作包导出会阻断以下情况：缺失音频、音频文件不存在、分段未审听通过、分段返修或存在未解决审听问题。脚本或声音配置改变后，相关旧版本会标记为 stale，必须重新生成或重新上传并审听。

## 验证

```bash
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
```

产品和数据模型说明见 [docs/product-brief.md](docs/product-brief.md)。
