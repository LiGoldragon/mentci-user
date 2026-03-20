use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use mentci_user::{
    load_local_config, load_user_profile, realize_env, resolve_secret, EnvRequirement,
};

fn temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("mentci-user-{name}-{nanos}"))
}

fn write_setup(path: &Path) {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut root = message.init_root::<mentci_user::mentci_user_capnp::user_setup_config::Builder>();
        root.set_text_hash("test");
        root.set_user_config_path("missing-local.json");
        let mut reqs = root.init_required_env_vars(2);
        {
            let mut req = reqs.reborrow().get(0);
            req.set_name("GEMINI_API_KEY");
            req.set_default_method("literal");
            req.set_default_path("setup-gemini");
        }
        {
            let mut req = reqs.get(1);
            req.set_name("OPENAI_API_KEY");
            req.set_default_method("literal");
            req.set_default_path("setup-openai");
        }
    }

    let mut file = fs::File::create(path).unwrap();
    capnp::serialize::write_message(&mut file, &message).unwrap();
}

#[test]
fn realize_env_prefers_local_overrides_then_profile_then_setup() {
    let profile_path = temp_path("profile.json");
    let local_path = temp_path("local.json");

    fs::write(
        &profile_path,
        r#"{
  "env": [
    {"name": "GEMINI_API_KEY", "method": "literal", "path": "profile-gemini"},
    {"name": "OPENAI_API_KEY", "method": "literal", "path": "profile-openai"},
    {"name": "LINKUP_API_KEY", "method": "literal", "path": "profile-linkup"}
  ],
  "shellVars": [
    {"name": "EDITOR", "value": "hx"},
    {"name": "MENTCI_USER_PROFILE_ACTIVE", "value": "1"}
  ],
  "pathAdditions": [".local/bin"]
}"#,
    )
    .unwrap();

    fs::write(
        &local_path,
        r#"{
  "secrets": [
    {"name": "OPENAI_API_KEY", "method": "literal", "path": "local-openai"}
  ]
}"#,
    )
    .unwrap();

    let profile = load_user_profile(profile_path.to_str().unwrap()).unwrap();
    let local = load_local_config(local_path.to_str().unwrap()).unwrap();
    let requirements = vec![
        EnvRequirement {
            name: "GEMINI_API_KEY".into(),
            default_method: "literal".into(),
            default_path: "setup-gemini".into(),
        },
        EnvRequirement {
            name: "OPENAI_API_KEY".into(),
            default_method: "literal".into(),
            default_path: "setup-openai".into(),
        },
    ];

    let realized = realize_env(&requirements, &profile, &local, Some("/tmp/home"), Some("/usr/bin")).unwrap();

    assert_eq!(realized.get("GEMINI_API_KEY"), Some(&"profile-gemini".to_string()));
    assert_eq!(realized.get("OPENAI_API_KEY"), Some(&"local-openai".to_string()));
    assert_eq!(realized.get("LINKUP_API_KEY"), Some(&"profile-linkup".to_string()));
    assert_eq!(realized.get("EDITOR"), Some(&"hx".to_string()));
    assert_eq!(realized.get("MENTCI_USER_PROFILE_ACTIVE"), Some(&"1".to_string()));
    assert_eq!(
        realized.get("PATH"),
        Some(&"/tmp/home/.local/bin:/usr/bin".to_string())
    );
}

#[test]
fn does_not_duplicate_path_entries_when_already_present() {
    let profile_path = temp_path("dedupe-profile.json");

    fs::write(
        &profile_path,
        r#"{
  "env": [],
  "shellVars": [],
  "pathAdditions": [".local/bin"]
}"#,
    )
    .unwrap();

    let profile = load_user_profile(profile_path.to_str().unwrap()).unwrap();
    let local = load_local_config("missing-local.json").unwrap();
    let realized = realize_env(&[], &profile, &local, Some("/tmp/home"), Some("/tmp/home/.local/bin:/usr/bin")).unwrap();

    assert_eq!(
        realized.get("PATH"),
        Some(&"/tmp/home/.local/bin:/usr/bin".to_string())
    );
}

#[test]
fn exec_mode_applies_profile_env_to_child_process() {
    let setup_path = temp_path("setup.bin");
    let profile_path = temp_path("profile.json");
    write_setup(&setup_path);

    fs::write(
        &profile_path,
        r#"{
  "env": [
    {"name": "GEMINI_API_KEY", "method": "literal", "path": "profile-gemini"}
  ],
  "shellVars": [
    {"name": "EDITOR", "value": "hx"},
    {"name": "MENTCI_USER_PROFILE_ACTIVE", "value": "1"}
  ],
  "pathAdditions": []
}"#,
    )
    .unwrap();

    let bin = env!("CARGO_BIN_EXE_mentci-user");
    let output = Command::new(bin)
        .env("MENTCI_USER_PROFILE_JSON", &profile_path)
        .arg("exec")
        .arg(&setup_path)
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg("printf '%s|%s|%s' \"$EDITOR\" \"$MENTCI_USER_PROFILE_ACTIVE\" \"$GEMINI_API_KEY\"")
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "hx|1|profile-gemini");
}

#[test]
fn existing_secret_resolution_modes_still_work() {
    std::env::set_var("MENTCI_USER_TEST_SECRET", "secret-from-env");
    assert_eq!(resolve_secret("env", "MENTCI_USER_TEST_SECRET").unwrap(), Some("secret-from-env".into()));
    assert_eq!(resolve_secret("literal", "literal-secret").unwrap(), Some("literal-secret".into()));
}
