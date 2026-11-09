use super::*;

#[test]
fn every_embedded_schema_is_json() {
    let assets = Assets::default();

    for asset in Asset::ALL
        .into_iter()
        .filter(|asset| asset.file_name().ends_with(".json"))
    {
        assets.schema(asset).expect("embedded schema parses");
    }
}

#[test]
fn a_file_in_the_prompts_dir_overrides_only_that_asset() {
    let dir = tempfile::tempdir().expect("temp dir");

    fs::write(dir.path().join("verify-prompt.md"), "custom verify rules")
        .expect("override written");

    let assets = Assets::new(Some(dir.path().to_owned())).expect("dir exists");

    assert_eq!(
        assets.text(Asset::VerifyPrompt).expect("override read"),
        "custom verify rules"
    );
    assert_eq!(
        assets.text(Asset::ReviewPrompt).expect("embedded"),
        Asset::ReviewPrompt.embedded()
    );
}

#[test]
fn a_missing_prompts_dir_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let missing = dir.path().join("nope");

    assert!(matches!(
        Assets::new(Some(missing)),
        Err(AssetError::MissingDir(_))
    ));
}

#[test]
fn materialize_writes_the_effective_copy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = Assets::default()
        .materialize(Asset::DiscoverySchema, dir.path())
        .expect("written");

    assert_eq!(
        fs::read_to_string(path).expect("readable"),
        Asset::DiscoverySchema.embedded()
    );
}

#[test]
fn schema_names_map_to_schema_assets() {
    assert_eq!(
        Asset::schema_named("discovery"),
        Some(Asset::DiscoverySchema)
    );
    assert_eq!(Asset::schema_named("verify"), Some(Asset::VerifySchema));
    assert_eq!(Asset::schema_named("review"), Some(Asset::ReviewSchema));
    assert_eq!(Asset::schema_named("other"), None);
}
