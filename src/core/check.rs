use anyhow::{Context, anyhow};
use clap::Args;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::time::{Duration, timeout};

#[derive(Args, Clone, Debug)]
pub(crate) struct CheckCommand {
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
    pub(crate) async fn execute(&self, root: &Path) -> anyhow::Result<()> {
        crate::ui::print_line(format!(
            "Checking runtime hierarchy at {}",
            display_path(root)
        ))?;

        let mut errors = Vec::new();

        let runtime_bin = current_runtime_binary()?;
        let runtime_parent = runtime_bin
            .parent()
            .ok_or_else(|| anyhow!("Current executable has no parent directory"))?;

        if !paths_equal(runtime_parent, root)? {
            crate::ui::print_line(format!(
                "[ok] germina binary detected: {}",
                display_path(runtime_bin.as_path())
            ))?;
        } else {
            crate::ui::print_line(format!(
                "[ok] germina binary: {}",
                display_path(runtime_bin.as_path())
            ))?;
        }

        check_modules_dir(&root, &mut errors)?;

        let cmake_sources = discover_cmake_source_components(root);
        if !cmake_sources.is_empty() {
            check_native_toolchain(&cmake_sources, &mut errors).await?;
        }

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

fn discover_cmake_source_components(root: &Path) -> Vec<&'static str> {
    ["germina-client", "germina-server"]
        .iter()
        .copied()
        .filter(|component| root.join(component).join("CMakeLists.txt").is_file())
        .collect()
}

async fn check_native_toolchain(
    cmake_components: &[&str],
    errors: &mut Vec<String>,
) -> anyhow::Result<()> {
    crate::ui::print_line(format!(
        "Checking native toolchain for CMake components: {}",
        cmake_components.join(", ")
    ))?;

    if let Some(version) = probe_command_version("cmake", &["--version"]).await? {
        crate::ui::print_line(format!("[ok] cmake: {version}"))?;
    } else {
        errors.push("Missing required tool 'cmake' in PATH".to_string());
    }

    if let Some(version) = probe_command_version("cargo", &["--version"]).await? {
        crate::ui::print_line(format!("[ok] cargo: {version}"))?;
    } else {
        errors.push("Missing required tool 'cargo' in PATH".to_string());
    }

    if let Some(version) = probe_command_version("ninja", &["--version"]).await? {
        crate::ui::print_line(format!("[ok] ninja: {version}"))?;
    } else {
        errors.push("Missing required tool 'ninja' in PATH".to_string());
    }

    check_windows_long_paths(errors).await?;

    if let Some(version) = probe_command_version("clang", &["--version"]).await? {
        crate::ui::print_line(format!("[ok] clang: {version}"))?;
    } else {
        errors.push("Missing required tool 'clang' in PATH".to_string());
    }

    if let Some(version) = probe_command_version("clang++", &["--version"]).await? {
        crate::ui::print_line(format!("[ok] clang++: {version}"))?;
    } else {
        errors.push("Missing required tool 'clang++' in PATH".to_string());
    }

    #[cfg(windows)]
    let compiler_candidates: &[(&str, &[&str])] = &[
        ("cl", &["/?"]),
        ("clang", &["--version"]),
        ("gcc", &["--version"]),
    ];

    #[cfg(not(windows))]
    let compiler_candidates: &[(&str, &[&str])] = &[
        ("cc", &["--version"]),
        ("clang", &["--version"]),
        ("gcc", &["--version"]),
    ];

    let mut detected = None;
    for (tool, args) in compiler_candidates {
        if let Some(version) = probe_command_version(tool, args).await? {
            detected = Some((*tool, version));
            break;
        }
    }

    if let Some((tool, version)) = detected {
        crate::ui::print_line(format!("[ok] C/C++ compiler: {tool} ({version})"))?;
    } else {
        #[cfg(windows)]
        let expected = "cl, clang, gcc";
        #[cfg(not(windows))]
        let expected = "cc, clang, gcc";
        errors.push(format!(
            "No C/C++ compiler found in PATH (expected one of: {expected})"
        ));
    }

    Ok(())
}

#[cfg(windows)]
async fn check_windows_long_paths(errors: &mut Vec<String>) -> anyhow::Result<()> {
    let output = timeout(
        Duration::from_secs(3),
        Command::new("reg")
            .args([
                "query",
                "HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem",
                "/v",
                "LongPathsEnabled",
            ])
            .output(),
    )
    .await
    .map_err(|_| anyhow!("Timeout while checking Windows Long Paths setting"))?
    .context("Failed to query Windows Long Paths registry setting")?;

    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    if !output.status.success() {
        errors.push(
            "Failed to read Windows Long Paths setting (LongPathsEnabled) from registry"
                .to_string(),
        );
        return Ok(());
    }

    if text.contains("LongPathsEnabled") && (text.contains("0x1") || text.contains("0x00000001")) {
        crate::ui::print_line("[ok] windows long paths: enabled")?;
    } else {
        errors.push(
            "Windows long paths are disabled. Enable LongPathsEnabled=1 in HKLM\\SYSTEM\\CurrentControlSet\\Control\\FileSystem"
                .to_string(),
        );
    }

    Ok(())
}

#[cfg(not(windows))]
async fn check_windows_long_paths(_errors: &mut Vec<String>) -> anyhow::Result<()> {
    Ok(())
}

async fn probe_command_version(command: &str, args: &[&str]) -> anyhow::Result<Option<String>> {
    match timeout(
        Duration::from_secs(3),
        Command::new(command).args(args).output(),
    )
    .await
    {
        Ok(Ok(output)) => {
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            text.push_str(&String::from_utf8_lossy(&output.stderr));

            let version_line = text
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(|line| line.to_string())
                .unwrap_or_else(|| "version unavailable".to_string());

            Ok(Some(version_line))
        }
        Ok(Err(err)) if err.kind() == ErrorKind::NotFound => Ok(None),
        Ok(Err(err)) => Err(anyhow!(
            "Failed to probe command '{}' ({}): {}",
            command,
            args.join(" "),
            err
        )),
        Err(_) => Ok(None),
    }
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
