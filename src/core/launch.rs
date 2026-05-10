use anyhow::{Context, anyhow};
use serde_json::{Map, Value, json};
use std::fs;
use std::path::Path;

const LAUNCH_FILE_VERSION: &str = "0.2.0";
const TASKS_FILE_VERSION: &str = "2.0.0";

#[derive(Clone, Copy)]
pub(crate) enum LaunchProfile {
    Release,
    Debug,
}

#[derive(Clone, Copy)]
enum ComponentKind {
    Source,
    Binary,
    Missing,
}

#[derive(Clone, Copy)]
struct ComponentSpec {
    key: &'static str,
    component_dir: &'static str,
    binary_name: &'static str,
}

impl ComponentSpec {
    fn title(&self) -> &'static str {
        match self.key {
            "client" => "Client",
            "server" => "Server",
            _ => "Component",
        }
    }
}

const COMPONENTS: [ComponentSpec; 2] = [
    ComponentSpec {
        key: "client",
        component_dir: "germina-client",
        binary_name: "germina-client",
    },
    ComponentSpec {
        key: "server",
        component_dir: "germina-server",
        binary_name: "germina-server",
    },
];

pub(crate) fn ensure_launch_configs(root: &Path, cli_endpoint: &str) -> anyhow::Result<()> {
    let vscode_dir = root.join(".vscode");
    if !vscode_dir.is_dir() {
        return Ok(());
    }

    let launch_path = vscode_dir.join("launch.json");
    let tasks_path = vscode_dir.join("tasks.json");

    let component_states: Vec<(ComponentSpec, ComponentKind)> = COMPONENTS
        .iter()
        .map(|component| (*component, detect_component_kind(root, component)))
        .collect();
    let has_workspace_cmake = root.join("CMakeLists.txt").is_file();

    let active_profile = detect_launch_profile();

    let mut launch_json = load_launch_json(launch_path.as_path())?;
    let root_obj = launch_json
        .as_object_mut()
        .ok_or_else(|| anyhow!("launch.json root must be an object"))?;

    root_obj
        .entry("version".to_string())
        .or_insert_with(|| Value::String(LAUNCH_FILE_VERSION.to_string()));

    let configurations = root_obj
        .entry("configurations".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let configs = configurations
        .as_array_mut()
        .ok_or_else(|| anyhow!("launch.json field 'configurations' must be an array"))?;

    remove_managed_configurations(configs);

    for (component, kind) in &component_states {
        match kind {
            ComponentKind::Source => {
                if has_workspace_cmake {
                    configs.push(workspace_source_launch_config(
                        component,
                        active_profile,
                        cli_endpoint,
                    ));
                } else {
                    configs.push(source_launch_config(
                        component,
                        active_profile,
                        cli_endpoint,
                    ));
                }
            }
            ComponentKind::Binary => {
                if !has_workspace_cmake {
                    configs.push(binary_attach_config(component, active_profile, root));
                }
            }
            ComponentKind::Missing => {}
        }
    }

    let serialized = serde_json::to_string_pretty(&launch_json)
        .context("Failed to serialize launch.json content")?;
    fs::write(&launch_path, format!("{serialized}\n"))
        .with_context(|| format!("Failed to write {}", launch_path.display()))?;

    sync_tasks_json(
        tasks_path.as_path(),
        &component_states,
        has_workspace_cmake,
        active_profile,
    )?;

    Ok(())
}

fn detect_launch_profile() -> LaunchProfile {
    match option_env!("GERMINA_BUILD_PROFILE") {
        Some(profile) if profile.eq_ignore_ascii_case("release") => LaunchProfile::Release,
        _ => LaunchProfile::Debug,
    }
}

fn sync_tasks_json(
    path: &Path,
    component_states: &[(ComponentSpec, ComponentKind)],
    has_workspace_cmake: bool,
    active_profile: LaunchProfile,
) -> anyhow::Result<()> {
    let mut tasks_json = load_tasks_json(path)?;
    let root_obj = tasks_json
        .as_object_mut()
        .ok_or_else(|| anyhow!("tasks.json root must be an object"))?;

    root_obj
        .entry("version".to_string())
        .or_insert_with(|| Value::String(TASKS_FILE_VERSION.to_string()));

    let tasks_value = root_obj
        .entry("tasks".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let tasks = tasks_value
        .as_array_mut()
        .ok_or_else(|| anyhow!("tasks.json field 'tasks' must be an array"))?;

    remove_managed_tasks(tasks);

    if has_workspace_cmake {
        tasks.push(configure_workspace_task(active_profile));
        for (component, kind) in component_states {
            if matches!(kind, ComponentKind::Source) {
                tasks.push(build_workspace_component_task(component, active_profile));
            }
        }
        tasks.push(build_workspace_task(active_profile));
    } else {
        for (component, kind) in component_states {
            if matches!(kind, ComponentKind::Source) {
                tasks.push(configure_task(component, active_profile));
                tasks.push(build_task(component, active_profile));
            }
        }
    }

    let serialized = serde_json::to_string_pretty(&tasks_json)
        .context("Failed to serialize tasks.json content")?;
    fs::write(path, format!("{serialized}\n"))
        .with_context(|| format!("Failed to write {}", path.display()))?;

    Ok(())
}

fn load_launch_json(path: &Path) -> anyhow::Result<Value> {
    if !path.is_file() {
        return Ok(json!({
            "version": LAUNCH_FILE_VERSION,
            "configurations": []
        }));
    }

    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(json!({
            "version": LAUNCH_FILE_VERSION,
            "configurations": []
        }));
    }

    serde_json::from_str::<Value>(&raw)
        .with_context(|| format!("Failed to parse {} as JSON", path.display()))
}

fn load_tasks_json(path: &Path) -> anyhow::Result<Value> {
    if !path.is_file() {
        return Ok(json!({
            "version": TASKS_FILE_VERSION,
            "tasks": []
        }));
    }

    let raw =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(json!({
            "version": TASKS_FILE_VERSION,
            "tasks": []
        }));
    }

    serde_json::from_str::<Value>(&raw)
        .with_context(|| format!("Failed to parse {} as JSON", path.display()))
}

