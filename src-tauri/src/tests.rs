#[cfg(test)]
mod tests {
    use crate::{ai, audio, importer, settings, storage, tts};
    use rusqlite::params;
    use serde_json::Value;
    use std::fs;
    use std::io::Read;
    use uuid::Uuid;

    #[test]
    fn persists_non_secret_application_settings() {
        let root = std::env::temp_dir().join(format!("xiic-settings-test-{}", Uuid::new_v4()));
        let mut value = settings::AppSettings::default();
        value.llm.model = "custom-model".to_string();
        value.audio.ffmpeg_path = "/opt/homebrew/bin/ffmpeg".to_string();

        settings::save(&root, &value).unwrap();
        let loaded = settings::load(&root).unwrap();

        assert_eq!(loaded, value);
        let raw = fs::read_to_string(settings::settings_path(&root)).unwrap();
        assert!(!raw.to_ascii_lowercase().contains("api_key"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creates_project_manifest_and_database() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "测试项目", Some("作者".to_string())).unwrap();

        assert_eq!(summary.manifest.title, "测试项目");
        assert!(root.join("project.json").exists());
        assert!(root.join("studio.sqlite").exists());
        assert!(root.join("assets/audio").exists());
        let conn = storage::open_connection(&root).unwrap();
        let migration_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(migration_count, 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_assets_can_be_validated_and_backed_up() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "资产备份测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_id = importer::import_source(&conn, &summary.manifest.id, &source)
            .unwrap()
            .remove(0);
        importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();
        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();
        approve_all_segment_audio(&conn);

        let assets = audio::validate_project_assets(&conn, &root).unwrap();
        assert!(assets.valid);
        assert!(assets.checked_assets > 0);
        let relative_path: String = conn
            .query_row(
                "SELECT relative_path FROM segment_audio LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        fs::remove_file(root.join(&relative_path)).unwrap();
        let missing = audio::validate_project_assets(&conn, &root).unwrap();
        assert!(!missing.valid);
        assert_eq!(missing.missing_assets, 1);

        let cover_source = root.with_file_name(format!("cover-{}.jpg", Uuid::new_v4()));
        fs::write(&cover_source, b"fake jpeg").unwrap();
        let cover_path = audio::copy_project_cover(&root, &cover_source).unwrap();
        assert!(cover_path.exists());
        let backup = audio::export_project_backup(&root).unwrap();
        let backup_file = fs::File::open(backup).unwrap();
        let mut archive = zip::ZipArchive::new(backup_file).unwrap();
        assert!(archive.by_name("project.json").is_ok());
        assert!(archive.by_name("studio.sqlite").is_ok());
        assert!(archive.by_name("assets/source/cover.jpg").is_ok());

        fs::remove_file(cover_source).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_an_existing_project_folder() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("project.json"), "已有项目").unwrap();

        let error = storage::create_project(&root, "不应覆盖", None).unwrap_err();
        assert!(error
            .to_string()
            .contains("已经包含 Xiic Voice Studio 项目"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_empty_project_names_and_empty_sources() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let empty_name = storage::create_project(&root, "  ", None).unwrap_err();
        assert!(empty_name.to_string().contains("项目名称不能为空"));

        let source_path = root.join("empty.txt");
        fs::write(&source_path, "\n  \n").unwrap();
        let empty_source = importer::read_source(&source_path).unwrap_err();
        assert!(empty_source.to_string().contains("稿件内容为空"));

        fs::remove_dir_all(root).unwrap_or(());
    }

    #[test]
    fn opening_project_recovers_incomplete_jobs_as_retryable_failures() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "任务恢复测试", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let running = storage::insert_job(&conn, &summary.manifest.id, "tts_batch", "{}").unwrap();
        let pending = storage::insert_job(&conn, &summary.manifest.id, "tts_batch", "{}").unwrap();
        storage::mark_job(&conn, &running.id, "running", 0.4, None).unwrap();

        let recovered = storage::recover_incomplete_jobs(&conn).unwrap();
        let jobs = storage::list_jobs(&conn).unwrap();

        assert_eq!(recovered, 2);
        assert!(jobs.iter().filter(|job| job.status == "failed").count() >= 2);
        assert!(jobs
            .iter()
            .filter(|job| job.id == running.id || job.id == pending.id)
            .all(|job| job.error.as_deref() == Some("应用上次退出时任务未完成，请重试")));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imports_txt_and_seeds_segments() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "导入测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(
            &source_path,
            "第一章 开场\n张三：你好。\n李四：来了。\n旁白继续。",
        )
        .unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();

        assert_eq!(chapter_ids.len(), 1);
        let seeded = importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        assert!(seeded >= 3);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_a_chapter_removes_its_segments() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "章节删除测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "第一章 开场\n旁白继续。\n第二章 继续\n新的内容。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();

        storage::delete_chapter(&conn, &chapter_ids[0]).unwrap();

        let remaining_chapters: i64 = conn.query_row("SELECT COUNT(*) FROM chapters", [], |row| row.get(0)).unwrap();
        let remaining_segments: i64 = conn.query_row("SELECT COUNT(*) FROM segments", [], |row| row.get(0)).unwrap();
        assert_eq!(remaining_chapters, 1);
        assert_eq!(remaining_segments, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn infers_dialogue_with_an_ascii_colon() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "Alice: Hello.").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let summary = storage::create_project(&root, "ASCII 台词测试", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_id = importer::import_source(&conn, &summary.manifest.id, &source)
            .unwrap()
            .remove(0);
        importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();
        let segment: (String, Option<String>) = conn
            .query_row(
                "SELECT segment_type, speaker FROM segments LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(segment.0, "dialogue");
        assert_eq!(segment.1.as_deref(), Some("Alice"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn llm_marking_payload_updates_annotations_and_preserves_manual_segments() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "AI 标注落库测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "旁白继续。\n张三：你好。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_id = importer::import_source(&conn, &summary.manifest.id, &source)
            .unwrap()
            .remove(0);
        importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();
        let first_id: String = conn
            .query_row(
                "SELECT id FROM segments WHERE chapter_id = ?1 ORDER BY order_index LIMIT 1",
                [&chapter_id],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "UPDATE segments SET is_manual_edit = 1, segment_type = 'transition' WHERE id = ?1",
            [&first_id],
        )
        .unwrap();

        let payload = serde_json::json!({
            "choices": [{"message": {"content": serde_json::json!({
                "segments": [
                    {"text": "旁白继续。", "segmentType": "sound_cue", "speaker": null},
                    {"text": "你好。", "segmentType": "dialogue", "speaker": "张三", "emotion": "平静"}
                ],
                "characters": [{"name": "张三", "aliases": ["三哥"]}]
            }).to_string()}}]
        })
        .to_string();
        let marking = ai::parse_marking_payload(&payload).unwrap();
        ai::apply_llm_marking(&conn, &chapter_id, &marking).unwrap();
        ai::extract_characters(&conn, &summary.manifest.id).unwrap();

        let first_type: String = conn
            .query_row(
                "SELECT segment_type FROM segments WHERE id = ?1",
                [&first_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(first_type, "transition");
        let second: (String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT segment_type, speaker, emotion FROM segments WHERE id != ?1",
                [&first_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(second.0, "dialogue");
        assert_eq!(second.1.as_deref(), Some("张三"));
        assert_eq!(second.2.as_deref(), Some("平静"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mock_tts_creates_segment_audio() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "TTS 测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();

        let segment_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM segment_audio", [], |row| row.get(0))
            .unwrap();
        let duration_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio WHERE duration_ms IS NOT NULL AND duration_ms > 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(segment_count > 0);
        assert_eq!(segment_count, duration_count);
        let relative_path: String = conn
            .query_row(
                "SELECT relative_path FROM segment_audio ORDER BY version LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(relative_path.starts_with("assets/audio/"));
        assert!(root.join(&relative_path).exists());
        assert!(storage::latest_audio_for_segment(
            &conn,
            &conn
                .query_row(
                    "SELECT segment_id FROM segment_audio ORDER BY version LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
        )
        .unwrap()
        .is_some());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mock_tts_test_audio_is_written_without_segment_audio_record() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "TTS 连通性测试", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();

        let output_path = tts::synthesize_test_audio(
            &root,
            &summary.manifest.id,
            Some(tts::ProviderSettings {
                provider: "mock".to_string(),
                api_key: None,
                endpoint: None,
                model: None,
            }),
            "mock-female-narrator".to_string(),
            Some("旁白".to_string()),
        )
        .unwrap();

        assert!(output_path.exists());
        assert!(output_path.starts_with(root.join("assets/audio")));
        let segment_audio_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM segment_audio", [], |row| row.get(0))
            .unwrap();
        assert_eq!(segment_audio_count, 0);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tts_job_persists_retryable_provider_settings() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "任务重试参数测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();

        tts::synthesize_segments(
            &conn,
            &root,
            &summary.manifest.id,
            Vec::new(),
            Some(tts::ProviderSettings {
                provider: "mock".to_string(),
                api_key: None,
                endpoint: Some("https://provider.example/v1".to_string()),
                model: Some("mock-model".to_string()),
            }),
        )
        .unwrap();

        let payload: serde_json::Value = conn
            .query_row(
                "SELECT payload_json FROM jobs WHERE job_type = 'tts_batch' ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .map(|value| serde_json::from_str(&value).unwrap())
            .unwrap();
        assert_eq!(
            payload
                .pointer("/provider")
                .and_then(|value| value.as_str()),
            Some("mock")
        );
        assert_eq!(
            payload
                .pointer("/endpoint")
                .and_then(|value| value.as_str()),
            Some("https://provider.example/v1")
        );
        assert_eq!(
            payload.pointer("/model").and_then(|value| value.as_str()),
            Some("mock-model")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tts_job_is_pending_before_background_execution_and_succeeds_afterward() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "任务入队测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_id = importer::import_source(&conn, &summary.manifest.id, &source)
            .unwrap()
            .remove(0);
        importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();
        let segment_id: String = conn
            .query_row("SELECT id FROM segments LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let options = tts::TtsSynthesisOptions::default();
        let job_id = tts::create_tts_job(
            &conn,
            &summary.manifest.id,
            std::slice::from_ref(&segment_id),
            None,
            &options,
        )
        .unwrap();
        let pending_status: String = conn
            .query_row("SELECT status FROM jobs WHERE id = ?1", [&job_id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(pending_status, "pending");

        tts::synthesize_segments_for_job(
            &conn,
            &root,
            &summary.manifest.id,
            &job_id,
            vec![segment_id],
            None,
            options,
        )
        .unwrap();
        let completed_status: String = conn
            .query_row("SELECT status FROM jobs WHERE id = ?1", [&job_id], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(completed_status, "succeeded");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn default_mimo_narrator_binds_to_unassigned_narration_segments() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "默认旁白测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();

        tts::ensure_default_voice_profiles(&conn, &summary.manifest.id).unwrap();

        let (provider, voice_id): (String, String) = conn
            .query_row(
                "SELECT v.tts_provider, v.voice_id
                 FROM segments s
                 JOIN voice_profiles v ON s.voice_profile_id = v.id
                 WHERE s.segment_type = 'narration'
                 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let dialogue_voice_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE segment_type = 'dialogue' AND voice_profile_id IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(provider, "mimo");
        assert_eq!(voice_id, "温柔、清澈、适合长篇有声书旁白的成年女声");
        assert_eq!(dialogue_voice_count, 0);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_voice_assignment_updates_matching_segment_scope() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "声音分配测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        ai::extract_characters(&conn, &summary.manifest.id).unwrap();
        tts::ensure_default_voice_profiles(&conn, &summary.manifest.id).unwrap();

        let character_id: String = conn
            .query_row(
                "SELECT id FROM characters WHERE canonical_name = '张三'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let narrator_profile_id = Uuid::new_v4().to_string();
        let character_profile_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO voice_profiles (id, project_id, character_id, name, age_stage, tts_provider, voice_id, speed, pitch, style)
             VALUES (?1, ?2, NULL, '新版旁白', 'adult', 'mimo', 'Mia', 1.0, 0.0, '新版旁白风格')",
            params![narrator_profile_id, summary.manifest.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO voice_profiles (id, project_id, character_id, name, age_stage, tts_provider, voice_id, speed, pitch, style)
             VALUES (?1, ?2, ?3, '张三成年声', 'adult', 'mimo', 'Milo', 1.0, 0.0, '角色对白')",
            params![character_profile_id, summary.manifest.id, character_id],
        )
        .unwrap();

        tts::bind_voice_profile_to_matching_segments(&conn, &narrator_profile_id, None, true)
            .unwrap();
        tts::bind_voice_profile_to_matching_segments(
            &conn,
            &character_profile_id,
            Some(&character_id),
            true,
        )
        .unwrap();

        let narration_profile: String = conn
            .query_row(
                "SELECT voice_profile_id FROM segments WHERE segment_type = 'narration' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let dialogue_profile: String = conn
            .query_row(
                "SELECT voice_profile_id FROM segments WHERE character_id = ?1 LIMIT 1",
                [&character_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(narration_profile, narrator_profile_id);
        assert_eq!(dialogue_profile, character_profile_id);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exports_production_package_after_mock_generation() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "制作包测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();

        let blocked = audio::export_production_package(&conn, &root).unwrap_err();
        assert!(blocked.to_string().contains("发布检查未通过"));
        approve_all_segment_audio(&conn);
        let package_path = audio::export_production_package(&conn, &root).unwrap();

        assert!(package_path.exists());
        assert!(package_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("production-package-"));
        assert!(root.join("exports/production-check.json").exists());
        assert!(root.join("exports/production-check.md").exists());
        assert!(root.join("exports/production-manifest.json").exists());
        assert!(root.join("exports/character-script.csv").exists());
        assert!(root.join("exports/character-script.json").exists());
        let manifest_text =
            fs::read_to_string(root.join("exports/production-manifest.json")).unwrap();
        let manifest: Value = serde_json::from_str(&manifest_text).unwrap();
        assert_eq!(
            manifest
                .pointer("/assets/0/version")
                .and_then(|value| value.as_i64()),
            Some(1)
        );
        let exported_audio_path = manifest
            .pointer("/assets/0/exportPath")
            .and_then(|value| value.as_str())
            .unwrap()
            .to_string();
        assert!(manifest
            .pointer("/assets/0/audioId")
            .and_then(|value| value.as_str())
            .is_some());
        assert!(manifest
            .pointer("/files")
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("production-manifest.json")));
        assert!(manifest
            .pointer("/files")
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some(exported_audio_path.as_str())));
        let zip_file = fs::File::open(&package_path).unwrap();
        let mut archive = zip::ZipArchive::new(zip_file).unwrap();
        let zipped_manifest_text = {
            let mut manifest_entry = archive.by_name("production-manifest.json").unwrap();
            let mut text = String::new();
            manifest_entry.read_to_string(&mut text).unwrap();
            text
        };
        assert!(zipped_manifest_text.contains("\"projectTitle\""));
        assert!(archive.by_name("character-script.csv").is_ok());
        assert!(archive.by_name("character-script.json").is_ok());
        assert!(archive.by_name(&exported_audio_path).is_ok());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn multi_chapter_segments_and_exports_keep_chapter_order_and_names() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "多章节顺序测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(
            &source_path,
            "第一章 开场\n张三：第一句。\n旁白一。\n第二章 转折\n李四：第二句。\n旁白二。",
        )
        .unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        assert_eq!(chapter_ids.len(), 2);
        for chapter_id in &chapter_ids {
            importer::seed_segments_from_chapter(&conn, chapter_id, false).unwrap();
        }

        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();
        let segments = storage::list_segments(&conn, None).unwrap();
        assert!(!segments.is_empty());
        assert!(segments
            .windows(2)
            .all(|pair| pair[0].chapter_id != pair[1].chapter_id
                || pair[0].order_index < pair[1].order_index));
        let first_chapter_count = segments
            .iter()
            .take_while(|segment| segment.chapter_id == chapter_ids[0])
            .count();
        assert!(first_chapter_count > 0);
        assert!(segments[first_chapter_count..]
            .iter()
            .all(|segment| segment.chapter_id == chapter_ids[1]));

        approve_all_segment_audio(&conn);
        let exported_dir = audio::export_segment_audio(&conn, &root).unwrap();
        let mut exported_names = fs::read_dir(&exported_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        exported_names.sort();
        assert!(exported_names
            .iter()
            .any(|name| name.starts_with("chapter-0001-segment-")));
        assert!(exported_names
            .iter()
            .any(|name| name.starts_with("chapter-0002-segment-")));

        let selected_chapters = vec![chapter_ids[1].clone()];
        let selected_dir =
            audio::export_segment_audio_for_chapters(&conn, &root, Some(&selected_chapters))
                .unwrap();
        let selected_names = fs::read_dir(selected_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(!selected_names.is_empty());
        assert!(selected_names
            .iter()
            .all(|name| name.starts_with("chapter-0002-segment-")));
        let selected_script =
            audio::export_voice_script_for_chapters(&conn, &root, Some(&selected_chapters))
                .unwrap();
        let selected_script_json =
            fs::read_to_string(selected_script.with_extension("json")).unwrap();
        let selected_segments: Vec<Value> = serde_json::from_str(&selected_script_json).unwrap();
        assert_eq!(
            selected_segments.len(),
            segments.len() - first_chapter_count
        );
        let selected_report = audio::build_production_check_report_for_chapters(
            &conn,
            &root,
            Some(&selected_chapters),
        )
        .unwrap();
        assert_eq!(
            selected_report.total_segments as usize,
            selected_segments.len()
        );

        let episode_paths = audio::collect_episode_audio_paths(&conn, &root).unwrap();
        let expected_paths = segments
            .iter()
            .map(|segment| {
                let audio = storage::latest_audio_for_segment(&conn, &segment.id)
                    .unwrap()
                    .unwrap();
                audio::resolve_audio_path(&root, &audio.relative_path).unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(episode_paths, expected_paths);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn episode_export_requires_every_segment_to_be_approved() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "整集导出门槛测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();

        let missing = audio::collect_episode_audio_paths(&conn, &root).unwrap_err();
        assert!(missing.to_string().contains("发布检查未通过"));

        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();
        let unreviewed = audio::collect_episode_audio_paths(&conn, &root).unwrap_err();
        assert!(unreviewed.to_string().contains("尚未审听通过"));

        approve_all_segment_audio(&conn);
        let approved_paths = audio::collect_episode_audio_paths(&conn, &root).unwrap();
        let segment_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM segments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(approved_paths.len() as i64, segment_count);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn production_check_blocks_missing_audio_and_passes_after_approval() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "发布检查测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();

        let blocked = audio::export_production_check_report(&conn, &root).unwrap();
        assert!(!blocked.can_publish);
        assert_eq!(blocked.missing_audio, blocked.total_segments);

        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();
        let unreviewed = audio::export_production_check_report(&conn, &root).unwrap();
        assert!(!unreviewed.can_publish);
        assert_eq!(unreviewed.missing_audio, 0);
        assert_eq!(unreviewed.unreviewed_segments, unreviewed.total_segments);
        let segment_ids = conn
            .prepare("SELECT id FROM segments ORDER BY order_index")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for segment_id in segment_ids {
            audio::set_latest_segment_audio_status(&conn, &segment_id, "approved").unwrap();
        }
        let ready = audio::export_production_check_report(&conn, &root).unwrap();
        assert!(ready.can_publish);
        assert_eq!(ready.missing_audio, 0);
        assert_eq!(ready.rejected_segments, 0);
        assert_eq!(ready.unreviewed_segments, 0);
        assert!(ready.total_duration_ms > 0);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn review_uses_newest_audio_and_open_issue_blocks_release() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "审听版本测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_id = importer::import_source(&conn, &summary.manifest.id, &source)
            .unwrap()
            .remove(0);
        importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();
        let segment_id: String = conn
            .query_row("SELECT id FROM segments LIMIT 1", [], |row| row.get(0))
            .unwrap();

        tts::synthesize_segments(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
        )
        .unwrap();
        audio::set_latest_segment_audio_status(&conn, &segment_id, "approved").unwrap();
        tts::synthesize_segments_with_options(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
            tts::TtsSynthesisOptions {
                force_regenerate: true,
            },
        )
        .unwrap();
        let newest_before_review = storage::newest_audio_for_segment(&conn, &segment_id)
            .unwrap()
            .unwrap();
        assert_eq!(newest_before_review.version, 2);
        assert_eq!(newest_before_review.status, "generated");
        audio::set_latest_segment_audio_status(&conn, &segment_id, "approved").unwrap();
        assert_eq!(
            storage::newest_audio_for_segment(&conn, &segment_id)
                .unwrap()
                .unwrap()
                .version,
            2
        );

        audio::create_review_issue(
            &conn,
            Some(segment_id.clone()),
            None,
            "human_note".to_string(),
            "请检查停顿".to_string(),
        )
        .unwrap();
        let report = audio::build_production_check_report(&conn, &root).unwrap();
        assert!(!report.can_publish);
        assert_eq!(report.unreviewed_segments, 1);
        audio::set_latest_segment_audio_status(&conn, &segment_id, "approved").unwrap();
        let resolved: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM review_issues WHERE segment_id = ?1 AND status = 'resolved'",
                [&segment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(resolved, 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_upload_creates_segment_audio_version() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "人工音频测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        let segment_id: String = conn
            .query_row(
                "SELECT id FROM segments ORDER BY order_index LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let manual_path = root.join("manual.wav");
        fs::write(&manual_path, tiny_wav()).unwrap();

        audio::upload_segment_audio(&conn, &root, &segment_id, &manual_path, None).unwrap();

        let source: String = conn
            .query_row(
                "SELECT source FROM segment_audio WHERE segment_id = ?1",
                [&segment_id],
                |row| row.get(0),
            )
            .unwrap();
        let status: String = conn
            .query_row(
                "SELECT audio_status FROM segments WHERE id = ?1",
                [&segment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(source, "manual_upload");
        assert_eq!(status, "uploaded");
        let relative_path: String = conn
            .query_row(
                "SELECT relative_path FROM segment_audio WHERE segment_id = ?1",
                [&segment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(relative_path.starts_with("assets/audio/"));
        assert!(root.join(&relative_path).exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn approved_audio_is_preferred_over_newer_unreviewed_generation() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "审听优先测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        let segment_id: String = conn
            .query_row(
                "SELECT id FROM segments ORDER BY order_index LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        tts::synthesize_segments_with_options(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
            tts::TtsSynthesisOptions {
                force_regenerate: true,
            },
        )
        .unwrap();
        audio::set_latest_segment_audio_status(&conn, &segment_id, "approved").unwrap();
        tts::synthesize_segments(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
        )
        .unwrap();

        let preferred = storage::latest_audio_for_segment(&conn, &segment_id)
            .unwrap()
            .unwrap();

        assert_eq!(preferred.status, "approved");
        assert_eq!(preferred.version, 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn script_change_invalidates_previously_approved_audio() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "脚本变更失效测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();
        approve_all_segment_audio(&conn);
        assert!(
            audio::export_production_check_report(&conn, &root)
                .unwrap()
                .can_publish
        );
        let segment_id: String = conn
            .query_row(
                "SELECT id FROM segments ORDER BY order_index LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();

        conn.execute(
            "UPDATE segments SET text = '张三：你好，重新录这一句。' WHERE id = ?1",
            [&segment_id],
        )
        .unwrap();
        audio::invalidate_segment_audio(
            &conn,
            &segment_id,
            "脚本内容已变更，请重新生成或上传并审听音频",
        )
        .unwrap();

        let stale_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio WHERE segment_id = ?1 AND status = 'stale'",
                [&segment_id],
                |row| row.get(0),
            )
            .unwrap();
        let (audio_status, review_status): (String, String) = conn
            .query_row(
                "SELECT audio_status, review_status FROM segments WHERE id = ?1",
                [&segment_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let report = audio::export_production_check_report(&conn, &root).unwrap();

        assert_eq!(stale_count, 1);
        assert_eq!(audio_status, "missing");
        assert_eq!(review_status, "unreviewed");
        assert!(!report.can_publish);
        assert_eq!(report.missing_audio, 1);
        assert!(storage::latest_audio_for_segment(&conn, &segment_id)
            .unwrap()
            .is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn voice_profile_change_invalidates_matching_approved_audio() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "声音变更失效测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        tts::ensure_default_voice_profiles(&conn, &summary.manifest.id).unwrap();
        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();
        approve_all_segment_audio(&conn);
        assert!(
            audio::export_production_check_report(&conn, &root)
                .unwrap()
                .can_publish
        );

        let affected_segments = tts::segment_ids_matching_voice_scope(&conn, None, true).unwrap();
        let narrator_profile_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO voice_profiles (id, project_id, character_id, name, age_stage, tts_provider, voice_id, speed, pitch, style)
             VALUES (?1, ?2, NULL, '旁白新声线', 'adult', 'mimo', 'Chloe', 1.0, 0.0, '新的旁白声线')",
            params![narrator_profile_id, summary.manifest.id],
        )
        .unwrap();
        tts::bind_voice_profile_to_matching_segments(&conn, &narrator_profile_id, None, true)
            .unwrap();
        for segment_id in &affected_segments {
            audio::invalidate_segment_audio(
                &conn,
                segment_id,
                "声音配置已变更，请重新生成或上传并审听音频",
            )
            .unwrap();
        }

        let report = audio::export_production_check_report(&conn, &root).unwrap();
        let stale_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio WHERE status = 'stale'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let changed_voice_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE segment_type = 'narration' AND voice_profile_id = ?1",
                [&narrator_profile_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(affected_segments.len(), 1);
        assert_eq!(stale_count, 1);
        assert_eq!(changed_voice_count, 1);
        assert!(!report.can_publish);
        assert_eq!(report.missing_audio, 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn voice_profile_change_does_not_target_manual_upload_audio() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "人工成品保护测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        tts::ensure_default_voice_profiles(&conn, &summary.manifest.id).unwrap();
        let segment_id: String = conn
            .query_row(
                "SELECT id FROM segments ORDER BY order_index LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let manual_path = root.join("manual.wav");
        fs::write(&manual_path, tiny_wav()).unwrap();
        audio::upload_segment_audio(&conn, &root, &segment_id, &manual_path, None).unwrap();
        audio::set_latest_segment_audio_status(&conn, &segment_id, "approved").unwrap();

        let affected_segments = tts::segment_ids_matching_voice_scope(&conn, None, true).unwrap();

        assert!(affected_segments.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merging_characters_uses_target_voice_and_invalidates_source_tts_audio() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "角色合并声音测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n李四：你好。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        ai::extract_characters(&conn, &summary.manifest.id).unwrap();
        let source_character_id = character_id_by_name(&conn, "张三");
        let target_character_id = character_id_by_name(&conn, "李四");
        let source_profile_id = Uuid::new_v4().to_string();
        let target_profile_id = Uuid::new_v4().to_string();
        insert_character_voice(
            &conn,
            &summary.manifest.id,
            &source_character_id,
            &source_profile_id,
            "张三声线",
            "Milo",
        );
        insert_character_voice(
            &conn,
            &summary.manifest.id,
            &target_character_id,
            &target_profile_id,
            "李四声线",
            "Chloe",
        );
        tts::bind_voice_profile_to_matching_segments(
            &conn,
            &source_profile_id,
            Some(&source_character_id),
            true,
        )
        .unwrap();
        tts::bind_voice_profile_to_matching_segments(
            &conn,
            &target_profile_id,
            Some(&target_character_id),
            true,
        )
        .unwrap();
        tts::synthesize_segments(&conn, &root, &summary.manifest.id, Vec::new(), None).unwrap();
        approve_all_segment_audio(&conn);

        crate::merge_character_records(&conn, &source_character_id, &target_character_id).unwrap();

        let source_segment_state: (String, String, String) = conn
            .query_row(
                "SELECT character_id, voice_profile_id, audio_status
                 FROM segments WHERE speaker = '张三'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let stale_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio a
                 JOIN segments s ON s.id = a.segment_id
                 WHERE s.speaker = '张三' AND a.status = 'stale'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let alias_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM character_aliases WHERE character_id = ?1 AND alias = '张三'",
                [&target_character_id],
                |row| row.get(0),
            )
            .unwrap();
        let report = audio::export_production_check_report(&conn, &root).unwrap();

        assert_eq!(source_segment_state.0, target_character_id);
        assert_eq!(source_segment_state.1, target_profile_id);
        assert_eq!(source_segment_state.2, "missing");
        assert_eq!(stale_count, 1);
        assert_eq!(alias_count, 1);
        assert!(!report.can_publish);
        assert_eq!(report.missing_audio, 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn merging_into_character_without_voice_adopts_source_voice_profile() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "角色合并继承声音测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n李四：你好。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        ai::extract_characters(&conn, &summary.manifest.id).unwrap();
        let source_character_id = character_id_by_name(&conn, "张三");
        let target_character_id = character_id_by_name(&conn, "李四");
        let source_profile_id = Uuid::new_v4().to_string();
        insert_character_voice(
            &conn,
            &summary.manifest.id,
            &source_character_id,
            &source_profile_id,
            "张三声线",
            "Milo",
        );

        crate::merge_character_records(&conn, &source_character_id, &target_character_id).unwrap();

        let adopted_character_id: String = conn
            .query_row(
                "SELECT character_id FROM voice_profiles WHERE id = ?1",
                [&source_profile_id],
                |row| row.get(0),
            )
            .unwrap();
        let moved_voice_profile: String = conn
            .query_row(
                "SELECT voice_profile_id FROM segments WHERE speaker = '张三'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(adopted_character_id, target_character_id);
        assert_eq!(moved_voice_profile, source_profile_id);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_tts_skips_protected_audio_and_force_regenerate_creates_new_version() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "保护生成测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        let segment_id: String = conn
            .query_row(
                "SELECT id FROM segments ORDER BY order_index LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        tts::synthesize_segments(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
        )
        .unwrap();
        audio::set_latest_segment_audio_status(&conn, &segment_id, "approved").unwrap();

        tts::synthesize_segments(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
        )
        .unwrap();
        let count_after_protected_batch: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio WHERE segment_id = ?1",
                [&segment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_after_protected_batch, 1);

        tts::synthesize_segments_with_options(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
            tts::TtsSynthesisOptions {
                force_regenerate: true,
            },
        )
        .unwrap();
        let count_after_force: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio WHERE segment_id = ?1",
                [&segment_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count_after_force, 2);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creates_mimo_voice_clone_asset_and_profile() {
        let root = std::env::temp_dir().join(format!("xiic-clone-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "音色模仿测试", None).unwrap();
        let source_path = root.join("reference.wav");
        fs::write(&source_path, tiny_wav()).unwrap();
        let mut conn = storage::open_connection(&root).unwrap();

        let unauthorized = tts::create_mimo_voice_clone_profile(
            &mut conn,
            &root,
            &summary.manifest.id,
            None,
            "测试音色",
            "adult",
            &source_path,
            Some("自然旁白"),
            false,
        )
        .unwrap_err();
        assert!(unauthorized.to_string().contains("声音使用授权"));

        let profile_id = tts::create_mimo_voice_clone_profile(
            &mut conn,
            &root,
            &summary.manifest.id,
            None,
            "测试音色",
            "adult",
            &source_path,
            Some("自然旁白"),
            true,
        )
        .unwrap();
        let (model, asset_id): (String, String) = conn
            .query_row(
                "SELECT model, voice_asset_id FROM voice_profiles WHERE id = ?1",
                [&profile_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let (relative_path, consent): (String, bool) = conn
            .query_row(
                "SELECT relative_path, consent_confirmed FROM voice_assets WHERE id = ?1",
                [&asset_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(model, "mimo-v2.5-tts-voiceclone");
        assert!(consent);
        assert!(relative_path.starts_with("assets/source/voices/"));
        assert!(root.join(relative_path).exists());

        fs::remove_dir_all(root).unwrap();
    }

    fn tiny_wav() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&36u32.to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&16_000u32.to_le_bytes());
        out.extend_from_slice(&32_000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    fn approve_all_segment_audio(conn: &rusqlite::Connection) {
        let segment_ids = conn
            .prepare("SELECT id FROM segments ORDER BY order_index")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for segment_id in segment_ids {
            audio::set_latest_segment_audio_status(conn, &segment_id, "approved").unwrap();
        }
    }

    fn character_id_by_name(conn: &rusqlite::Connection, name: &str) -> String {
        conn.query_row(
            "SELECT id FROM characters WHERE canonical_name = ?1",
            [name],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn insert_character_voice(
        conn: &rusqlite::Connection,
        project_id: &str,
        character_id: &str,
        profile_id: &str,
        name: &str,
        voice_id: &str,
    ) {
        conn.execute(
            "INSERT INTO voice_profiles (id, project_id, character_id, name, age_stage, tts_provider, voice_id, speed, pitch, style)
             VALUES (?1, ?2, ?3, ?4, 'adult', 'mimo', ?5, 1.0, 0.0, '角色对白')",
            params![profile_id, project_id, character_id, name, voice_id],
        )
        .unwrap();
    }
}
