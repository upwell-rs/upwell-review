use std::collections::HashMap;

use tempfile::TempDir;

use super::*;

fn no_env(_: &str) -> Option<String> {
    None
}

fn requested(provider: &str, model: &str, effort: &str) -> Requested {
    let some = |value: &str| Some(value.to_owned());

    Requested {
        provider: some(provider),
        model: some(model),
        effort: some(effort),
    }
}

fn run(provider: ProviderKind, model: &str, effort: &str) -> StageRun {
    StageRun {
        provider,
        model: model.to_owned(),
        effort: effort.to_owned(),
    }
}

#[test]
fn defaults_apply_and_verification_follows_discovery() {
    let plan =
        plan(Requested::default(), Requested::default(), None, no_env).expect("defaults are valid");

    assert_eq!(
        plan.discovery,
        run(ProviderKind::Codex, "gpt-6.1-terra", "medium")
    );
    assert_eq!(plan.verify, plan.discovery);
}

#[test]
fn the_request_beats_the_mix_which_beats_the_environment() {
    let mixes = Mixes::load(None).expect("built-ins");
    let env: HashMap<&str, &str> = HashMap::from([
        ("UPWELL_VERIFY_MODEL", "env-model"),
        ("UPWELL_VERIFY_EFFORT", "low"),
    ]);
    let lookup = |name: &str| env.get(name).map(|value| (*value).to_owned());
    let verify = Requested {
        model: Some("flag-model".to_owned()),
        ..Requested::default()
    };
    let mixed = plan(
        Requested::default(),
        verify,
        Some(mixes.get("risky").expect("risky")),
        lookup,
    )
    .expect("valid");
    let unmixed = plan(
        requested("gemini", "g", ""),
        Requested::default(),
        None,
        lookup,
    )
    .expect("valid");

    assert_eq!(
        mixed.discovery,
        run(ProviderKind::Copilot, "claude-opus-5.5", "high")
    );
    assert_eq!(
        mixed.verify,
        run(ProviderKind::Codex, "flag-model", "medium")
    );
    assert_eq!(unmixed.discovery, run(ProviderKind::Gemini, "g", "medium"));
    assert_eq!(
        unmixed.verify,
        run(ProviderKind::Gemini, "env-model", "low")
    );
}

#[test]
fn an_unknown_provider_is_refused() {
    let error = plan(
        Requested::default(),
        requested("claude", "m", "e"),
        None,
        no_env,
    )
    .expect_err("unknown");

    assert_eq!(
        error.to_string(),
        "unknown provider 'claude'; expected codex, gemini or copilot"
    );
}

#[test]
fn the_file_overrides_and_extends_the_built_in_mixes() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("providers.toml");
    let text = r#"
        [mix.cheap]
        discovery = { provider = "gemini", model = "flash" }
        verify = { provider = "codex", model = "mini", effort = "low" }

        [mix.deep]
        discovery = { provider = "codex", model = "big", effort = "xhigh" }
        verify = { provider = "copilot", model = "claude" }
    "#;

    std::fs::write(&path, text).expect("written");

    let mixes = Mixes::load(Some(&path)).expect("valid file");

    assert_eq!(
        mixes.get("cheap").expect("cheap").discovery,
        run(ProviderKind::Gemini, "flash", "medium")
    );
    assert_eq!(
        mixes.get("deep").expect("deep").verify,
        run(ProviderKind::Copilot, "claude", "medium")
    );
    assert!(mixes.get("risky").is_ok());
    assert_eq!(mixes.source(), Some(path.as_path()));
    assert_eq!(
        mixes.get("nope").expect_err("unknown").to_string(),
        "unknown mix 'nope'; known mixes: cheap, deep, risky"
    );
}

#[test]
fn a_bad_file_names_itself() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("providers.toml");

    std::fs::write(
        &path,
        "[mix.x]\ndiscovery = { provider = \"claude\", model = \"m\" }\n",
    )
    .expect("written");

    let error = Mixes::load(Some(&path)).expect_err("unknown provider");

    assert!(matches!(error, ConfigError::Parse { .. }));
    assert!(
        error.to_string().contains("unknown provider 'claude'"),
        "{error}"
    );
}

#[test]
fn the_default_file_is_used_only_when_it_exists() {
    let dir = TempDir::new().expect("temp dir");
    let config = dir.path().to_string_lossy().into_owned();
    let env = |name: &str| (name == "XDG_CONFIG_HOME").then(|| config.to_owned());
    let explicit = PathBuf::from("/explicit.toml");

    assert_eq!(providers_path(None, env), None);
    assert_eq!(
        providers_path(Some(explicit.to_owned()), env),
        Some(explicit)
    );

    std::fs::create_dir(dir.path().join("upwell-review")).expect("dir");
    std::fs::write(dir.path().join("upwell-review/providers.toml"), "").expect("written");

    assert_eq!(
        providers_path(None, env),
        Some(dir.path().join("upwell-review/providers.toml"))
    );
}
