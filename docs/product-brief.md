# Xiic Voice Studio Product Brief

## 1. Positioning

Xiic Voice Studio is a local-first AI-assisted production tool for Chinese scripted audio.

The core object is not a picture book or a simple audiobook file. It is a production-ready voice script: imported text is converted into editable segments with speaker, role, emotion, voice, audio, review status, and export metadata.

The product should cover:

- audiobooks
- audio dramas
- multi-character TTS productions
- `huaben`/voice script preparation
- AI pre-review and human final review

The working code project name is `xiic-voice-studio`.

The product is a desktop application built with Tauri + Rust. It should treat the user's local project folder as the durable source of truth, with cloud TTS and AI services acting as optional providers around that local workflow.

## 2. Target Users

Primary users:

- audiobook producers
- audio drama producers
- script editors
- voice directors
- TTS operators
- post-production reviewers

The product should feel like a production workbench, not a consumer reading app or a browser-only SaaS dashboard.

## 3. MVP Workflow

1. Create a local project workspace and import source text, such as TXT or DOCX.
2. Clean the text and split it into chapters or episodes.
3. Use an LLM to mark the script:
   - narration
   - character dialogue
   - inner monologue
   - overlapping or multi-speaker lines
   - sound effects
   - scene transitions
   - timing anchors
4. Extract characters and merge aliases.
5. Assign each character a human CV, TTS voice, or voice profile.
6. Generate TTS by chapter, scene, role, or segment batch.
7. Review generated audio with AI pre-review plus human review.
8. Repair individual segments or regenerate larger ranges.
9. Export:
   - voice script
   - character script
   - segmented audio
   - full episode audio
   - production package

The MVP should keep normal editing, review, asset browsing, and export usable on local files even when network-only AI or TTS providers are unavailable.

## 4. Core Data Model

Suggested domain objects:

- `BookProject`: title, author, source, language, production type, status.
- `Chapter`: project ID, title, order, raw text, script status.
- `Scene`: chapter ID, order, description, time/location, mood.
- `Segment`: text, segment type, speaker, character, emotion, sound cue, anchor, voice profile, audio status.
- `Character`: canonical name, aliases, gender, age timeline, notes, default color.
- `VoiceProfile`: character ID, age stage, TTS provider, voice ID, speed, pitch, timbre/style settings.
- `VoiceBatch`: grouped TTS generation task with provider, parameters, context window, result metadata.
- `SegmentAudio`: segment ID, generated or uploaded file, duration, loudness, version, source.
- `ReviewIssue`: chapter/segment/audio reference, issue type, note, status.
- `ExportJob`: target format, selected chapters, audio mix settings, status.

Local project storage should preserve these objects in a portable workspace format. A likely shape is:

- a project manifest for metadata and app version compatibility
- a local database for structured script, character, voice, review, and job records
- an `assets/` directory for generated and uploaded audio
- an `exports/` directory for rendered deliverables
- a provider cache for voice catalogs and request metadata

## 5. TTS Generation Strategy

Segment-level audio should be stored independently, because it enables targeted repair and re-export.

However, generation should not treat each sentence as an isolated TTS request. That can cause unstable tone, volume, rhythm, and character consistency.

Preferred strategy:

- keep audio assets at segment level
- generate audio in larger batches by chapter, scene, or nearby role context
- pass previous and next text as context when provider support exists
- preserve provider parameters and generation seed/config per batch when available
- normalize generated audio after synthesis
- re-mix full chapters from the latest segment audio versions

For audio consistency:

- normalize segment loudness to a target such as `-18 LUFS`
- limit peaks
- trim excessive silence
- add short fades only where useful
- run chapter-level loudness normalization after final mix

Because this is a desktop tool, long-running generation and repair work should run through a local job queue. The UI should be able to pause, resume, retry, and inspect jobs without hiding provider errors behind a generic failure state.

## 6. Character Voice Growth

The product should support characters whose voices change as the story progresses, such as a child growing into an adult.

Do not model a character as one fixed voice forever. Use a voice timeline:

- childhood
- teenager
- young adult
- adult
- middle-aged
- elderly

Each stage can bind to a different TTS voice, cloned voice, or voice design preset.

For gradual change:

- use a `VoiceTimeline` with keyframes
- each segment stores or derives an `age_progress`
- prefer chapter/scene-level changes, not sentence-by-sentence changes
- use smooth progression between keyframes
- when true voice interpolation is unavailable, use several intermediate voice profiles
- avoid simple pitch shifting as the main solution

Single-segment repair must inherit the original voice stage and progress value, otherwise the repaired line may sound detached from nearby lines.

## 7. AI Review

AI review should be framed as pre-review, not a replacement for human final review.

Recommended review layers:

