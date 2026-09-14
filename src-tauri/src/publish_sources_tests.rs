fn add_other_source(fixture: &Fixture) {
    let mut gateway = fixture.store.gateway("gateway").unwrap();
    gateway.id = "other".into();
    gateway.name = "Other source".into();
    gateway.api_root = "https://other.example/v1".into();
    fixture.store.save_gateway(&gateway).unwrap();
    fixture
        .store
        .save_gateway_token("other", "other-token")
        .unwrap();
    for id in ["gpt-5", "claude"] {
        let mut model = fixture.store.model("gateway::gpt-5").unwrap();
        model.id = id.into();
        model.key = format!("other::{id}");
        model.gateway_id = "other".into();
        fixture.store.save_model(&model).unwrap();
    }
}

fn selection(gateway_id: &str, ids: &[&str]) -> PublishSourceSelection {
    PublishSourceSelection {
        gateway_id: gateway_id.into(),
        model_ids: ids.iter().map(|id| (*id).into()).collect(),
    }
}

fn prepare_sources(
    fixture: &Fixture,
    sources: Vec<PublishSourceSelection>,
) -> ExecutePublishRequest {
    let targets = vec![TargetKind::Workbuddy, TargetKind::Codebuddy];
    let preview = fixture
        .coordinator()
        .preview(
            &PreparePublishRequest {
                sources: sources.clone(),
                targets: targets.clone(),
            },
            &fixture.paths,
        )
        .unwrap();
    ExecutePublishRequest {
        sources,
        targets,
        expectations: preview
            .targets
            .into_iter()
            .map(|target| TargetExpectation {
                target: target.target,
                path: target.path,
                write_path: target.write_path,
                fingerprint: target.fingerprint,
            })
            .collect(),
        source_revisions: preview.source_revisions,
        model_revisions: preview.model_revisions,
        accept_conflicts: true,
    }
}

#[test]
fn publishes_distinct_models_from_both_sources_in_one_target_write() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
        fs::write(fixture.path(target), b"[{\"id\":\"external\"}]").unwrap();
    }
    let request = prepare_sources(
        &fixture,
        vec![
            selection("gateway", &["gpt-5"]),
            selection("other", &["claude"]),
        ],
    );
    assert!(
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap()
            .success
    );
    for target in &request.targets {
        let bytes = fs::read(fixture.path(*target)).unwrap();
        let document = ConfigDocument::parse(&bytes).unwrap();
        assert_eq!(document.models().len(), 3);
        let first = document
            .models()
            .iter()
            .find(|model| model["id"] == "gpt-5")
            .unwrap();
        let second = document
            .models()
            .iter()
            .find(|model| model["id"] == "claude")
            .unwrap();
        assert_eq!(first["apiKey"], "test-token");
        assert_eq!(second["apiKey"], "other-token");
        assert_eq!(second["url"], "https://other.example/v1");
        assert_eq!(fixture.store.list_backups(Some(*target)).unwrap().len(), 1);
    }
    assert!(fixture.store.pending_file_writes().unwrap().is_empty());
}

#[test]
fn rejects_duplicate_model_sources_and_duplicate_gateway_scopes_before_writes() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    let mut request = fixture.request(vec![TargetKind::Workbuddy]);
    request.sources.push(selection("other", &["gpt-5"]));
    assert!(fixture
        .coordinator()
        .preview(
            &PreparePublishRequest {
                sources: request.sources.clone(),
                targets: request.targets.clone()
            },
            &fixture.paths
        )
        .is_err());
    assert!(matches!(
        fixture.coordinator().execute(&request, &fixture.paths),
        Err(CoreError::Conflict(_))
    ));
    request.sources = vec![selection("gateway", &["gpt-5"]), selection("gateway", &[])];
    assert!(fixture
        .coordinator()
        .execute(&request, &fixture.paths)
        .is_err());
    assert!(fixture.store.list_backups(None).unwrap().is_empty());
    assert!(!fixture.path(TargetKind::Workbuddy).exists());
}

