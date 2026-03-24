use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const PACKAGE_NAME: &str = "codex-desktop";

pub fn installed_package_version() -> String {
    match Command::new("dpkg-query")
        .args(["-W", "-f=${Version}", PACKAGE_NAME])
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if version.is_empty() {
                "unknown".to_string()
            } else {
                version
            }
        }
        _ => "unknown".to_string(),
    }
}

pub fn install_deb(path: &Path) -> Result<()> {
    anyhow::ensure!(path.exists(), "Debian package not found: {}", path.display());

    if command_exists("apt") {
        let mut command = apt_install_command(path)?;
        run_install(&mut command).context("apt install failed")?;
        return Ok(());
    }

    let mut command = dpkg_install_command(path);
    run_install(&mut command).context("dpkg -i failed")
}

pub fn pkexec_command(current_exe: &Path, deb_path: &Path) -> Command {
    let mut command = Command::new("pkexec");
    command
        .arg(current_exe)
        .arg("install-deb")
        .arg("--path")
        .arg(deb_path);
    command
}

fn run_install(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .context("Failed to execute installation command")?;
    anyhow::ensure!(status.success(), "installation command exited with {status}");
    Ok(())
}

fn apt_install_command(path: &Path) -> Result<Command> {
    let parent = path
        .parent()
        .context("Debian package path has no parent directory")?;
    let file_name = path
        .file_name()
        .context("Debian package path has no file name")?
        .to_string_lossy()
        .into_owned();

    let mut command = Command::new("apt");
    command
        .current_dir(parent)
        .arg("install")
        .arg("-y")
        .arg(format!("./{file_name}"));
    Ok(command)
}

fn dpkg_install_command(path: &Path) -> Command {
    let mut command = Command::new("dpkg");
    command.arg("-i").arg(path.as_os_str());
    command
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|entry| {
                let candidate: PathBuf = entry.join(name);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_pkexec_command_for_privileged_install() {
        let command = pkexec_command(
            Path::new("/usr/bin/codex-update-manager"),
            Path::new("/tmp/update.deb"),
        );
        let args: Vec<_> = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "/usr/bin/codex-update-manager",
                "install-deb",
                "--path",
                "/tmp/update.deb"
            ]
        );
    }

    #[test]
    fn builds_local_apt_install_command() -> Result<()> {
        let command = apt_install_command(Path::new("/tmp/build/codex.deb"))?;
        assert_eq!(command.get_program().to_string_lossy(), "apt");
        assert_eq!(
            command
                .get_args()
                .map(|value| value.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            vec!["install", "-y", "./codex.deb"]
        );
        Ok(())
    }
}