fn config_name(value: &Value) -> anyhow::Result<&str> {
    value
        .as_object()
        .and_then(|obj| obj.get("name"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("launch configuration misses string field 'name'"))
}

fn task_label(value: &Value) -> Option<&str> {
    value
        .as_object()
        .and_then(|obj| obj.get("label"))
        .and_then(Value::as_str)
}

fn remove_managed_configurations(configs: &mut Vec<Value>) {
    configs.retain(|value| {
        config_name(value)
            .map(|name| !is_managed_launch_name(name))
            .unwrap_or(true)
    });
}

fn remove_managed_tasks(tasks: &mut Vec<Value>) {
    tasks.retain(|value| {
        task_label(value)
            .map(|label| !is_managed_task_label(label))
            .unwrap_or(true)
    });
}

fn is_managed_launch_name(name: &str) -> bool {
    name.starts_with("Germina Client (") || name.starts_with("Germina Server (")
}

fn is_managed_task_label(label: &str) -> bool {
    label.starts_with("Germina Configure Client (")
        || label.starts_with("Germina Configure Server (")
        || label.starts_with("Germina Configure Workspace (")
        || label.starts_with("Germina Build Client (")
        || label.starts_with("Germina Build Server (")
        || label.starts_with("Germina Build Workspace (")
}

fn detect_component_kind(root: &Path, component: &ComponentSpec) -> ComponentKind {
    let source_path = root.join(component.component_dir);
    if source_path.is_dir() && source_path.join("CMakeLists.txt").is_file() {
        return ComponentKind::Source;
    }

    if binary_candidates(root, component.binary_name)
        .iter()
        .any(|path| path.is_file())
    {
        return ComponentKind::Binary;
    }

    ComponentKind::Missing
}

fn binary_candidates(root: &Path, binary_name: &str) -> Vec<std::path::PathBuf> {
    #[cfg(windows)]
    {
        vec![
            root.join(format!("{binary_name}.exe")),
            root.join(binary_name),
        ]
    }

    #[cfg(not(windows))]
    {
        vec![root.join(binary_name)]
    }
}

impl LaunchProfile {
    fn build_type(self) -> &'static str {
        match self {
            LaunchProfile::Release => "Release",
            LaunchProfile::Debug => "Debug",
        }
    }

    fn launch_suffix(self) -> &'static str {
        match self {
            LaunchProfile::Release => "Release",
            LaunchProfile::Debug => "Debug",
        }
    }

    fn cmake_build_preset(self) -> &'static str {
        match self {
            LaunchProfile::Debug => {
                if cfg!(windows) {
                    "windows-build-debug"
                } else if cfg!(target_os = "macos") {
                    "macos-build-debug"
                } else if cfg!(target_os = "linux") {
                    "linux-build-debug"
                } else {
                    "default-build-debug"
                }
            }
            LaunchProfile::Release => {
                if cfg!(windows) {
                    "windows-build-release"
                } else if cfg!(target_os = "macos") {
                    "macos-build-release"
                } else if cfg!(target_os = "linux") {
                    "linux-build-release"
                } else {
                    "default-build-release"
                }
            }
        }
    }

    fn cmake_configure_preset(self) -> &'static str {
        match self {
            LaunchProfile::Debug => {
                if cfg!(windows) {
                    "windows-debug"
                } else if cfg!(target_os = "macos") {
                    "macos-debug"
                } else if cfg!(target_os = "linux") {
                    "linux-debug"
                } else {
                    "default-debug"
                }
            }
            LaunchProfile::Release => {
                if cfg!(windows) {
                    "windows-release"
                } else if cfg!(target_os = "macos") {
                    "macos-release"
                } else if cfg!(target_os = "linux") {
                    "linux-release"
                } else {
                    "default-release"
                }
            }
        }
    }

    fn cmake_workspace_component_build_preset(self, component: &ComponentSpec) -> &'static str {
        match self {
            LaunchProfile::Debug => match component.key {
                "client" => {
                    if cfg!(windows) {
                        "windows-build-debug-client"
                    } else if cfg!(target_os = "macos") {
                        "macos-build-debug-client"
                    } else if cfg!(target_os = "linux") {
                        "linux-build-debug-client"
                    } else {
                        "default-build-debug-client"
                    }
                }
                "server" => {
                    if cfg!(windows) {
                        "windows-build-debug-server"
                    } else if cfg!(target_os = "macos") {
                        "macos-build-debug-server"
                    } else if cfg!(target_os = "linux") {
                        "linux-build-debug-server"
                    } else {
                        "default-build-debug-server"
                    }
                }
                _ => self.cmake_build_preset(),
            },
            LaunchProfile::Release => match component.key {
                "client" => {
                    if cfg!(windows) {
                        "windows-build-release-client"
                    } else if cfg!(target_os = "macos") {
                        "macos-build-release-client"
                    } else if cfg!(target_os = "linux") {
                        "linux-build-release-client"
                    } else {
                        "default-build-release-client"
                    }
                }
                "server" => {
                    if cfg!(windows) {
                        "windows-build-release-server"
                    } else if cfg!(target_os = "macos") {
                        "macos-build-release-server"
                    } else if cfg!(target_os = "linux") {
                        "linux-build-release-server"
                    } else {
                        "default-build-release-server"
                    }
                }
                _ => self.cmake_build_preset(),
            },
        }
    }
}

