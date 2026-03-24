use crate::{
    config::{RuntimeConfig, RuntimePaths},
    state::{ArtifactPaths, PersistedState, UpdateStatus},
};
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio::process::Command;
use tracing::info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildArtifacts {
    pub workspace_dir: PathBuf,
    pub deb_path: PathBuf,
}

pub async fn build_update(
    config: &RuntimeConfig,
    state: &mut PersistedState,
    paths: &RuntimePaths,
    candidate_version: &str,
    dmg_path: &Path,
) -> Result<BuildArtifacts> {
    let workspace_dir = config.workspace_root.join("workspaces").join(candidate_version);
    let bundle_dir = workspace_dir.join("builder");
    let dist_dir = workspace_dir.join("dist");
    let app_dir = workspace_dir.join("codex-app");
    let logs_dir = workspace_dir.join("logs");
    let install_log = logs_dir.join("install.log");
    let build_log = logs_dir.join("build-deb.log");

    if workspace_dir.exists() {
        fs::remove_dir_all(&workspace_dir)
            .with_context(|| format!("Failed to remove {}", workspace_dir.display()))?;
    }

    fs::create_dir_all(&logs_dir)
        .with_context(|| format!("Failed to create {}", logs_dir.display()))?;

    state.status = UpdateStatus::PreparingWorkspace;
    state.artifact_paths.workspace_dir = Some(workspace_dir.clone());
    state.save(&paths.state_file)?;

    copy_builder_bundle(&config.builder_bundle_root, &bundle_dir)?;

    state.status = UpdateStatus::PatchingApp;
    state.save(&paths.state_file)?;
    run_and_log(
        Command::new(bundle_dir.join("install.sh"))
            .arg(dmg_path)
            .env("CODEX_INSTALL_DIR", &app_dir)
            .current_dir(&bundle_dir),
        &install_log,
    )
    .await
    .context("install.sh failed during local rebuild")?;

    state.status = UpdateStatus::BuildingDeb;
    state.save(&paths.state_file)?;
    run_and_log(
        Command::new(bundle_dir.join("scripts/build-deb.sh"))
            .env("PACKAGE_VERSION", candidate_version)
            .env("APP_DIR_OVERRIDE", &app_dir)
            .env("DIST_DIR_OVERRIDE", &dist_dir)
            .env("UPDATER_BINARY_SOURCE", std::env::current_exe()?)
            .env(
                "UPDATER_SERVICE_SOURCE",
                bundle_dir.join("packaging/linux/codex-update-manager.service"),
            )
            .current_dir(&bundle_dir),
        &build_log,
    )
    .await
    .context("build-deb.sh failed during local rebuild")?;

    let deb_path = find_deb_in(&dist_dir)?;
    state.status = UpdateStatus::ReadyToInstall;
    state.artifact_paths = ArtifactPaths {
        dmg_path: Some(dmg_path.to_path_buf()),
        workspace_dir: Some(workspace_dir.clone()),
        deb_path: Some(deb_path.clone()),
    };
    state.save(&paths.state_file)?;
    info!(candidate_version, deb = %deb_path.display(), "local update build ready");

    Ok(BuildArtifacts {
        workspace_dir,
        deb_path,
    })
}

fn copy_builder_bundle(source_root: &Path, destination_root: &Path) -> Result<()> {
    copy_path(&source_root.join("install.sh"), &destination_root.join("install.sh"))?;
    copy_path(
        &source_root.join("scripts/build-deb.sh"),
        &destination_root.join("scripts/build-deb.sh"),
    )?;
    copy_dir_recursive(
        &source_root.join("packaging/linux"),
        &destination_root.join("packaging/linux"),
    )?;
    copy_path(
        &source_root.join("assets/codex.png"),
        &destination_root.join("assets/codex.png"),
    )?;
    Ok(())
}

fn copy_path(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("Destination path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create {}", parent.display()))?;
    fs::copy(source, destination)
        .with_context(|| format!("Failed to copy {} to {}", source.display(), destination.display()))?;
    let metadata = fs::metadata(source)
        .with_context(|| format!("Failed to stat {}", source.display()))?;
    fs::set_permissions(destination, metadata.permissions())
        .with_context(|| format!("Failed to set permissions on {}", destination.display()))?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("Failed to create {}", destination.display()))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("Failed to read {}", source.display()))?
    {
        let entry = entry?;
        let entry_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry_path, &destination_path)?;
        } else {
            copy_path(&entry_path, &destination_path)?;
        }
    }

    Ok(())
}

