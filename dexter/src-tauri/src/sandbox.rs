use regex::Regex;
use std::collections::HashSet;
use std::io::Write;
use std::process::Command;
use std::sync::Mutex;

/// Sandbox security level.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum SandboxMode {
    /// Commands run in a temp workspace with env sanitization + blocklist.
    Guarded,
    /// Commands run inside a Docker container with volume mounts.
    Docker,
}

impl Default for SandboxMode {
    fn default() -> Self {
        Self::Guarded
    }
}

/// Configuration for the sandboxed shell.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SandboxConfig {
    #[serde(default)]
    pub mode: SandboxMode,
    /// Max seconds a command can run before being killed.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Directories the LLM is allowed to read (guarded mode).
    #[serde(default = "default_readable_paths")]
    pub readable_paths: Vec<String>,
    /// The workspace directory commands run in.
    #[serde(default = "default_workspace")]
    pub workspace: String,
    /// Docker image for Docker mode.
    #[serde(default = "default_docker_image")]
    pub docker_image: String,
    /// Whether network access is allowed in Docker mode.
    #[serde(default = "default_true")]
    pub allow_network: bool,
}

fn default_timeout() -> u64 {
    30
}
fn default_true() -> bool {
    true
}
fn default_readable_paths() -> Vec<String> {
    vec![
        "~/Documents".to_string(),
        "~/Desktop".to_string(),
        "~/Downloads".to_string(),
        "~/Projects".to_string(),
    ]
}
fn default_workspace() -> String {
    dirs::home_dir()
        .map(|h| h.join(".voice-assistant-sandbox").to_string_lossy().to_string())
        .unwrap_or_else(|| "C:/temp/voice-assistant-sandbox".to_string())
}
fn default_docker_image() -> String {
    "ubuntu:24.04".to_string()
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            mode: SandboxMode::default(),
            timeout_secs: default_timeout(),
            readable_paths: default_readable_paths(),
            workspace: default_workspace(),
            docker_image: default_docker_image(),
            allow_network: true,
        }
    }
}

// ── Blocked command patterns ──

const BLOCKED_COMMANDS: &[&str] = &[
    "sudo",
    "su",
    "doas",
    "pkexec",
    "runas",
];

const BLOCKED_PATTERNS: &[&str] = &[
    // Destructive filesystem ops (Windows)
    "rm -rf /",
    "rm -rf ~",
    "rm -rf $HOME",
    "rm -rf /*",
    "rm -rf ~/*",
    "del /f /s C:\\",
    "rd /s /q C:\\",
    "format C:",
    "format D:",
    // Disk/partition ops
    "mkfs",
    "dd if=",
    "diskpart",
    "diskutil eraseDisk",
    "diskutil partitionDisk",
    // System control
    "shutdown",
    "reboot",
    "halt",
    "poweroff",
    "shutdown /s",
    "shutdown /r",
    "launchctl unload",
    "csrutil",
    // Fork bomb variants
    ":(){ :|:& };:",
    ".(){.|.&};.",
    // Credential theft
    "security find-generic-password",
    "security find-internet-password",
    "security dump-keychain",
    // Network exfiltration of sensitive files
    "curl.*~/.ssh",
    "curl.*/.aws",
    "curl.*/.env",
    "wget.*~/.ssh",
    "Invoke-WebRequest.*\\.ssh",
    // Chmod dangerous
    "chmod -R 777 /",
    "chmod -R 777 ~",
    "icacls C:\\ /grant",
    // Kill all
    "killall -9",
    "kill -9 -1",
    "pkill -9",
    "taskkill /f /fi \"PID ne 0\"",
];

const STRIPPED_ENV_VARS: &[&str] = &[
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "GITHUB_TOKEN",
    "GITLAB_TOKEN",
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "OPENROUTER_API_KEY",
    "DATABASE_URL",
    "REDIS_URL",
    "SECRET_KEY",
    "PRIVATE_KEY",
    "SSH_AUTH_SOCK",
    "GPG_AGENT_INFO",
    "HOMEBREW_GITHUB_API_TOKEN",
    "NPM_TOKEN",
    "CARGO_REGISTRY_TOKEN",
    "DOCKER_PASSWORD",
    "NERVE_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_ENDPOINT",
    "GOOGLE_API_KEY",
    "COHERE_API_KEY",
];

/// Audit logger for sandbox commands.
pub struct AuditLog {
    path: std::path::PathBuf,
}

impl AuditLog {
    pub fn new() -> Self {
        let path = dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
            .join("voice-assistant")
            .join("sandbox-audit.log");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        Self { path }
    }