fn configure_task(component: &ComponentSpec, profile: LaunchProfile) -> Value {
    let configure_cmd = format!("cmake --preset {}", profile.cmake_configure_preset());

    let mut obj = Map::new();
    obj.insert(
        "label".to_string(),
        Value::String(configure_task_label(component, profile)),
    );
    obj.insert("type".to_string(), Value::String("shell".to_string()));
    obj.insert("command".to_string(), Value::String(configure_cmd));
    obj.insert(
        "options".to_string(),
        json!({
            "cwd": format!("${{workspaceFolder}}/{}", component.component_dir)
        }),
    );
    obj.insert("problemMatcher".to_string(), Value::Array(Vec::new()));

    Value::Object(obj)
}

fn source_launch_config(
    component: &ComponentSpec,
    profile: LaunchProfile,
    cli_endpoint: &str,
) -> Value {
    let program = if cfg!(windows) {
        format!(
            "${{workspaceFolder}}/{}/build/{}/{}.exe",
            component.component_dir,
            profile.build_type(),
            component.binary_name
        )
    } else {
        format!(
            "${{workspaceFolder}}/{}/build/{}/{}",
            component.component_dir,
            profile.build_type(),
            component.binary_name
        )
    };

    let mut obj = Map::new();
    obj.insert(
        "name".to_string(),
        Value::String(format!(
            "Germina {} ({})",
            component.title(),
            profile.launch_suffix()
        )),
    );
    obj.insert("type".to_string(), Value::String("lldb".to_string()));
    obj.insert("request".to_string(), Value::String("launch".to_string()));
    obj.insert("program".to_string(), Value::String(program));
    obj.insert(
        "cwd".to_string(),
        Value::String(format!("${{workspaceFolder}}/{}", component.component_dir)),
    );
    obj.insert(
        "args".to_string(),
        Value::Array(vec![
            Value::String("--cli".to_string()),
            Value::String(cli_endpoint.to_string()),
        ]),
    );
    obj.insert(
        "preLaunchTask".to_string(),
        Value::String(build_task_label(component, profile)),
    );

    Value::Object(obj)
}