- ASR transcription plus text comparison for missing words, wrong words, extra words, and wrong order.
- Speaker or role consistency checks for wrong character voice.
- Audio quality scoring for noise, clipping, excessive silence, distortion, and loudness problems.
- Multimodal/audio model comments for subjective performance issues, such as flat emotion or unnatural pauses.

Useful model/tool categories:

- ASR: OpenAI `gpt-4o-transcribe`, `gpt-4o-mini-transcribe`, `gpt-4o-transcribe-diarize`, Whisper-style models.
- Audio quality: NISQA, DNSMOS, Distill-MOS.
- Speaker consistency: pyannote.audio, speaker embeddings, ECAPA-TDNN style models.
- Subjective review: audio-capable multimodal LLMs.

The review UI should locate issues to exact chapters, scenes, segments, and timestamps.

## 8. TTS Provider Notes

The initial Chinese TTS provider shortlist:

### Xiaomi MiMo

MiMo-V2.5-TTS, VoiceDesign, and VoiceClone appear attractive for early testing because the official MiMo site states they are free for a limited time. Treat this as trial-stage pricing, not a permanent cost assumption.

Good fit:

- Chinese TTS exploration
- voice design experiments
- voice clone experiments
- early product demos

Risk:

- limited-time free access may change
- production SLA, pricing, and long-term API stability need validation

### Alibaba Cloud Model Studio / Bailian

Strong candidate for the main provider because CosyVoice/Qwen-TTS style offerings are positioned around expressive Chinese speech, voice cloning, voice design, and scenarios such as audiobooks or audio drama.

Good fit:

- Chinese scripted audio
- character voices
- voice design
- voice cloning
- long-form production

### Tencent Cloud TTS

Strong engineering fallback. Useful for stable production integration, SSML-style control, long text, multiple Chinese voices, and cloud-native operational maturity.

Good fit:

- batch generation
- long text
- stable API integration
- provider fallback

### iFlytek

Strong Chinese voice ecosystem with many speakers, long-text synthesis, and voice cloning options.

Good fit:

- Chinese audiobook workflows
- Mandarin and dialect experiments
- voice library comparison

### Baidu Speech

Potentially useful for cost-sensitive Chinese TTS, emotional synthesis, and voice cloning experiments.

Good fit:

- cost comparison
- Chinese style testing
- fallback provider

### OpenAI TTS

Useful as an international/multilingual option, but not the first choice for a Chinese-first production engine because official guidance has historically emphasized English-optimized voices.

## 9. Desktop Architecture

Use Tauri + Rust as the application foundation.

Recommended boundaries:

- Tauri frontend: script editor, timeline/review UI, character and voice assignment, batch controls, import/export screens, and settings.
- Rust domain core: project model, parsing pipeline, script segmentation, character extraction state, review state, and export planning.
- Rust storage layer: local project manifest, embedded database, asset paths, migrations, and compatibility checks.
- Rust audio layer: segment asset management, waveform metadata, loudness normalization, silence trimming, fades, and chapter mixing.
- Rust job system: import, AI marking, TTS generation, review, repair, and export tasks.
- Rust provider layer: TTS, ASR, LLM, audio review, and voice catalog adapters.

Tauri commands should expose focused application operations instead of leaking database tables directly to the UI. For example:

- `create_project`
- `import_source`
- `mark_chapter`
- `update_segment`
- `assign_voice_profile`
- `enqueue_tts_batch`
- `review_segment_audio`
- `export_episode`

The app should keep secrets such as provider API keys in the OS keychain when possible, not inside the portable project folder.

## 10. Provider Architecture

The system should not hard-code one TTS vendor.

Build a provider adapter layer:

- common voice synthesis request type
- provider-specific parameter mapping
- provider-specific voice catalog sync
- consistent audio artifact storage
- retry and error classification
- cost and duration tracking
- provider fallback rules

Minimum provider interface:

```rust
trait TtsProvider {
    fn list_voices(&self) -> Vec<VoiceInfo>;
    fn synthesize(&self, request: TtsRequest) -> TtsResult;
    fn clone_voice(&self, request: VoiceCloneRequest) -> VoiceCloneResult;
}
```

The exact Rust interface can change during implementation, but the architecture should keep vendor lock-in low from day one.

## 11. Product Principles

- AI output must be editable.
- Preserve human control over script marking, character merging, voice assignment, and final review.
- Store segment audio independently, but generate with enough context for consistency.
- Support both audiobooks and audio dramas.
- Design for Chinese first.
- Prefer local project ownership over cloud lock-in.
- Keep editing, review, asset management, and export useful without a network connection.
- Treat TTS provider pricing and availability as unstable.
- Make regeneration predictable: single line, role range, scene, chapter, or whole project.
- Never overwrite manually uploaded human audio unless the user explicitly chooses it.