    pub fn log(&self, command: &str, result: &str, blocked: bool) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let status = if blocked { "BLOCKED" } else { "EXECUTED" };
        let entry = format!(
            "[{}] {} | cmd: {} | result: {}\n",
            timestamp,
            status,
            command,
            if result.len() > 200 {
                format!("{}...", &result[..200])
            } else {
                result.to_string()
            }
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            let _ = f.write_all(entry.as_bytes());
        }
    }
}

/// Map common LLM mistakes to real Windows inbox executable names. Sandbox PATH only includes
/// System32 etc.; there is `mspaint.exe`, not `paint.exe`.
fn normalize_windows_launch_command(command: &str) -> String {
    let trimmed = command.trim();
    let mut out = trimmed.to_string();

    if let Ok(re) = Regex::new(r"(?i)(Start-Process\s+)paint(\s|$|;|\)|&)") {
        out = re.replace_all(&out, "${1}mspaint$2").into_owned();
    }
    if let Ok(re) = Regex::new(r"(?i)(\s-Name\s+)paint(\s|$|;|\)|&)") {
        out = re.replace_all(&out, "${1}mspaint$2").into_owned();
    }

    let t = out.trim();
    if t.eq_ignore_ascii_case("paint") || t.eq_ignore_ascii_case("paint.exe") {
        return "Start-Process mspaint".to_string();
    }

    out
}

/// Validate a command against the blocklist. Returns Err with reason if blocked.
pub fn validate_command(command: &str) -> Result<(), String> {
    let lower = command.to_lowercase();
    let trimmed = command.trim();

    // Check if command starts with a blocked command
    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    // Strip path prefix (e.g. C:\Windows\System32\cmd.exe -> cmd.exe)
    let base_cmd = if let Some(pos) = first_word.rfind('\\') {
        &first_word[pos + 1..]
    } else if let Some(pos) = first_word.rfind('/') {
        &first_word[pos + 1..]
    } else {
        first_word
    };

    if BLOCKED_COMMANDS.contains(&base_cmd) {
        return Err(format!(
            "Command '{}' is blocked — elevated privileges are not allowed in the sandbox.",
            base_cmd
        ));
    }

    // Check for pipe chains that start with a blocked command
    for segment in command.split('|').chain(command.split("&&")).chain(command.split(';')) {
        let seg_first = segment.trim().split_whitespace().next().unwrap_or("");
        let seg_base = if let Some(pos) = seg_first.rfind('\\') {
            &seg_first[pos + 1..]
        } else {
            seg_first
        };
        if BLOCKED_COMMANDS.contains(&seg_base) {
            return Err(format!(
                "Command '{}' in pipeline is blocked — elevated privileges are not allowed.",
                seg_base
            ));
        }
    }

    // Check for blocked patterns
    for pattern in BLOCKED_PATTERNS {
        if lower.contains(&pattern.to_lowercase()) {
            return Err(format!(
                "Command blocked — matches dangerous pattern: '{}'",
                pattern
            ));
        }
    }

    Ok(())
}

/// Build a sanitized environment for the sandbox.
fn build_safe_env() -> Vec<(String, String)> {
    let stripped: HashSet<&str> = STRIPPED_ENV_VARS.iter().copied().collect();
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| !stripped.contains(k.as_str()))
        .collect();

    // Override PATH to only include safe Windows locations
    let safe_path = "C:\\Windows\\System32;C:\\Windows;C:\\Windows\\System32\\WindowsPowerShell\\v1.0";
    env.retain(|(k, _)| k != "PATH");
    env.push(("PATH".to_string(), safe_path.to_string()));

    // Set home dirs
    if let Some(home) = dirs::home_dir() {
        env.retain(|(k, _)| k != "HOME" && k != "USERPROFILE");
        env.push(("USERPROFILE".to_string(), home.to_string_lossy().to_string()));
        env.push(("HOME".to_string(), home.to_string_lossy().to_string()));
    }

    env
}

/// Execute a command in guarded mode (PowerShell on Windows).
fn run_guarded(
    command: &str,
    config: &SandboxConfig,
) -> Result<String, String> {
    // Ensure workspace exists
    let _ = std::fs::create_dir_all(&config.workspace);

    let env = build_safe_env();

    let (tx, rx) = std::sync::mpsc::channel();
    let cmd = command.to_string();
    let workspace = config.workspace.clone();
    let env_clone = env.clone();

    std::thread::spawn(move || {
        let result = Command::new("powershell")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &cmd])
            .current_dir(&workspace)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env_clear()
            .envs(env_clone)
            .output();
        let _ = tx.send(result);
    });

    let output = match rx.recv_timeout(std::time::Duration::from_secs(config.timeout_secs)) {
        Ok(result) => result.map_err(|e| format!("Falha ao executar: {}", e))?,
        Err(_) => return Err(format!("Command timed out after {}s", config.timeout_secs)),
    };

    format_output(&output)
}

