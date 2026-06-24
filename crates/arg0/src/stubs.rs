/// Inline stubs for dropped crates that codex-arg0 depended on.
///
/// ── codex_apply_patch stubs ───────────────────────────────────────────────────
pub const CODEX_CORE_APPLY_PATCH_ARG1: &str = "--codex-run-as-apply-patch";

pub fn apply_patch_main() -> ! {
    eprintln!("apply_patch: not supported in this build");
    std::process::exit(1);
}

#[allow(dead_code)]
pub async fn apply_patch(
    _patch: &str,
    _cwd: &std::path::Path,
    _stdout: &mut dyn std::io::Write,
    _stderr: &mut dyn std::io::Write,
    _fs: &dyn std::any::Any,
    _sandbox: Option<()>,
) -> Result<(), String> {
    Err("apply_patch: not supported in this build".to_string())
}

// ── codex_install_context stubs ───────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct PackageLayout {
    pub path_dir: Option<std::path::PathBuf>,
}

#[derive(Debug)]
pub struct InstallContext {
    pub package_layout: Option<PackageLayout>,
}

impl InstallContext {
    pub fn current() -> &'static InstallContext {
        static INSTANCE: std::sync::OnceLock<InstallContext> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(|| InstallContext {
            package_layout: None,
        })
    }
}

// ── codex_utils_home_dir stubs ────────────────────────────────────────────────

/// Returns the agentx home directory (`~/.agentx` or `$AGENTX_HOME`).
pub fn find_codex_home() -> std::io::Result<std::path::PathBuf> {
    if let Ok(val) = std::env::var("AGENTX_HOME") {
        return Ok(std::path::PathBuf::from(val));
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "cannot determine home directory",
            )
        })?;
    Ok(std::path::PathBuf::from(home).join(".agentx"))
}

// ── codex_linux_sandbox stub ──────────────────────────────────────────────────

/// Stub: the Linux sandbox dispatch entry point.  Never called on non-Linux.
pub fn linux_sandbox_run_main() -> ! {
    eprintln!("codex-linux-sandbox: not supported in this build");
    std::process::exit(1);
}

// ── codex_shell_escalation stub ───────────────────────────────────────────────

/// Stub: execve-wrapper dispatch.  Only compiled on Unix targets.
#[cfg(unix)]
pub async fn run_shell_escalation_execve_wrapper(
    _file: String,
    _argv: Vec<String>,
) -> Result<i32, String> {
    Err("shell escalation: not supported in this build".to_string())
}