fn find_deb_in(dist_dir: &Path) -> Result<PathBuf> {
    for entry in fs::read_dir(dist_dir)
        .with_context(|| format!("Failed to read {}", dist_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("deb") {
            return Ok(path);
        }
    }

    anyhow::bail!("No .deb package found in {}", dist_dir.display())
}

async fn run_and_log(command: &mut Command, log_path: &Path) -> Result<()> {
    let output = command
        .output()
        .await
        .context("Failed to spawn external command")?;

    let mut combined = Vec::new();
    combined.extend_from_slice(&output.stdout);
    combined.extend_from_slice(&output.stderr);
    fs::write(log_path, &combined)
        .with_context(|| format!("Failed to write {}", log_path.display()))?;

    if !output.status.success() {
        anyhow::bail!(
            "Command failed with status {:?}; see {}",
            output.status.code(),
            log_path.display()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RuntimePaths;
    use anyhow::Result;
    use tempfile::tempdir;

    #[tokio::test]
    async fn builds_update_with_fake_bundle() -> Result<()> {
        let temp = tempdir()?;
        let bundle_root = temp.path().join("bundle");
        let state_root = temp.path().join("state");
        let cache_root = temp.path().join("cache");
        fs::create_dir_all(bundle_root.join("scripts"))?;
        fs::create_dir_all(bundle_root.join("packaging/linux"))?;
        fs::create_dir_all(bundle_root.join("assets"))?;
        fs::write(bundle_root.join("assets/codex.png"), b"png")?;
        fs::write(bundle_root.join("packaging/linux/control"), "Package: codex")?;
        fs::write(
            bundle_root.join("packaging/linux/codex-desktop.desktop"),
            "[Desktop Entry]",
        )?;
        fs::write(
            bundle_root.join("packaging/linux/codex-update-manager.service"),
            "[Unit]\nDescription=Codex Update Manager\n",
        )?;
        fs::write(
            bundle_root.join("install.sh"),
            r#"#!/bin/bash
set -euo pipefail
mkdir -p "${CODEX_INSTALL_DIR}"
echo launcher > "${CODEX_INSTALL_DIR}/start.sh"
chmod +x "${CODEX_INSTALL_DIR}/start.sh"
"#,
        )?;
        fs::write(
            bundle_root.join("scripts/build-deb.sh"),
            r#"#!/bin/bash
set -euo pipefail
mkdir -p "${DIST_DIR_OVERRIDE}"
touch "${DIST_DIR_OVERRIDE}/codex-desktop_${PACKAGE_VERSION}_amd64.deb"
"#,
        )?;
        let install_meta = fs::metadata(bundle_root.join("install.sh"))?;
        fs::set_permissions(bundle_root.join("install.sh"), install_meta.permissions())?;
        let build_meta = fs::metadata(bundle_root.join("scripts/build-deb.sh"))?;
        fs::set_permissions(
            bundle_root.join("scripts/build-deb.sh"),
            build_meta.permissions(),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                bundle_root.join("install.sh"),
                fs::Permissions::from_mode(0o755),
            )?;
            fs::set_permissions(
                bundle_root.join("scripts/build-deb.sh"),
                fs::Permissions::from_mode(0o755),
            )?;
        }

        let paths = RuntimePaths {
            config_file: temp.path().join("config/config.toml"),
            state_file: state_root.join("state.json"),
            log_file: state_root.join("service.log"),
            cache_dir: cache_root.clone(),
            state_dir: state_root.clone(),
            config_dir: temp.path().join("config"),
        };
        paths.ensure_dirs()?;

        let config = RuntimeConfig {
            dmg_url: "https://example.com/Codex.dmg".to_string(),
            initial_check_delay_seconds: 30,
            check_interval_hours: 6,
            auto_install_on_app_exit: true,
            notifications: true,
            workspace_root: cache_root,
            builder_bundle_root: bundle_root,
            app_executable_path: PathBuf::from("/opt/codex-desktop/electron"),
        };
        let dmg_path = temp.path().join("Codex.dmg");
        fs::write(&dmg_path, b"dmg")?;

        let mut state = PersistedState::new(true);
        let artifacts =
            build_update(&config, &mut state, &paths, "2026.03.24+abcd1234", &dmg_path).await?;
        assert_eq!(state.status, UpdateStatus::ReadyToInstall);
        assert!(artifacts.workspace_dir.exists());
        assert!(artifacts.deb_path.exists());
        assert_eq!(
            artifacts.deb_path.file_name().and_then(|value| value.to_str()),
            Some("codex-desktop_2026.03.24+abcd1234_amd64.deb")
        );
        Ok(())
    }
}
