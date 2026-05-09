use anyhow::{Context, anyhow};
use clap::Args;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

#[derive(Args, Clone, Debug)]
pub(crate) struct CheckCommand {
    #[arg(
        long,
        value_name = "PATH",
        help = "Root directory to validate. Defaults to directory containing current binary"
    )]
    root: Option<PathBuf>,

    #[arg(
        long = "runtime-version",
        value_name = "VERSION",
        default_value = env!("CARGO_PKG_VERSION"),
        help = "Expected runtime version for client/server binaries"
    )]
    expected_runtime_version: String,
}

enum ComponentKind {
    Binary(PathBuf),
    SourceFolder(PathBuf),
}

impl CheckCommand {
    pub(crate) async fn execute(&self) -> anyhow::Result<()> {
        let root = resolve_root(&self.root)?;

        crate::ui::print_line(format!(
            "Checking runtime hierarchy at {}",
            display_path(root.as_path())
        ))?;

        let mut errors = Vec::new();

        let runtime_bin = current_runtime_binary()?;
        if self.root.is_none() {
            let runtime_parent = runtime_bin
                .parent()
                .ok_or_else(|| anyhow!("Current executable has no parent directory"))?;

            if !paths_equal(runtime_parent, root.as_path())? {
                errors.push(format!(
                    "Current binary is outside root: {}",
                    display_path(runtime_bin.as_path())
                ));
            } else {
                crate::ui::print_line(format!(
                    "[ok] germina binary: {}",
                    display_path(runtime_bin.as_path())
                ))?;
            }
        } else {
            crate::ui::print_line(format!(
                "[ok] germina binary detected: {}",
                display_path(runtime_bin.as_path())
            ))?;
        }

        check_modules_dir(&root, &mut errors)?;

        check_component(
            &root,
            "germina-client",
            &self.expected_runtime_version,
            &mut errors,
        )
        .await?;

        check_component(
            &root,
            "germina-server",
            &self.expected_runtime_version,
            &mut errors,
        )
        .await?;

        if errors.is_empty() {
            crate::ui::print_line("Check completed successfully")?;
            return Ok(());
        }

        for err in &errors {
            crate::ui::print_line(format!("[error] {err}"))?;
        }

        Err(anyhow!(
            "Hierarchy check failed ({} issue(s))",
            errors.len()
        ))
    }
}

fn resolve_root(configured_root: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let raw_root = if let Some(root) = configured_root {
        root.clone()
    } else {
        let exe = std::env::current_exe().context("Failed to determine current executable")?;
        exe.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("Current executable has no parent directory"))?
    };

    std::fs::canonicalize(&raw_root)
        .with_context(|| format!("Failed to resolve root path {}", raw_root.display()))
}

fn current_runtime_binary() -> anyhow::Result<PathBuf> {
    std::env::current_exe().context("Failed to resolve current binary path")
}

fn check_modules_dir(root: &Path, errors: &mut Vec<String>) -> anyhow::Result<()> {
    let modules = root.join("modules");
    if !modules.exists() {
        errors.push(format!(
            "Missing required folder: {}",
            display_path(modules.as_path())
        ));
        return Ok(());
    }

    if !modules.is_dir() {
        errors.push(format!(
            "Path exists but is not a directory: {}",
            display_path(modules.as_path())
        ));
        return Ok(());
    }

    crate::ui::print_line(format!(
        "[ok] modules folder: {}",
        display_path(modules.as_path())
    ))?;
    Ok(())
}