fn workspace_source_launch_config(
    component: &ComponentSpec,
    profile: LaunchProfile,
    cli_endpoint: &str,
) -> Value {
    let program = if cfg!(windows) {
        format!(
            "${{workspaceFolder}}/out/{}/{}/{}.exe",
            profile.build_type(),
            component.component_dir,
            component.binary_name
        )
    } else {
        format!(
            "${{workspaceFolder}}/out/{}/{}/{}",
            profile.build_type(),
            component.component_dir,
            component.binary_name
        )
    };

    let mut obj = Map::new();
    obj.insert(
        "name".to_string(),
        Value::String(format!(
            "Germina {} ({})",
            component.title(),
            profile.launch_suffix()
        )),
    );
    obj.insert("type".to_string(), Value::String("lldb".to_string()));
    obj.insert("request".to_string(), Value::String("launch".to_string()));
    obj.insert("program".to_string(), Value::String(program));
    obj.insert(
        "cwd".to_string(),
        Value::String("${workspaceFolder}".to_string()),
    );
    obj.insert(
        "args".to_string(),
        Value::Array(vec![
            Value::String("--cli".to_string()),
            Value::String(cli_endpoint.to_string()),
        ]),
    );
    obj.insert(
        "preLaunchTask".to_string(),
        Value::String(build_task_label(component, profile)),
    );

    Value::Object(obj)
}

fn binary_attach_config(component: &ComponentSpec, profile: LaunchProfile, root: &Path) -> Value {
    let binary_path = binary_candidates(root, component.binary_name)
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.display().to_string())
        .unwrap_or_default();

    let mut obj = Map::new();
    obj.insert(
        "name".to_string(),
        Value::String(format!(
            "Germina {} ({})",
            component.title(),
            profile.launch_suffix()
        )),
    );
    obj.insert("type".to_string(), Value::String("lldb".to_string()));
    obj.insert("request".to_string(), Value::String("attach".to_string()));
    obj.insert(
        "pid".to_string(),
        Value::String("${command:pickProcess}".to_string()),
    );
    if !binary_path.is_empty() {
        obj.insert("program".to_string(), Value::String(binary_path));
    }

    Value::Object(obj)
}

