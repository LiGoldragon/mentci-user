use anyhow::{Context, Result};
use std::env;
use std::fs::File;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

use mentci_user::{load_local_config, mentci_user_capnp, resolve_secret};

fn default_setup_bin_path() -> Result<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![];

    if let Ok(p) = env::var("MENTCI_USER_SETUP_BIN") {
        candidates.push(PathBuf::from(p));
    }

    if let Ok(repo_root) = env::var("MENTCI_REPO_ROOT") {
        candidates.push(
            Path::new(&repo_root)
                .join("Components/mentci-user/data/setup.bin"),
        );
    }

    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("Components/mentci-user/data/setup.bin"));
    }

    candidates.push(PathBuf::from("Components/mentci-user/data/setup.bin"));
    candidates.push(PathBuf::from("setup.bin"));

    candidates
        .into_iter()
        .find(|p| p.exists())
        .context("Failed to locate setup.bin. Provide explicit path or set MENTCI_USER_SETUP_BIN")
}

fn collect_env_values(
    setup: mentci_user_capnp::user_setup_config::Reader<'_>,
) -> Result<Vec<(String, String)>> {
    let user_config_path = setup.get_user_config_path()?.to_string()?;
    let local_config =
        load_local_config(&user_config_path).unwrap_or_else(|_| mentci_user::UserLocalConfig { secrets: vec![] });

    let mut out = vec![];
    let reqs = setup.get_required_env_vars()?;

    for req in reqs.iter() {
        let name = req.get_name()?.to_string()?;
        let mut method = req.get_default_method()?.to_string()?;
        let mut path = req.get_default_path()?.to_string()?;

        if let Some(over) = local_config.secrets.iter().find(|s| s.name == name) {
            method = over.method.clone();
            path = over.path.clone();
        }

        if let Ok(Some(val)) = resolve_secret(&method, &path) {
            out.push((name, val));
        }
    }

    Ok(out)
}

fn read_setup_message(setup_bin: &Path) -> Result<capnp::message::Reader<capnp::serialize::OwnedSegments>> {
    let packed_attempt = (|| {
        let file = File::open(setup_bin)
            .with_context(|| format!("Failed to open {}", setup_bin.display()))?;
        let mut reader = BufReader::new(file);
        capnp::serialize_packed::read_message(&mut reader, capnp::message::ReaderOptions::new())
            .with_context(|| format!("Failed to read packed capnp message from {}", setup_bin.display()))
    })();

    if let Ok(message) = packed_attempt {
        if message
            .get_root::<mentci_user_capnp::user_setup_config::Reader>()
            .is_ok()
        {
            return Ok(message);
        }
    }

    let mut file = File::open(setup_bin)
        .with_context(|| format!("Failed to open {}", setup_bin.display()))?;
    capnp::serialize::read_message(&mut file, capnp::message::ReaderOptions::new())
        .with_context(|| format!("Failed to read unpacked capnp message from {}", setup_bin.display()))
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage:");
        println!("  mentci-user export-env [path_to_setup_bin]");
        println!("  mentci-user exec [path_to_setup_bin] -- <command> [args...]");
        return Ok(());
    }

    let mode = args[1].as_str();

    let setup_bin = match mode {
        "export-env" => {
            if args.len() >= 3 {
                PathBuf::from(&args[2])
            } else {
                default_setup_bin_path()?
            }
        }
        "exec" => {
            let separator_idx = args
                .iter()
                .position(|a| a == "--")
                .context("Missing '--' separator before command")?;

            if separator_idx >= 3 {
                PathBuf::from(&args[2])
            } else {
                default_setup_bin_path()?
            }
        }
        _ => anyhow::bail!("Unknown mode: {}", mode),
    };

    let message_reader = read_setup_message(&setup_bin)?;
    let setup = message_reader.get_root::<mentci_user_capnp::user_setup_config::Reader>()?;
    let env_values = collect_env_values(setup)?;

    if mode == "export-env" {
        for (name, val) in env_values {
            let escaped_val = val.replace("'", "'\\''");
            println!("export {}='{}';", name, escaped_val);
        }
        return Ok(());
    }

    let separator_idx = args
        .iter()
        .position(|a| a == "--")
        .context("Missing '--' separator before command")?;
    let cmd_args = &args[separator_idx + 1..];

    if cmd_args.is_empty() {
        anyhow::bail!("No command specified after '--'");
    }

    for (name, val) in env_values {
        env::set_var(name, val);
    }

    let mut cmd = Command::new(&cmd_args[0]);
    cmd.args(&cmd_args[1..]);

    #[cfg(unix)]
    {
        let err = cmd.exec();
        anyhow::bail!("Failed to exec {}: {}", cmd_args[0], err);
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("mentci-user-{}-{}.bin", suffix, nanos))
    }

    fn write_setup(path: &Path, packed: bool) -> Result<()> {
        let mut message = capnp::message::Builder::new_default();
        {
            let mut root = message.init_root::<mentci_user_capnp::user_setup_config::Builder>();
            root.set_text_hash("test");
            root.set_user_config_path(".mentci/user.json");
            let mut reqs = root.init_required_env_vars(1);
            let mut req = reqs.reborrow().get(0);
            req.set_name("LINKUP_API_KEY");
            req.set_default_method("literal");
            req.set_default_path("dummy");
        }

        let mut file = File::create(path)?;
        if packed {
            capnp::serialize_packed::write_message(&mut file, &message)?;
        } else {
            capnp::serialize::write_message(&mut file, &message)?;
        }
        Ok(())
    }

    #[test]
    fn reads_packed_setup_bin() {
        let path = temp_file_path("packed");
        write_setup(&path, true).unwrap();

        let reader = read_setup_message(&path).unwrap();
        let root = reader
            .get_root::<mentci_user_capnp::user_setup_config::Reader>()
            .unwrap();
        assert_eq!(root.get_text_hash().unwrap().to_string().unwrap(), "test");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn reads_unpacked_setup_bin() {
        let path = temp_file_path("unpacked");
        write_setup(&path, false).unwrap();

        let reader = read_setup_message(&path).unwrap();
        let root = reader
            .get_root::<mentci_user_capnp::user_setup_config::Reader>()
            .unwrap();
        assert_eq!(root.get_text_hash().unwrap().to_string().unwrap(), "test");

        let _ = fs::remove_file(path);
    }
}
