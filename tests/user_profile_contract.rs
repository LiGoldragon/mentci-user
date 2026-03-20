use mentci_user::load_user_profile;

#[test]
fn parses_component_local_user_profile() {
    let profile = load_user_profile("data/user-profile.json").expect("profile should parse");

    assert!(profile.env.len() >= 3);
    assert!(profile.shell_vars.iter().any(|entry| entry.name == "MENTCI_USER_PROFILE_ACTIVE"));
    assert!(profile.path_additions.iter().any(|entry| entry == ".local/bin"));
}