async fn check_component(
    root: &Path,
    component_name: &str,
    expected_runtime_version: &str,
    errors: &mut Vec<String>,
) -> anyhow::Result<()> {
    match resolve_component_kind(root, component_name) {
        Some(ComponentKind::Binary(path)) => {
            crate::ui::print_line(format!(
                "[ok] {component_name}: binary {}",
                display_path(path.as_path())
            ))?;
            check_binary_runtime_version(&path, component_name, expected_runtime_version, errors)
                .await?;
        }
        Some(ComponentKind::SourceFolder(path)) => {
            crate::ui::print_line(format!(
                "[ok] {component_name}: source folder {}",
                display_path(path.as_path())
            ))?;
            check_source_folder(path.as_path(), component_name, errors)?;
        }
        None => {
            errors.push(format!(
                "Missing {component_name} (expected binary or source folder under {})",
                display_path(root)
            ));
        }
    }

    Ok(())
}

fn resolve_component_kind(root: &Path, component_name: &str) -> Option<ComponentKind> {
    let direct_binary = binary_candidates(root, component_name)
        .into_iter()
        .find(|path| path.is_file());
    if let Some(path) = direct_binary {
        return Some(ComponentKind::Binary(path));
    }

    let source_folder = root.join(component_name);
    if source_folder.is_dir() {
        return Some(ComponentKind::SourceFolder(source_folder));
    }

    None
}

fn binary_candidates(root: &Path, component_name: &str) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            root.join(format!("{component_name}.exe")),
            root.join(component_name),
        ]
    }

    #[cfg(not(windows))]
    {
        vec![root.join(component_name)]
    }
}

fn check_source_folder(
    path: &Path,
    component_name: &str,
    errors: &mut Vec<String>,
) -> anyhow::Result<()> {
    let has_known_manifest = ["Cargo.toml", "CMakeLists.txt", "package.json"]
        .iter()
        .any(|name| path.join(name).is_file());

    if has_known_manifest {
        crate::ui::print_line(format!("[ok] {component_name}: source manifest detected"))?;
        return Ok(());
    }

    let modules_subdir = path.join("modules");
    if modules_subdir.is_dir() {
        crate::ui::print_line(format!("[ok] {component_name}: modules subtree detected"))?;
        return Ok(());
    }

    errors.push(format!(
        "{component_name} source folder exists but no known build manifest found in {}",
        display_path(path)
    ));
    Ok(())
}

async fn check_binary_runtime_version(
    binary_path: &Path,
    component_name: &str,
    expected_runtime_version: &str,
    errors: &mut Vec<String>,
) -> anyhow::Result<()> {
    let output = timeout(
        Duration::from_secs(5),
        Command::new(binary_path).arg("--version").output(),
    )
    .await
    .map_err(|_| anyhow!("Timeout while checking {component_name} runtime version"))?
    .with_context(|| format!("Failed to execute {} --version", display_path(binary_path)))?;

    if !output.status.success() {
        errors.push(format!(
            "{component_name} binary returned non-zero status for --version"
        ));
        return Ok(());
    }

    let mut version_text = String::new();
    version_text.push_str(&String::from_utf8_lossy(&output.stdout));
    version_text.push_str(&String::from_utf8_lossy(&output.stderr));

    if version_text.contains(expected_runtime_version) {
        crate::ui::print_line(format!(
            "[ok] {component_name}: runtime version contains {expected_runtime_version}"
        ))?;
    } else {
        errors.push(format!(
            "{component_name} runtime version mismatch: expected to find '{expected_runtime_version}', got '{}'",
            version_text.trim()
        ));
    }

    Ok(())
}

fn paths_equal(a: &Path, b: &Path) -> anyhow::Result<bool> {
    let a_canon = std::fs::canonicalize(a)
        .with_context(|| format!("Failed to canonicalize path {}", display_path(a)))?;
    let b_canon = std::fs::canonicalize(b)
        .with_context(|| format!("Failed to canonicalize path {}", display_path(b)))?;
    Ok(a_canon == b_canon)
}

fn display_path(path: &Path) -> String {
    canonicalize_for_display(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn canonicalize_for_display(path: &Path) -> Option<PathBuf> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Some(canonical);
    }

    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    let cwd = std::env::current_dir().ok()?;
    Some(cwd.join(path))
}
