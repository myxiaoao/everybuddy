#[test]
fn rejects_final_model_count_before_backup_or_write() {
    let fixture = Fixture::new();
    let path = fixture.path(TargetKind::Workbuddy);
    let original = serde_json::to_vec(
        &(0..10_000)
            .map(|i| json!({"id": format!("external-{i}")}))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    fs::write(&path, &original).unwrap();
    let request = fixture.request(vec![TargetKind::Workbuddy]);
    let preview = PreparePublishRequest {
        sources: request.sources.clone(),
        targets: request.targets.clone(),
    };
    assert!(fixture
        .coordinator()
        .preview(&preview, &fixture.paths)
        .is_err());
    assert!(fixture
        .coordinator()
        .execute(&request, &fixture.paths)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(fixture.store.list_backups(None).unwrap().is_empty());
    assert!(fixture.store.pending_file_writes().unwrap().is_empty());
}

#[test]
fn rejects_pretty_print_expansion_before_touching_targets() {
    let fixture = Fixture::new();
    let path = fixture.path(TargetKind::Workbuddy);
    let mut root = json!({"models": [], "padding": ""});
    let overhead = serde_json::to_vec(&root).unwrap().len();
    root["padding"] = json!("x".repeat(crate::target::MAX_TARGET_CONFIG_BYTES - overhead));
    let original = serde_json::to_vec(&root).unwrap();
    ConfigDocument::parse(&original).unwrap();
    fs::write(&path, &original).unwrap();
    let request = fixture.request(vec![TargetKind::Workbuddy]);
    assert!(fixture
        .coordinator()
        .execute(&request, &fixture.paths)
        .is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(fixture.store.list_backups(None).unwrap().is_empty());
}

#[test]
fn rejects_restoring_a_backup_after_target_paths_are_swapped() {
    let fixture = Fixture::new();
    let work = fixture.path(TargetKind::Workbuddy);
    let code = fixture.path(TargetKind::Codebuddy);
    fs::write(&work, b"[]").unwrap();
    fs::write(&code, b"[{\"id\":\"other\"}]").unwrap();
    let request = fixture.request(vec![TargetKind::Workbuddy]);
    assert!(
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap()
            .success
    );
    let backup = fixture.store.list_backups(None).unwrap().pop().unwrap();
    let swapped = HashMap::from([
        (TargetKind::Workbuddy, code.to_string_lossy().into_owned()),
        (TargetKind::Codebuddy, work.to_string_lossy().into_owned()),
    ]);
    let before = fs::read(&work).unwrap();
    assert!(matches!(
        fixture.coordinator().restore(&backup.id, &swapped),
        Err(CoreError::Conflict(_))
    ));
    assert_eq!(fs::read(work).unwrap(), before);
    assert_eq!(fs::read(code).unwrap(), b"[{\"id\":\"other\"}]");
}

fn stage_interrupted_publish(fixture: &Fixture) -> Vec<PreparedTarget> {
    let mut writes = Vec::new();
    for kind in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
        let configured_path = fixture.path(kind);
        fs::write(&configured_path, b"[]").unwrap();
        let write_path = target_write_path(&configured_path).unwrap();
        fixture
            .coordinator()
            .create_backup(kind, &write_path, b"[]")
            .unwrap();
        writes.push(PreparedTarget {
            kind,
            configured_path,
            write_path,
            original: Some(b"[]".to_vec()),
            output: b"[{\"id\":\"new\"}]".to_vec(),
        });
    }
    fixture.store.begin_file_writes(&writes).unwrap();
    write_and_verify(&writes[0]).unwrap();
    writes
}

#[test]
fn reopened_store_recovers_a_publish_interrupted_after_the_first_target() {
    let fixture = Fixture::new();
    let writes = stage_interrupted_publish(&fixture);
    let reopened = Store::open(&fixture.directory.path().join("everybuddy.db")).unwrap();
    let coordinator = PublishCoordinator {
        store: &reopened,
        backup_root: &fixture.backup_root,
    };
    let issues = coordinator.recover_interrupted().unwrap();
    assert_eq!(issues.len(), 2);
    assert!(issues
        .iter()
        .all(|issue| issue.code == "interruptedWriteRecovered"));
    for write in writes {
        assert_eq!(fs::read(write.write_path).unwrap(), b"[]");
    }
    assert!(reopened.pending_file_writes().unwrap().is_empty());
    assert!(coordinator.recover_interrupted().unwrap().is_empty());
}

#[test]
fn recovery_preserves_external_changes_after_an_interrupted_publish() {
    let fixture = Fixture::new();
    let writes = stage_interrupted_publish(&fixture);
    fs::write(&writes[0].write_path, b"[{\"id\":\"external\"}]").unwrap();
    let issues = fixture.coordinator().recover_interrupted().unwrap();
    assert!(issues
        .iter()
        .any(|issue| issue.code == "interruptedWriteChanged"));
    assert_eq!(
        fs::read(&writes[0].write_path).unwrap(),
        b"[{\"id\":\"external\"}]"
    );
    assert!(fixture.store.pending_file_writes().unwrap().is_empty());
    assert_eq!(fixture.store.list_backups(None).unwrap().len(), 2);
}

#[test]
fn recovery_removes_only_unchanged_new_files() {
    let fixture = Fixture::new();
    let configured_path = fixture.path(TargetKind::Workbuddy);
    let write = PreparedTarget {
        kind: TargetKind::Workbuddy,
        write_path: target_write_path(&configured_path).unwrap(),
        configured_path,
        original: None,
        output: b"[]".to_vec(),
    };
    fixture
        .store
        .begin_file_writes(std::slice::from_ref(&write))
        .unwrap();
    write_and_verify(&write).unwrap();
    fixture.coordinator().recover_interrupted().unwrap();
    assert!(!write.write_path.exists());
}

#[test]
fn successful_publish_clears_its_journal_with_database_state() {
    let fixture = Fixture::new();
    let request = fixture.request(vec![TargetKind::Workbuddy, TargetKind::Codebuddy]);
    assert!(
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap()
            .success
    );
    assert!(fixture.store.pending_file_writes().unwrap().is_empty());
    assert!(fixture
        .coordinator()
        .recover_interrupted()
        .unwrap()
        .is_empty());
    assert_eq!(
        model_ids(&fixture.path(TargetKind::Workbuddy)),
        vec!["gpt-5"]
    );
}

#[test]
fn pending_recovery_blocks_publish_before_creating_more_backups() {
    let fixture = Fixture::new();
    stage_interrupted_publish(&fixture);
    let backups = fixture.store.list_backups(None).unwrap().len();
    let request = fixture.request(vec![TargetKind::Workbuddy]);
    assert!(matches!(
        fixture.coordinator().execute(&request, &fixture.paths),
        Err(CoreError::Conflict(_))
    ));
    assert_eq!(fixture.store.list_backups(None).unwrap().len(), backups);
}

#[test]
fn version_four_migration_preserves_credentials_and_creates_recovery_table() {
    let fixture = Fixture::new();
    let token = fixture.store.gateway_token("gateway").unwrap();
    fixture
        .store
        .execute_test_sql("DROP TABLE pending_file_writes; PRAGMA user_version=4;")
        .unwrap();
    let upgraded = Store::open(&fixture.directory.path().join("everybuddy.db")).unwrap();
    assert_eq!(upgraded.gateway_token("gateway").unwrap(), token);
    assert_eq!(upgraded.models_for_gateway("gateway").unwrap().len(), 1);
    assert!(upgraded.pending_file_writes().unwrap().is_empty());
    assert!(fixture.directory.path().join("migration-backups").is_dir());
}

#[test]
fn startup_reconciles_a_backup_record_interrupted_before_file_creation() {
    let fixture = Fixture::new();
    let backup = fixture
        .coordinator()
        .create_backup(
            TargetKind::Workbuddy,
            &fixture.path(TargetKind::Workbuddy),
            b"[]",
        )
        .unwrap();
    fs::remove_file(backup.path).unwrap();
    fixture.coordinator().recover_interrupted().unwrap();
    assert!(fixture.store.list_backups(None).unwrap().is_empty());
}