/// Execute a command in Docker mode.
fn run_docker(
    command: &str,
    config: &SandboxConfig,
) -> Result<String, String> {
    // Check if Docker is available
    let docker_check = Command::new("docker")
        .args(["info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match docker_check {
        Ok(status) if status.success() => {}
        _ => return Err("Docker is not running. Switch to Guarded mode in settings or start Docker Desktop.".to_string()),
    }

    let _ = std::fs::create_dir_all(&config.workspace);

    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--memory=512m".to_string(),
        "--cpus=1.0".to_string(),
        "--pids-limit=100".to_string(),
        "--security-opt=no-new-privileges".to_string(),
        "--read-only".to_string(),
        "-v".to_string(),
        format!("{}:/workspace", config.workspace),
        "-w".to_string(),
        "/workspace".to_string(),
        "--tmpfs".to_string(),
        "/tmp:rw,noexec,nosuid,size=64m".to_string(),
    ];

    // Mount readable paths as read-only
    for path in &config.readable_paths {
        let expanded = path.replace('~', &dirs::home_dir().unwrap_or_default().to_string_lossy());
        if std::path::Path::new(&expanded).exists() {
            let mount_point = format!(
                "/mnt/{}",
                std::path::Path::new(&expanded)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            );
            args.push("-v".to_string());
            args.push(format!("{}:{}:ro", expanded, mount_point));
        }
    }

    if !config.allow_network {
        args.push("--network=none".to_string());
    }

    args.push(config.docker_image.clone());
    args.push("sh".to_string());
    args.push("-c".to_string());
    args.push(command.to_string());

    let (tx, rx) = std::sync::mpsc::channel();
    let args_clone = args.clone();

    std::thread::spawn(move || {
        let result = Command::new("docker")
            .args(&args_clone)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output();
        let _ = tx.send(result);
    });

    let timeout = config.timeout_secs + 5;
    let output = match rx.recv_timeout(std::time::Duration::from_secs(timeout)) {
        Ok(result) => result.map_err(|e| format!("Docker exec failed: {}", e))?,
        Err(_) => return Err(format!("Docker command timed out after {}s", timeout)),
    };

    format_output(&output)
}

/// Format command output, truncating if needed.
fn format_output(output: &std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    let max_len = 4000;
    let mut result = String::new();

    if !stdout.is_empty() {
        if stdout.len() > max_len {
            result.push_str(&stdout[..max_len]);
            result.push_str("\n... (output truncated)");
        } else {
            result.push_str(&stdout);
        }
    }

    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push_str("\n\nSTDERR:\n");
        }
        let stderr_trunc = if stderr.len() > 1000 {
            format!("{}... (truncated)", &stderr[..1000])
        } else {
            stderr
        };
        result.push_str(&stderr_trunc);
    }

    if !output.status.success() {
        result = format!(
            "Exit code: {}\n{}",
            output.status.code().unwrap_or(-1),
            result
        );
    }

    if result.is_empty() {
        result = "(no output)".to_string();
    }

    Ok(result)
}

/// The main entry point — validate, execute, and audit.
pub fn execute(command: &str, config: &SandboxConfig, audit: &Mutex<AuditLog>) -> Result<String, String> {
    let command = normalize_windows_launch_command(command);

    // Validate first
    if let Err(reason) = validate_command(&command) {
        if let Ok(log) = audit.lock() {
            log.log(&command, &reason, true);
        }
        return Err(reason);
    }

    // Execute based on mode
    let result = match config.mode {
        SandboxMode::Guarded => run_guarded(&command, config),
        SandboxMode::Docker => run_docker(&command, config),
    };

    // Audit
    if let Ok(log) = audit.lock() {
        match &result {
            Ok(output) => log.log(&command, output, false),
            Err(e) => log.log(&command, e, false),
        }
    }

    result
}

#[cfg(test)]
mod normalize_tests {
    use super::normalize_windows_launch_command;

    #[test]
    fn start_process_paint_becomes_mspaint() {
        assert_eq!(
            normalize_windows_launch_command("Start-Process paint"),
            "Start-Process mspaint"
        );
    }

    #[test]
    fn bare_paint_becomes_start_mspaint() {
        assert_eq!(
            normalize_windows_launch_command("  paint.exe  "),
            "Start-Process mspaint"
        );
    }
}
