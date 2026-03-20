pub mod mentci_user_capnp {
    include!(concat!(env!("OUT_DIR"), "/mentci_user_capnp.rs"));
}

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct UserSecretOverride {
    pub name: String,
    pub method: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NamedValue {
    pub name: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct UserLocalConfig {
    pub secrets: Vec<UserSecretOverride>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct UserProfile {
    pub env: Vec<UserSecretOverride>,
    pub shell_vars: Vec<NamedValue>,
    pub path_additions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvRequirement {
    pub name: String,
    pub default_method: String,
    pub default_path: String,
}

impl Default for UserProfile {
    fn default() -> Self {
        Self {
            env: vec![],
            shell_vars: vec![],
            path_additions: vec![],
        }
    }
}

pub fn load_local_config(path: &str) -> Result<UserLocalConfig> {
    if !Path::new(path).exists() {
        return Ok(UserLocalConfig { secrets: vec![] });
    }
    let content = fs::read_to_string(path)?;
    let config: UserLocalConfig = serde_json::from_str(&content)?;
    Ok(config)
}

pub fn load_user_profile(path: &str) -> Result<UserProfile> {
    if !Path::new(path).exists() {
        return Ok(UserProfile::default());
    }
    let content = fs::read_to_string(path)?;
    let profile: UserProfile = serde_json::from_str(&content)?;
    Ok(profile)
}

pub fn resolve_secret(method: &str, path: &str) -> Result<Option<String>> {
    match method {
        "gopass" => {
            let output = Command::new("gopass")
                .arg("show")
                .arg(path)
                .output()
                .with_context(|| format!("Failed to execute gopass for {}", path))?;
            if output.status.success() {
                let val = String::from_utf8(output.stdout)?
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                return Ok(Some(val));
            } else {
                anyhow::bail!("gopass failed: {}", String::from_utf8_lossy(&output.stderr));
            }
        }
        "env" => Ok(std::env::var(path).ok()),
        "literal" => Ok(Some(path.to_string())),
        _ => anyhow::bail!("Unknown secret method: {}", method),
    }
}

fn build_resolution_entries(
    requirements: &[EnvRequirement],
    profile: &UserProfile,
    local_config: &UserLocalConfig,
) -> Vec<UserSecretOverride> {
    let mut resolved: BTreeMap<String, UserSecretOverride> = BTreeMap::new();

    for requirement in requirements {
        resolved.insert(
            requirement.name.clone(),
            UserSecretOverride {
                name: requirement.name.clone(),
                method: requirement.default_method.clone(),
                path: requirement.default_path.clone(),
            },
        );
    }

    for entry in &profile.env {
        resolved.insert(entry.name.clone(), entry.clone());
    }

    for entry in &local_config.secrets {
        resolved.insert(entry.name.clone(), entry.clone());
    }

    resolved.into_values().collect()
}

fn expand_path_addition(addition: &str, home_dir: Option<&str>) -> String {
    if Path::new(addition).is_absolute() {
        return addition.to_string();
    }

    match home_dir {
        Some(home) if !home.is_empty() => Path::new(home).join(addition).display().to_string(),
        _ => addition.to_string(),
    }
}

pub fn realize_env(
    requirements: &[EnvRequirement],
    profile: &UserProfile,
    local_config: &UserLocalConfig,
    home_dir: Option<&str>,
    current_path: Option<&str>,
) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();

    for entry in build_resolution_entries(requirements, profile, local_config) {
        if let Some(value) = resolve_secret(&entry.method, &entry.path)? {
            out.insert(entry.name, value);
        }
    }

    for shell_var in &profile.shell_vars {
        out.insert(shell_var.name.clone(), shell_var.value.clone());
    }

    if !profile.path_additions.is_empty() {
        let mut path_parts: Vec<String> = vec![];

        for entry in profile
            .path_additions
            .iter()
            .map(|entry| expand_path_addition(entry, home_dir))
        {
            if !path_parts.iter().any(|existing| existing == &entry) {
                path_parts.push(entry);
            }
        }

        if let Some(existing_path) = current_path.filter(|value| !value.is_empty()) {
            for entry in existing_path.split(':').filter(|value| !value.is_empty()) {
                if !path_parts.iter().any(|existing| existing == entry) {
                    path_parts.push(entry.to_string());
                }
            }
        }

        out.insert("PATH".to_string(), path_parts.join(":"));
    }

    Ok(out)
}
