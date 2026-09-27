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

    /// 回归：改角色音色不得把分段引用清空。
    ///
    /// 旧实现 `UPDATE segments SET voice_profile_id = NULL WHERE character_id=? AND audio_status='missing'`
    /// 会让这些分段在合成时 JOIN 不到音色档案 → 被 `COALESCE(v.tts_provider,'mock')` 兜底成 mock
    /// → 整批生成撞上「供应商不一致」报错。这是当时那 26 个 mock 分段的机制性来源。
    #[test]
    fn changing_character_voice_keeps_segments_resolvable() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "改音色回归测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        ai::extract_characters(&conn, &summary.manifest.id).unwrap();
        let character_id = character_id_by_name(&conn, "张三");

        let default_profile_id: String = conn
            .query_row(
                "SELECT id FROM voice_profiles WHERE character_id = ?1 AND is_default = 1",
                [&character_id],
                |row| row.get(0),
            )
            .unwrap();
        tts::validate_voice_resolution(&conn, &summary.manifest.id, Vec::new()).unwrap();

        crate::apply_character_voice(&conn, &character_id, "Milo", "mimo", None, None, None).unwrap();

        // 分段不再持有可独立演化的副本（未定稿时 voice_profile_id 本就为 NULL），
        // 关键是它们仍然能解析到音色——旧 bug 就是解析不到而被兜底成 mock。
        tts::validate_voice_resolution(&conn, &summary.manifest.id, Vec::new()).unwrap();
        let resolved_profile = tts::preferred_voice_profile_for_character(&conn, &character_id)
            .unwrap()
            .expect("角色应当有可解析的音色");
        let resolved_voice_id: String = conn
            .query_row(
                "SELECT voice_id FROM voice_profiles WHERE id = ?1",
                [&resolved_profile],
                |row| row.get(0),
            )
            .unwrap();
        let is_default: i64 = conn
            .query_row(
                "SELECT is_default FROM voice_profiles WHERE id = ?1",
                [&default_profile_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(resolved_voice_id, "Milo", "改完音色后解析到的应当是新的音色");
        assert_eq!(
            resolved_profile, default_profile_id,
            "改音色应就地更新原有档案，而不是新开一条留下孤儿"
        );
        assert_eq!(is_default, 0, "人定过的音色不再是默认档");

        fs::remove_dir_all(root).unwrap();
    }

    /// 生成前的音色闸门：说话人没有音色时必须报错并指出是谁，而不是静默兜底成 mock。
    #[test]
    fn generation_is_blocked_when_a_speaker_has_no_voice() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "音色闸门测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三：你好。\n旁白继续。").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_ids = importer::import_source(&conn, &summary.manifest.id, &source).unwrap();
        importer::seed_segments_from_chapter(&conn, &chapter_ids[0], false).unwrap();
        ai::extract_characters(&conn, &summary.manifest.id).unwrap();

        tts::validate_voice_resolution(&conn, &summary.manifest.id, Vec::new()).unwrap();

        conn.execute("DELETE FROM voice_profiles", []).unwrap();
        let error = tts::validate_voice_resolution(&conn, &summary.manifest.id, Vec::new())
            .expect_err("说话人没有音色时应当报错");
        let message = error.to_string();
        assert!(message.contains("张三"), "报错应指明缺音色的角色：{message}");
        assert!(message.contains("旁白"), "报错应指明旁白：{message}");

        fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn chapter_split_heuristics_cover_common_formats() {
        let formats: Vec<(&str, &str, usize)> = vec![
            ("第一章 陨落的天才\n正文。\n第二章 斗气大陆\n正文。\n第三章 客人\n正文。", "di", 3),
            ("第1章 起程\n正文。\n第2章 相遇\n正文。\n第3章 离开\n正文。", "di", 3),
            ("1. 离开新手村\n正文。\n2. 相遇\n正文。\n3. 战斗\n正文。", "numbered", 3),
            ("1\n正文。\n2\n正文。\n3\n正文。", "number-line", 3),
            ("Chapter 1 The Beginning\n正文。\nChapter 2 The Journey\n正文。\nChapter 3 The End\n正文。", "chapter-en", 3),
        ];
        for (text, expected_id, expected_count) in formats {
            let pattern = importer::detect_chapter_pattern(text)
                .unwrap_or_else(|| panic!("规则 {expected_id} 应能命中"));
            let matched = importer::SPLIT_RULES
                .iter()
                .find(|rule| rule.pattern == pattern.as_str())
                .map(|rule| rule.id);
            assert_eq!(
                matched, Some(expected_id),
                "文本应命中规则 {expected_id}，实际命中 {matched:?}"
            );
            let preview = importer::preview_split(text, Some(&pattern));
            assert_eq!(preview.chapter_count, expected_count);
        }
    }

    #[test]
    fn implausible_patterns_are_rejected() {
        // 只有三个非空行、全部命中：命中数不少于 3 但占满全部正文，不合理，应拒绝
        let text = "第一段内容较多一些。\n第二段内容较多一些。\n第三段内容较多一些。";
        assert!(importer::detect_chapter_pattern(text).is_none());
    }

    #[test]
    fn custom_pattern_overrides_heuristics() {
        let text = "卷一 起程\n正文。\n卷二 相遇\n正文。\n卷三 战斗\n正文。";
        // 内置规则命不中（"卷" 必须跟在 "第" 后），自定义正则可以
        assert!(importer::detect_chapter_pattern(text).is_none());
        let pattern = importer::compile_split_pattern(r"^卷[一二三]").unwrap();
        let preview = importer::preview_split(text, Some(&pattern));
        assert_eq!(preview.chapter_count, 3);
        assert_eq!(preview.sample_titles[0], "卷一 起程");
    }

    #[test]
    fn marking_ignores_generic_speakers_and_narration_speaker() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "泛称过滤测试", None).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(
            &source_path,
            "旁白描述场景。\n“你好。”\n“让开。”\n“我是萧炎。”",
        )
        .unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_id = importer::import_source(&conn, &summary.manifest.id, &source)
            .unwrap()
            .remove(0);
        importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();

        let payload = serde_json::json!({
            "choices": [{"message": {"content": serde_json::json!({
                "segments": [
                    {"text": "旁白描述场景。", "segmentType": "narration", "speaker": "旁白"},
                    {"text": "“你好。”", "segmentType": "dialogue", "speaker": "路人甲"},
                    {"text": "“让开。”", "segmentType": "dialogue", "speaker": "众人"},
                    {"text": "“我是萧炎。”", "segmentType": "dialogue", "speaker": "萧炎"}
                ],
                "characters": [{"name": "旁白", "aliases": []}, {"name": "萧炎", "aliases": ["炎哥"]}]
            }).to_string()}}]
        })
        .to_string();
        let marking = ai::parse_marking_payload(&payload).unwrap();
        ai::apply_llm_marking(&conn, &chapter_id, &marking).unwrap();
        ai::extract_characters(&conn, &summary.manifest.id).unwrap();
        ai::apply_character_aliases(&conn, &summary.manifest.id, &marking.characters).unwrap();

        // 只建了真实角色；旁白和泛称（路人甲、众人）都不建角色
        let mut names: Vec<String> = conn
            .prepare("SELECT canonical_name FROM characters ORDER BY canonical_name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        names.sort();
        assert_eq!(names, vec!["萧炎".to_string()]);

        // narration 段的 speaker 被清空
        let narration_speaker: Option<String> = conn
            .query_row(
                "SELECT speaker FROM segments WHERE text = '旁白描述场景。'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(narration_speaker, None);

        // 泛称 speaker 保留原文标注，但不归属任何角色
        let unassigned: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segments WHERE speaker IN ('路人甲', '众人') AND character_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unassigned, 2);

        // 萧炎的分段已归属角色
        let assigned: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segments s JOIN characters c ON s.character_id = c.id
                 WHERE c.canonical_name = '萧炎'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(assigned, 1);

        fs::remove_dir_all(root).unwrap();
    }

    /* ---------- 音色描述的章节上下文 ---------- */

    fn context_segment(character_id: Option<&str>, text: &str, segment_type: &str) -> crate::domain::Segment {
        crate::domain::Segment {
            id: Uuid::new_v4().to_string(),
            chapter_id: "chapter-1".to_string(),
            scene_id: None,
            order_index: 0,
            text: text.to_string(),
            segment_type: crate::domain::SegmentType::from(segment_type),
            speaker: None,
            character_id: character_id.map(str::to_string),
            emotion: None,
            sound_cue: None,
            anchor: None,
            voice_profile_id: None,
            audio_status: "missing".to_string(),
            review_status: "unreviewed".to_string(),
            age_progress: None,
            is_manual_edit: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// 引号内的对白与引号外的叙述必须分开——旧 bug 就是把整段原文当"台词样本"，
    /// LLM 原样抄回来，用户看到的"音色描述"其实是章节原文。
    #[test]
    fn voice_context_splits_quoted_lines_from_narration() {
        let raw = "“萧炎，斗之力，三段！级别：低级！”测验魔石碑之旁，一位中年男子，看了一眼碑上所显示出来的信息，语气漠然的将之公布了出来…";
        let (quotes, rest) = crate::voice_context::split_quotes(raw);
        assert_eq!(quotes, vec!["萧炎，斗之力，三段！级别：低级！".to_string()]);
        assert!(rest.contains("语气漠然"));
        assert!(!rest.contains('“'));
    }

    #[test]
    fn voice_context_draft_uses_evidence_not_raw_text() {
        let raw = "“萧炎，斗之力，三段！级别：低级！”测验魔石碑之旁，一位中年男子，看了一眼碑上所显示出来的信息，语气漠然的将之公布了出来…";
        let segments = vec![
            context_segment(Some("char-1"), raw, "dialogue"),
            context_segment(
                None,
                "中年男子话刚刚脱口，便是不出意外的在人头汹涌的广场上带起了一阵嘲讽的骚动。",
                "narration",
            ),
        ];
        let context = crate::voice_context::assemble("char-1", "中年男子", &[], &segments);
        assert_eq!(context.line_count(), 1);
        assert!(
            context.narration().iter().any(|item| item.contains("语气漠然")),
            "引号外的叙述应当作为人物气质线索保留"
        );
        assert!(
            context.narration().iter().any(|item| item.contains("骚动")),
            "提到该角色的其它叙述也应进入上下文"
        );

        let draft = crate::voice_context::compose_draft(
            "中年男子",
            &[],
            None,
            None,
            None,
            &context,
        );
        assert!(draft.contains("中年男性"), "应当从上下文推断出年龄段与性别：{draft}");
        assert!(draft.contains("漠然"), "应当引用语气线索：{draft}");
        assert!(
            !draft.contains("测验魔石碑"),
            "草稿不能照抄章节原文：{draft}"
        );
    }

    /// 回归：**不能拿角色自己的台词推性别**。
    ///
    /// 真实数据里「中年男子」的台词提到的是别人（「薰儿小姐…」「对着少女略微恭声道」），
    /// 旧实现把这些词和他的名字一起丢进同一个语料里数词频，
    /// 结果「女」的命中数压过「男」，草稿写成「中年**女**性角色」——音色直接选错。
    #[test]
    fn gender_comes_from_identity_evidence_not_from_spoken_lines() {
        let segments = vec![
            context_segment(
                Some("char-1"),
                "“斗之力，三段！”",
                "dialogue",
            ),
            context_segment(
                Some("char-1"),
                "“薰儿小姐，半年之后，你应该便能凝聚斗气之旋。”",
                "dialogue",
            ),
            context_segment(
                Some("char-1"),
                "望着石碑上的信息，一旁的中年测验员漠然的脸庞上竟然也是罕见的露出了一丝笑意，对着少女略微恭声道：",
                "dialogue",
            ),
        ];
        let context = crate::voice_context::assemble("char-1", "中年男子", &[], &segments);
        let draft = crate::voice_context::compose_draft(
            "中年男子",
            &[],
            None,
            Some("adult"), // AI 标注阶段写下的占位值，不能压过名字里的「中年」
            None,
            &context,
        );
        assert!(
            draft.contains("中年男性角色"),
            "名字自述应当压过台词里提到的女性称谓：{draft}"
        );
        assert!(
            !draft.contains("女性") && !draft.contains("adult"),
            "既不能判成女性，也不能把内部 token 打出来：{draft}"
        );
    }

    /// 人标注的性别/年龄阶段必须归一成中文，且优先于文本推断。
    #[test]
    fn stated_gender_and_age_stage_are_normalized_and_take_priority() {
        assert_eq!(
            crate::voice_context::normalize_gender("female").as_deref(),
            Some("女")
        );
        assert_eq!(
            crate::voice_context::normalize_gender("male").as_deref(),
            Some("男"),
            "female 含 male 子串，必须整串比较"
        );
        assert_eq!(crate::voice_context::age_stage_label("middle_aged").as_deref(), Some("中年"));
        assert_eq!(
            crate::voice_context::age_stage_label("中年").as_deref(),
            Some("中年"),
            "自由文本框里手写的中文也应当认得"
        );
        assert_eq!(crate::voice_context::age_stage_label("adult").as_deref(), Some("成年"));
        assert!(
            crate::voice_context::is_placeholder_age_stage("adult"),
            "导入阶段统一写下的 adult 是占位值，排序时要让位于文本证据"
        );
        assert!(!crate::voice_context::is_placeholder_age_stage("middle_aged"));

        let segments = vec![context_segment(
            None,
            "少女微微点了点头，柔软的嗓音在广场上散开。",
            "narration",
        )];
        let context = crate::voice_context::assemble("char-1", "萧薰儿", &[], &segments);
        let draft = crate::voice_context::compose_draft(
            "萧薰儿",
            &[],
            Some("female"),
            None,
            None,
            &context,
        );
        assert!(draft.contains("女性角色"), "人标注的性别优先：{draft}");
    }

    /// 一段叙述里同时写到多个人时，词频无法判断哪个词说的是谁——
    /// 这时必须**拒绝下结论**，而不是挑词频高的那个。
    ///
    /// 真实数据：萧炎的叙述里有「面对着少女毫不掩饰的坦率话语，少年尴尬的笑了一声」，
    /// 也有「少女顿下了脚步，对着萧炎恭敬的弯了弯腰」。
    /// 「少女」与「少年」都紧挨着名字，谁分高全靠词表顺序，写成「少年女性角色」音色就选错了。
    #[test]
    fn conflicting_gender_cues_yield_no_claim() {
        let segments = vec![
            context_segment(
                Some("char-1"),
                "“我现在还有资格让你怎么叫么?”",
                "dialogue",
            ),
            context_segment(
                None,
                "面对着少女毫不掩饰的坦率话语，少年尴尬的笑了一声，落寞的回转过身。",
                "narration",
            ),
        ];
        let context = crate::voice_context::assemble("char-1", "萧炎", &[], &segments);
        let draft = crate::voice_context::compose_draft("萧炎", &[], None, None, None, &context);
        assert!(
            !draft.contains("女性") && !draft.contains("男性"),
            "证据冲突时不能给出性别：{draft}"
        );
        assert!(
            draft.contains("未能确定性别"),
            "不确定就要说明，让用户先确认再合成：{draft}"
        );
    }

    /// 占位年龄阶段兜底时必须声明"这是默认值"。
    ///
    /// 真实数据：萧炎的叙述里有「少年」（他自己）也有「一位中年男子」（旁人），
    /// 年龄线索冲突 → 只能退回导入时写下的占位值 `adult` → 成年。
    /// 这个成年不是从文里读出来的，草稿必须讲明白，否则用户会以为已经确认过。
    #[test]
    fn placeholder_age_stage_fallback_is_disclosed() {
        let segments = vec![
            context_segment(Some("char-1"), "“我现在还有资格让你怎么叫么?”", "dialogue"),
            context_segment(
                None,
                "测验魔石碑之旁，一位中年男子漠然的将信息公布了出来。",
                "narration",
            ),
            context_segment(
                None,
                "面对着少女毫不掩饰的坦率话语，少年尴尬的笑了一声。",
                "narration",
            ),
        ];
        let context = crate::voice_context::assemble("char-1", "萧炎", &[], &segments);
        let draft =
            crate::voice_context::compose_draft("萧炎", &[], None, Some("adult"), None, &context);
        assert!(draft.contains("成年"), "占位值仍可使用：{draft}");
        assert!(
            draft.contains("年龄阶段按默认值处理"),
            "兜底出来的年龄必须声明是默认值：{draft}"
        );
        assert!(
            !draft.contains("成年男性") && !draft.contains("成年女性"),
            "线索冲突时不能给性别：{draft}"
        );
    }

    /// 名字/别名自述压过叙述里的杂音：名字里有「男子」就是男性，
    /// 哪怕叙述里同时出现了「少女」。
    #[test]
    fn name_self_description_outweighs_noisy_narration() {
        let segments = vec![
            context_segment(
                Some("char-1"),
                "“下一个，萧媚！”",
                "dialogue",
            ),
            context_segment(
                None,
                "测验魔石碑之旁，一位中年男子漠然开口，随后对着广场另一侧的少女略微恭声说了几句。",
                "narration",
            ),
        ];
        let context = crate::voice_context::assemble("char-1", "中年男子", &[], &segments);
        let draft = crate::voice_context::compose_draft("中年男子", &[], None, None, None, &context);
        assert!(draft.contains("中年男性角色"), "名字自述最可靠：{draft}");
        assert!(!draft.contains("女性"), "不能被叙述里的「少女」带偏：{draft}");
    }

    /// LLM 把章节原文抄回来时必须被判为不合格，从而触发严格重试或回退草稿。
    #[test]
    fn voice_description_validation_rejects_echoed_chapter_text() {
        let lines = vec!["萧炎，斗之力，三段！级别：低级！".to_string()];
        assert!(ai::invalid_voice_description_reason(
            "“萧炎，斗之力，三段！级别：低级！”测验魔石碑之旁，一位中年男子。",
            &lines
        )
        .is_some());
        assert!(ai::invalid_voice_description_reason(&lines[0], &lines).is_some());
        assert!(ai::invalid_voice_description_reason("中年男性，中低音色，语气漠然克制，语速平稳偏慢。", &lines)
            .is_none());
    }

    /// 回归：拆分分段时人工填的情绪要跟着拆，别每拆一次就丢一次。
    /// （`split_segment_at` 曾在 INSERT 里把 emotion 写死成 NULL。）
    #[test]
    fn splitting_a_segment_keeps_the_emotion_for_both_halves() {
        let root = std::env::temp_dir().join(format!("xiic-split-emotion-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source_path = root.join("sample.txt");
        fs::write(&source_path, "张三: 斗之力，三段！级别：低级！").unwrap();
        let source = importer::read_source(&source_path).unwrap();
        let summary = storage::create_project(&root, "拆分情绪测试", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let chapter_id = importer::import_source(&conn, &summary.manifest.id, &source)
            .unwrap()
            .remove(0);
        importer::seed_segments_from_chapter(&conn, &chapter_id, false).unwrap();
        let segment_id: String = conn
            .query_row(
                "SELECT id FROM segments ORDER BY order_index LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        conn.execute(
            "UPDATE segments SET emotion = '大声宣布', audio_status = 'generated' WHERE id = ?1",
            [&segment_id],
        )
        .unwrap();

        storage::split_segment_at(&conn, &segment_id, 5).unwrap();

        let rows: Vec<(String, Option<String>, String)> = conn
            .prepare("SELECT text, emotion, audio_status FROM segments ORDER BY order_index")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rows.len(), 2);
        for (text, emotion, status) in &rows {
            assert_eq!(
                emotion.as_deref(),
                Some("大声宣布"),
                "拆出来的两半都应保留情绪：{text}"
            );
            assert_eq!(status, "missing", "拆分后音频必须失效、等重新生成：{text}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// 可空人工字段的归一：空白一律落 NULL，别在库里留下 `''`
    /// （否则「清空情绪」和「本来就是空」会被算成两次不同的改动，白让音频失效）。
    #[test]
    fn blank_optional_text_normalizes_to_none() {
        assert_eq!(
            crate::normalize_optional_text(Some("大声宣布")),
            Some("大声宣布".to_string())
        );
        assert_eq!(
            crate::normalize_optional_text(Some("  大声宣布 ")),
            Some("大声宣布".to_string())
        );
        assert_eq!(crate::normalize_optional_text(Some("")), None);
        assert_eq!(crate::normalize_optional_text(Some("   ")), None);
        assert_eq!(crate::normalize_optional_text(None), None);
    }

    /// 自动失效提示：同一分段同一类型只允许一条，且重新生成后必须消失。
    ///
    /// 症状回归：每「改一次 → 重新生成一次」就多插一条 `script_changed`，
    /// 界面上堆成一串同名的「脚本内容已变更」（实测某分段累积 4 条、横跨 16 天），
    /// 而重新生成那条提示要求的动作之后，它自己却还挂着。
    #[test]
    fn auto_invalidation_keeps_one_open_issue_and_regeneration_closes_it() {
        let root = std::env::temp_dir().join(format!("xiic-issue-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "失效提示测试", None).unwrap();
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

        let count_open = |issue_type: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM review_issues
                 WHERE segment_id = ?1 AND issue_type = ?2 AND status = 'open'",
                params![segment_id, issue_type],
                |row| row.get(0),
            )
            .unwrap()
        };

        tts::synthesize_segments(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
        )
        .unwrap();

        // 三轮「改了脚本/音色 → 重新生成」，每轮都必须回到干净状态
        for reason in ["脚本内容已变更", "角色音色已固化", "声音配置已变更"] {
            audio::invalidate_segment_audio(&conn, &segment_id, reason).unwrap();
            assert_eq!(
                count_open("script_changed"),
                1,
                "失效后应有且仅有一条 open 提示（不能累加）：{reason}"
            );
            tts::synthesize_segments_with_options(
                &conn,
                &root,
                &summary.manifest.id,
                vec![segment_id.clone()],
                None,
                tts::TtsSynthesisOptions {
                    force_regenerate: false,
                },
            )
            .unwrap();
            assert_eq!(
                count_open("script_changed"),
                0,
                "音频已按当前脚本重新生成，提示必须消失：{reason}"
            );
        }

        // 人工审听意见是人的判断，不能被"重新生成"顺手抹掉
        audio::create_review_issue(
            &conn,
            Some(segment_id.clone()),
            None,
            "human_note".to_string(),
            "第 3 句停顿太长".to_string(),
        )
        .unwrap();
        audio::invalidate_segment_audio(&conn, &segment_id, "脚本内容已变更").unwrap();
        tts::synthesize_segments_with_options(
            &conn,
            &root,
            &summary.manifest.id,
            vec![segment_id.clone()],
            None,
            tts::TtsSynthesisOptions {
                force_regenerate: false,
            },
        )
        .unwrap();
        assert_eq!(count_open("script_changed"), 0, "自动提示应被关闭");
        assert_eq!(
            count_open("human_note"),
            1,
            "人工审听意见必须保留，只能由人确认关闭"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_character_creation_blocks_duplicate_names_and_alias_collisions() {
        let root = std::env::temp_dir().join(format!("xiic-test-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "手动建角色测试", None).unwrap();
        let project_id = summary.manifest.id.clone();
        let mut conn = storage::open_connection(&root).unwrap();

        // 名字两端空白要 trim；别名里与本体同名或相互重复的都要丢掉
        let id = storage::create_character(
            &mut conn,
            &project_id,
            "  药老 ",
            &[
                "药尘".to_string(),
                "药老".to_string(),
                "药尘".to_string(),
            ],
            Some("male"),
            Some("老年"),
            None,
        )
        .unwrap();
        let (name, color): (String, String) = conn
            .query_row(
                "SELECT canonical_name, default_color FROM characters WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "药老");
        assert_eq!(color, ai::pick_character_color("药老"));
        let aliases: Vec<String> = conn
            .prepare("SELECT alias FROM character_aliases WHERE character_id = ?1")
            .unwrap()
            .query_map(params![id], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(aliases, vec!["药尘".to_string()]);

        // 命令层的收尾动作：新建后必须补一条默认音色档，
        // 否则这个角色一进合成就撞上「没有音色」的闸门。
        tts::ensure_default_voice_profiles(&conn, &project_id).unwrap();
        let default_profiles: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM voice_profiles WHERE character_id = ?1 AND is_default = 1",
                params![id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(default_profiles, 1);

        // 重名：界面里出现两个无从分辨的同名角色，归属会立刻烂掉
        let duplicate =
            storage::create_character(&mut conn, &project_id, "药老", &[], None, None, None)
                .unwrap_err()
                .to_string();
        assert!(duplicate.contains("已经有叫「药老」的角色"), "{duplicate}");

        // 别名撞车：既包括撞上已有角色的**名字**，也包括撞上它的**别名**。
        // 别名共用 = 分段会被并到错的人名下，必须挡住。
        for alias in ["药老", "药尘"] {
            let clash = storage::create_character(
                &mut conn,
                &project_id,
                "萧炎",
                &[alias.to_string()],
                None,
                None,
                None,
            )
            .unwrap_err()
            .to_string();
            assert!(clash.contains(alias), "{clash}");
        }
        assert!(
            storage::create_character(
                &mut conn,
                &project_id,
                "萧炎",
                &["炎帝".to_string()],
                None,
                None,
                None,
            )
            .is_ok(),
            "不冲突的别名应该放行"
        );

        fs::remove_dir_all(root).unwrap();
    }

    /// 卡片上的「试听固化样本」按钮完全依赖这个函数：
    /// 它必须把"文件被外部删了"如实报成 None，否则前端会拿一个播不响的路径
    /// 去喂播放器 —— 用户点了没声音、也没有任何提示。
    #[test]
    fn voice_asset_audio_path_only_returns_paths_that_exist() {
        let root = std::env::temp_dir().join(format!("xiic-voice-asset-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "音色样本路径", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let asset_id = Uuid::new_v4().to_string();
        let relative_path = format!("assets/source/voices/{asset_id}.wav");
        let timestamp = storage::now();
        conn.execute(
            "INSERT INTO voice_assets
             (id, project_id, name, asset_type, provider, model, relative_path, mime_type,
              source_file_name, consent_confirmed, status, created_at, updated_at)
             VALUES (?1, ?2, '音色样本', 'voice_design_sample', 'mimo', 'mimo-v2.5-tts-voicedesign',
                     ?3, 'audio/wav', 'design-x', 1, 'ready', ?4, ?5)",
            params![
                asset_id,
                summary.manifest.id,
                relative_path,
                timestamp,
                timestamp
            ],
        )
        .unwrap();

        // 记录在库里、文件还没落盘：只能给 None
        assert!(storage::voice_asset_audio_path(&conn, &root, &summary.manifest.id, &asset_id)
            .unwrap()
            .is_none());

        let destination = root.join(&relative_path);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::write(&destination, b"RIFF").unwrap();
        assert_eq!(
            storage::voice_asset_audio_path(&conn, &root, &summary.manifest.id, &asset_id).unwrap(),
            Some(destination.to_string_lossy().to_string())
        );

        // 文件删掉之后必须重新变回 None（不能只信库里的记录）
        fs::remove_file(&destination).unwrap();
        assert!(storage::voice_asset_audio_path(&conn, &root, &summary.manifest.id, &asset_id)
            .unwrap()
            .is_none());

        // 资产不属于当前项目时也查不到
        assert!(storage::voice_asset_audio_path(&conn, &root, "other-project", &asset_id)
            .unwrap()
            .is_none());

        fs::remove_dir_all(root).unwrap();
    }

    /* ---------- 删除 / 撤销删除 ---------- */

    /// 造一个章节，塞进 `count` 条分段，返回 (chapter_id, 各分段 id)。
    /// 这些测试关心的是删除链路，所以直接写表，不走导入 + 标注那条长链路。
    fn seed_chapter(conn: &rusqlite::Connection, project_id: &str, count: i64) -> (String, Vec<String>) {
        let chapter_id = Uuid::new_v4().to_string();
        let timestamp = storage::now();
        conn.execute(
            "INSERT INTO chapters (id, project_id, title, order_index, raw_text, script_status, created_at, updated_at)
             VALUES (?1, ?2, '第一章', 0, '原文', 'imported', ?3, ?4)",
            params![chapter_id, project_id, timestamp, timestamp],
        )
        .unwrap();

        let mut ids = Vec::new();
        for index in 0..count {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO segments (id, chapter_id, scene_id, order_index, text, segment_type, speaker, character_id, emotion, sound_cue, anchor, voice_profile_id, audio_status, review_status, age_progress, is_manual_edit, created_at, updated_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, 'dialogue', '萧炎', NULL, ?5, NULL, NULL, NULL, 'ready', 'approved', NULL, 0, ?6, ?7)",
                params![
                    id,
                    chapter_id,
                    index,
                    format!("第 {index} 句"),
                    format!("情绪{index}"),
                    timestamp,
                    timestamp
                ],
            )
            .unwrap();
            ids.push(id);
        }
        (chapter_id, ids)
    }

    /// 删除必须是可撤销的，而且要**原样**撤销：
    /// 文本、情绪、序号、音频记录、审听备注全都得回来。
    /// 只恢复文本不算数——用户的音频是花钱生成的。
    #[test]
    fn deleting_a_segment_can_be_undone_intact() {
        let root = std::env::temp_dir().join(format!("xiic-undo-delete-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "撤销删除", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let (chapter_id, ids) = seed_chapter(&conn, &summary.manifest.id, 3);
        let target = ids[1].clone();
        let timestamp = storage::now();

        // 给目标分段配一条音频记录 + 一条人工审听备注
        let audio_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO segment_audio (id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at)
             VALUES (?1, ?2, 'assets/audio/keep-me.wav', 1200, -16.0, 1, 'tts', 'ready', ?3)",
            params![audio_id, target, timestamp],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO review_issues (id, segment_id, audio_id, issue_type, note, status, created_at)
             VALUES (?1, ?2, ?3, 'human_note', '这里咬字不清', 'open', ?4)",
            params![Uuid::new_v4().to_string(), target, audio_id, timestamp],
        )
        .unwrap();

        let archived = storage::delete_segment(&conn, &summary.manifest.id, &target).unwrap();
        assert_eq!(archived.position_index, 1);
        assert_eq!(archived.text_preview, "第 1 句");

        // 活表里三样都得消失（分段本体、音频记录、审听备注）
        let alive = storage::list_segments(&conn, Some(&chapter_id)).unwrap();
        assert_eq!(alive.len(), 2, "删除后活分段应只剩两条");
        assert!(!alive.iter().any(|segment| segment.id == target));
        let audio_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio WHERE segment_id = ?1",
                params![target],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audio_count, 0, "音频记录应随分段一起离开活表");
        let issue_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM review_issues WHERE segment_id = ?1",
                params![target],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(issue_count, 0, "审听备注应随分段一起离开活表");

        let restored = storage::restore_segment(&conn, &target).unwrap();
        assert_eq!(restored.id, target);
        assert_eq!(restored.order_index, 1, "必须落回原来的序号");
        assert_eq!(restored.emotion.as_deref(), Some("情绪1"));
        assert_eq!(restored.audio_status, "ready", "音频状态不能被重置");
        assert_eq!(restored.review_status, "approved", "审听结论不能被重置");

        let alive = storage::list_segments(&conn, Some(&chapter_id)).unwrap();
        assert_eq!(
            alive.iter().map(|segment| segment.order_index).collect::<Vec<_>>(),
            vec![0, 1, 2],
            "恢复后序号不能被撑出空洞"
        );
        assert_eq!(alive[1].id, target);
        let audio_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM segment_audio WHERE segment_id = ?1",
                params![target],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audio_count, 1, "音频记录必须回来");
        let note: String = conn
            .query_row(
                "SELECT note FROM review_issues WHERE segment_id = ?1",
                params![target],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(note, "这里咬字不清", "审听备注必须回来");

        // 归档是一次性的：撤销过一次就不能再撤一遍
        assert!(storage::restore_segment(&conn, &target).is_err());
        assert!(storage::list_deleted_segments(&conn, &summary.manifest.id)
            .unwrap()
            .is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    /// 归档里引用的角色可能在删除期间被删掉。
    /// 这时**不能拒绝恢复**——分段文本和音频远比"当时配的哪个角色"重要，
    /// 失效的引用降级成 NULL 即可。
    #[test]
    fn restoring_a_segment_survives_a_deleted_character() {
        let root = std::env::temp_dir().join(format!("xiic-undo-char-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "撤销遇角色被删", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let (_, ids) = seed_chapter(&conn, &summary.manifest.id, 2);
        let target = ids[0].clone();

        let character_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO characters (id, project_id, canonical_name, gender, age_timeline, notes, default_color)
             VALUES (?1, ?2, '萧炎', NULL, NULL, NULL, '#276ef1')",
            params![character_id, summary.manifest.id],
        )
        .unwrap();
        conn.execute(
            "UPDATE segments SET character_id = ?1 WHERE id = ?2",
            params![character_id, target],
        )
        .unwrap();

        storage::delete_segment(&conn, &summary.manifest.id, &target).unwrap();
        // 归档之后角色没了
        conn.execute("DELETE FROM characters WHERE id = ?1", params![character_id])
            .unwrap();

        let restored = storage::restore_segment(&conn, &target).unwrap();
        assert_eq!(restored.text, "第 0 句", "文本必须完整回来");
        assert_eq!(
            restored.character_id, None,
            "角色已不存在，引用要降级成 NULL 而不是让整条恢复失败"
        );

        fs::remove_dir_all(root).unwrap();
    }

    /// 原序号在这期间被别人占了（用户删完又插了新句），
    /// 恢复时要让位——把 >= 该序号的整段后移，而不是覆盖或报错。
    #[test]
    fn restoring_a_segment_makes_room_when_the_slot_is_taken() {
        let root = std::env::temp_dir().join(format!("xiic-undo-slot-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "撤销让位", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let (chapter_id, ids) = seed_chapter(&conn, &summary.manifest.id, 3);
        let target = ids[1].clone();

        storage::delete_segment(&conn, &summary.manifest.id, &target).unwrap();
        // 在序号 1 插一句新的，占掉刚空出来的位置
        storage::insert_segment_after(
            &conn,
            &chapter_id,
            Some(ids[0].as_str()),
            "插进来的新句",
            crate::domain::SegmentType::Dialogue,
            None,
            None,
        )
        .unwrap();

        let restored = storage::restore_segment(&conn, &target).unwrap();
        assert_eq!(restored.order_index, 1, "恢复的段落回原序号");
        let alive = storage::list_segments(&conn, Some(&chapter_id)).unwrap();
        assert_eq!(
            alive.iter().map(|segment| segment.order_index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3],
            "被占位时整段后移，序号仍连续"
        );
        assert_eq!(alive[1].id, target);
        assert_eq!(alive[2].text, "插进来的新句", "新句让位到后一格");

        fs::remove_dir_all(root).unwrap();
    }

    /// 归档不是永久的：超过保留期才真正清理，
    /// 并且要把**磁盘上的音频路径**交出来，让调用方删文件。
    /// 不返回路径 = 归档清了但音频文件永远变成孤儿。
    #[test]
    fn expired_archives_are_purged_with_their_audio_paths() {
        let root = std::env::temp_dir().join(format!("xiic-undo-purge-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "归档清理", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let (_, ids) = seed_chapter(&conn, &summary.manifest.id, 2);
        let target = ids[0].clone();
        conn.execute(
            "INSERT INTO segment_audio (id, segment_id, relative_path, duration_ms, loudness_lufs, version, source, status, created_at)
             VALUES (?1, ?2, 'assets/audio/gone.wav', 900, -18.0, 1, 'tts', 'ready', ?3)",
            params![Uuid::new_v4().to_string(), target, storage::now()],
        )
        .unwrap();

        storage::delete_segment(&conn, &summary.manifest.id, &target).unwrap();

        // 保留期内的不能被清掉
        assert!(storage::purge_expired_deleted_segments(&conn, 7).unwrap().is_empty());
        assert_eq!(
            storage::list_deleted_segments(&conn, &summary.manifest.id).unwrap().len(),
            1
        );

        // 把删除时间挪到 30 天前，再清
        conn.execute(
            "UPDATE deleted_segments SET deleted_at = ?1 WHERE id = ?2",
            params![
                (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339(),
                target
            ],
        )
        .unwrap();

        let paths = storage::purge_expired_deleted_segments(&conn, 7).unwrap();
        assert_eq!(paths, vec!["assets/audio/gone.wav".to_string()]);
        assert!(
            storage::list_deleted_segments(&conn, &summary.manifest.id)
                .unwrap()
                .is_empty(),
            "超期归档应被清掉"
        );
        // 清了之后连撤销也做不了（归档已不存在）
        assert!(storage::restore_segment(&conn, &target).is_err());

        fs::remove_dir_all(root).unwrap();
    }

    /// 空章节的「添加第一句」：没有"上一段"可指，必须能插到章首。
    /// 这条路径以前不存在——章里的分段被删光后，界面上根本没有入口再加回来。
    #[test]
    fn inserting_a_segment_into_an_empty_chapter_lands_first() {
        let root = std::env::temp_dir().join(format!("xiic-insert-empty-{}", Uuid::new_v4()));
        let summary = storage::create_project(&root, "空章节插入", None).unwrap();
        let conn = storage::open_connection(&root).unwrap();
        let (chapter_id, _) = seed_chapter(&conn, &summary.manifest.id, 0);

        storage::insert_segment_after(
            &conn,
            &chapter_id,
            None,
            "第一句",
            crate::domain::SegmentType::Narration,
            None,
            None,
        )
        .unwrap();

        let alive = storage::list_segments(&conn, Some(&chapter_id)).unwrap();
        assert_eq!(alive.len(), 1);
        assert_eq!(alive[0].order_index, 0);
        assert_eq!(alive[0].text, "第一句");

        // 已有分段时插章首，旧的整段后移
        storage::insert_segment_after(
            &conn,
            &chapter_id,
            None,
            "插到最前面",
            crate::domain::SegmentType::Narration,
            None,
            None,
        )
        .unwrap();
        let alive = storage::list_segments(&conn, Some(&chapter_id)).unwrap();
        assert_eq!(
            alive.iter().map(|segment| segment.text.as_str()).collect::<Vec<_>>(),
            vec!["插到最前面", "第一句"]
        );

        fs::remove_dir_all(root).unwrap();
    }

    /// 空章节的「添加第一句」不会传 `afterSegmentId` 这个字段。
    /// 参数从必填改成可选后，反序列化是最脆的一环：缺字段必须落成 None，
    /// 而不是抛 "missing field" —— 那会表现成"点了添加没反应"。
    #[test]
    fn insert_request_tolerates_a_missing_after_segment_id() {
        let request: crate::InsertSegmentRequest =
            serde_json::from_str(r#"{"chapterId":"c1","text":"第一句","segmentType":"narration"}"#)
                .unwrap();
        assert_eq!(request.chapter_id, "c1");
        assert_eq!(request.after_segment_id, None, "缺字段应落成 None");

        let request: crate::InsertSegmentRequest = serde_json::from_str(
            r#"{"chapterId":"c1","afterSegmentId":"s9","text":"补一句","segmentType":"dialogue"}"#,
        )
        .unwrap();
        assert_eq!(request.after_segment_id.as_deref(), Some("s9"), "给了位置就照用");
    }
}