fn build_task(component: &ComponentSpec, profile: LaunchProfile) -> Value {
    let build_cmd = format!("cmake --build --preset {}", profile.cmake_build_preset());

    let mut obj = Map::new();
    obj.insert(
        "label".to_string(),
        Value::String(build_task_label(component, profile)),
    );
    obj.insert("type".to_string(), Value::String("shell".to_string()));
    obj.insert("command".to_string(), Value::String(build_cmd));
    obj.insert(
        "dependsOn".to_string(),
        Value::Array(vec![Value::String(configure_task_label(
            component, profile,
        ))]),
    );
    obj.insert(
        "dependsOrder".to_string(),
        Value::String("sequence".to_string()),
    );
    obj.insert(
        "options".to_string(),
        json!({
            "cwd": format!("${{workspaceFolder}}/{}", component.component_dir)
        }),
    );
    obj.insert("problemMatcher".to_string(), Value::Array(Vec::new()));

    Value::Object(obj)
}

fn configure_workspace_task(profile: LaunchProfile) -> Value {
    let configure_cmd = format!("cmake --preset {}", profile.cmake_configure_preset());

    let mut obj = Map::new();
    obj.insert(
        "label".to_string(),
        Value::String(configure_workspace_task_label(profile)),
    );
    obj.insert("type".to_string(), Value::String("shell".to_string()));
    obj.insert("command".to_string(), Value::String(configure_cmd));
    obj.insert(
        "options".to_string(),
        json!({
            "cwd": "${workspaceFolder}"
        }),
    );
    obj.insert("problemMatcher".to_string(), Value::Array(Vec::new()));

    Value::Object(obj)
}

fn build_workspace_task(profile: LaunchProfile) -> Value {
    let build_cmd = format!("cmake --build --preset {}", profile.cmake_build_preset());

    let mut obj = Map::new();
    obj.insert(
        "label".to_string(),
        Value::String(build_workspace_task_label(profile)),
    );
    obj.insert("type".to_string(), Value::String("shell".to_string()));
    obj.insert("command".to_string(), Value::String(build_cmd));
    obj.insert(
        "dependsOn".to_string(),
        Value::Array(vec![Value::String(configure_workspace_task_label(profile))]),
    );
    obj.insert(
        "dependsOrder".to_string(),
        Value::String("sequence".to_string()),
    );
    obj.insert(
        "options".to_string(),
        json!({
            "cwd": "${workspaceFolder}"
        }),
    );
    obj.insert("problemMatcher".to_string(), Value::Array(Vec::new()));

    Value::Object(obj)
}

fn build_workspace_component_task(component: &ComponentSpec, profile: LaunchProfile) -> Value {
    let build_cmd = format!(
        "cmake --build --preset {}",
        profile.cmake_workspace_component_build_preset(component)
    );

    let mut obj = Map::new();
    obj.insert(
        "label".to_string(),
        Value::String(build_task_label(component, profile)),
    );
    obj.insert("type".to_string(), Value::String("shell".to_string()));
    obj.insert("command".to_string(), Value::String(build_cmd));
    obj.insert(
        "dependsOn".to_string(),
        Value::Array(vec![Value::String(configure_workspace_task_label(profile))]),
    );
    obj.insert(
        "dependsOrder".to_string(),
        Value::String("sequence".to_string()),
    );
    obj.insert(
        "options".to_string(),
        json!({
            "cwd": "${workspaceFolder}"
        }),
    );
    obj.insert("problemMatcher".to_string(), Value::Array(Vec::new()));

    Value::Object(obj)
}

fn configure_task_label(component: &ComponentSpec, profile: LaunchProfile) -> String {
    format!(
        "Germina Configure {} ({})",
        component.title(),
        profile.launch_suffix()
    )
}

fn build_task_label(component: &ComponentSpec, profile: LaunchProfile) -> String {
    format!(
        "Germina Build {} ({})",
        component.title(),
        profile.launch_suffix()
    )
}

fn configure_workspace_task_label(profile: LaunchProfile) -> String {
    format!("Germina Configure Workspace ({})", profile.launch_suffix())
}

fn build_workspace_task_label(profile: LaunchProfile) -> String {
    format!("Germina Build Workspace ({})", profile.launch_suffix())
}