#[test]
fn chosen_source_replaces_a_same_id_once_with_its_own_credentials() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    let original = br#"[{"id":"gpt-5","url":"https://api.example.com/v1","apiKey":"test-token"} ]"#;
    for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
        fs::write(fixture.path(target), original).unwrap();
    }
    let request = prepare_sources(
        &fixture,
        vec![selection("gateway", &[]), selection("other", &["gpt-5"])],
    );
    assert!(
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap()
            .success
    );
    for target in request.targets {
        let document = ConfigDocument::parse(&fs::read(fixture.path(target)).unwrap()).unwrap();
        assert_eq!(document.models().len(), 1);
        assert_eq!(document.models()[0]["id"], "gpt-5");
        assert_eq!(document.models()[0]["apiKey"], "other-token");
    }
}

#[test]
fn a_secondary_credential_change_invalidates_the_whole_preview() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    let request = prepare_sources(
        &fixture,
        vec![
            selection("gateway", &["gpt-5"]),
            selection("other", &["claude"]),
        ],
    );
    fixture
        .store
        .save_gateway_token("other", "rotated-token")
        .unwrap();
    assert!(matches!(
        fixture.coordinator().execute(&request, &fixture.paths),
        Err(CoreError::Conflict(_))
    ));
    assert!(fixture.store.list_backups(None).unwrap().is_empty());
    assert!(!fixture.path(TargetKind::Workbuddy).exists());
}

#[test]
fn a_secondary_source_state_failure_rolls_back_every_target_and_source() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
        fs::write(fixture.path(target), b"[]").unwrap();
    }
    let request = prepare_sources(
        &fixture,
        vec![
            selection("gateway", &["gpt-5"]),
            selection("other", &["claude"]),
        ],
    );
    fixture.store.execute_test_sql("CREATE TRIGGER fail_second_source BEFORE INSERT ON gateway_source_identities WHEN NEW.gateway_id = 'other' BEGIN SELECT RAISE(FAIL, 'injected secondary source failure'); END;").unwrap();
    let result = fixture
        .coordinator()
        .execute(&request, &fixture.paths)
        .unwrap();
    assert!(!result.success);
    assert!(result.results.iter().all(|target| target.rolled_back));
    for target in request.targets {
        assert_eq!(fs::read(fixture.path(target)).unwrap(), b"[]");
    }
    assert!(!fixture.store.has_gateway_source_history().unwrap());
    assert!(fixture.store.pending_file_writes().unwrap().is_empty());
}

#[test]
fn empty_source_selection_removes_only_its_managed_models() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    let original = br#"[{"id":"gpt-5","url":"https://api.example.com/v1","apiKey":"test-token"},{"id":"claude","url":"https://other.example/v1","apiKey":"other-token"}]"#;
    for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
        fs::write(fixture.path(target), original).unwrap();
    }
    let request = prepare_sources(&fixture, vec![selection("gateway", &[])]);
    assert!(
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap()
            .success
    );
    for target in request.targets {
        assert_eq!(model_ids(&fixture.path(target)), vec!["claude"]);
    }
}

#[test]
fn changing_the_chosen_source_after_preview_is_rejected() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    let mut request = prepare_sources(
        &fixture,
        vec![
            selection("gateway", &["gpt-5"]),
            selection("other", &["claude"]),
        ],
    );
    request.sources = vec![
        selection("gateway", &[]),
        selection("other", &["gpt-5", "claude"]),
    ];
    assert!(matches!(
        fixture.coordinator().execute(&request, &fixture.paths),
        Err(CoreError::Conflict(_))
    ));
    assert!(!fixture.path(TargetKind::Workbuddy).exists());
}

#[test]
fn removal_matches_each_source_when_model_id_and_token_are_shared() {
    let fixture = Fixture::new();
    add_other_source(&fixture);
    fixture
        .store
        .save_gateway_token("other", "test-token")
        .unwrap();
    let original = br#"[{"id":"gpt-5","url":"https://api.example.com/v1","apiKey":"test-token"}]"#;
    for target in [TargetKind::Workbuddy, TargetKind::Codebuddy] {
        fs::write(fixture.path(target), original).unwrap();
    }
    let request = prepare_sources(
        &fixture,
        vec![selection("gateway", &[]), selection("other", &[])],
    );
    assert!(
        fixture
            .coordinator()
            .execute(&request, &fixture.paths)
            .unwrap()
            .success
    );
    for target in request.targets {
        assert!(model_ids(&fixture.path(target)).is_empty());
    }
}
