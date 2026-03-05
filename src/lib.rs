pub mod mentci_user_capnp {
    include!(concat!(env!("OUT_DIR"), "/mentci_user_capnp.rs"));
}

use std::fs;
use std::process::Command;
use std::path::Path;
use anyhow::{Context, Result};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct UserSecretOverride {
    pub name: String,
    pub method: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UserLocalConfig {
    pub secrets: Vec<UserSecretOverride>,
}

pub fn load_local_config(path: &str) -> Result<UserLocalConfig> {
    if !Path::new(path).exists() {
        return Ok(UserLocalConfig { secrets: vec![] });
    }
    let content = fs::read_to_string(path)?;
    let config: UserLocalConfig = serde_json::from_str(&content)?;
    Ok(config)
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
        "env" => {
            Ok(std::env::var(path).ok())
        }
        "literal" => {
            Ok(Some(path.to_string()))
        }
        _ => anyhow::bail!("Unknown secret method: {}", method),
    }
}
