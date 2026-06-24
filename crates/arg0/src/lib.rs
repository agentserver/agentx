mod stubs;

use std::ffi::OsString;
use std::fs::File;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;

use crate::stubs::CODEX_CORE_APPLY_PATCH_ARG1;
use crate::stubs::InstallContext;
use crate::stubs::find_codex_home;
use agentx_exec_server::CODEX_FS_HELPER_ARG1;
use agentx_sandboxing::landlock::CODEX_LINUX_SANDBOX_ARG0;
#[cfg(target_os = "windows")]
use agentx_windows_sandbox::CODEX_WINDOWS_SANDBOX_ARG1;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use tempfile::TempDir;

const APPLY_PATCH_ARG0: &str = "apply_patch";
const MISSPELLED_APPLY_PATCH_ARG0: &str = "applypatch";
#[cfg(unix)]
const EXECVE_WRAPPER_ARG0: &str = "codex-execve-wrapper";
const LOCK_FILENAME: &str = ".lock";
const TOKIO_WORKER_STACK_SIZE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Arg0DispatchPaths {
    /// Stable path to the current Codex executable for child re-execs.
    ///
    /// Prefer this over [`std::env::current_exe()`] in code that may run under
    /// a test harness, where `current_exe()` can point at the harness binary
    /// instead of the real Codex CLI.
    pub codex_self_exe: Option<PathBuf>,
    pub codex_linux_sandbox_exe: Option<PathBuf>,
    pub main_execve_wrapper_exe: Option<PathBuf>,
}

/// Keeps the per-session PATH entry alive and locked for the process lifetime.
pub struct Arg0PathEntryGuard {
    _temp_dir: TempDir,
    _lock_file: File,
    paths: Arg0DispatchPaths,
}

impl Arg0PathEntryGuard {
    fn new(temp_dir: TempDir, lock_file: File, paths: Arg0DispatchPaths) -> Self {
        Self {
            _temp_dir: temp_dir,
            _lock_file: lock_file,
            paths,
        }
    }

    pub fn paths(&self) -> &Arg0DispatchPaths {
        &self.paths
    }
}

pub fn arg0_dispatch() -> Option<Arg0PathEntryGuard> {
    // Determine if we were invoked via the special alias.
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_default();
    let exe_name = Path::new(&argv0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    #[cfg(unix)]
    if exe_name == EXECVE_WRAPPER_ARG0 {
        let mut args = std::env::args();
        let _ = args.next();
        let file = match args.next() {
            Some(file) => file,
            None => std::process::exit(1),
        };
        let argv = args.collect::<Vec<_>>();

        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => std::process::exit(1),
        };
        let exit_code = runtime.block_on(
            crate::stubs::run_shell_escalation_execve_wrapper(file, argv),
        );
        match exit_code {
            Ok(exit_code) => std::process::exit(exit_code),
            Err(_) => std::process::exit(1),
        }
    }

    if exe_name == CODEX_LINUX_SANDBOX_ARG0 {
        // Safety: [`linux_sandbox_run_main`] never returns.
        crate::stubs::linux_sandbox_run_main();
    } else if exe_name == APPLY_PATCH_ARG0 || exe_name == MISSPELLED_APPLY_PATCH_ARG0 {
        crate::stubs::apply_patch_main();
    }

    let argv1 = args.next().unwrap_or_default();
    if argv1 == CODEX_FS_HELPER_ARG1 {
        agentx_exec_server::run_fs_helper_main();
    }
    #[cfg(target_os = "windows")]
    if argv1 == CODEX_WINDOWS_SANDBOX_ARG1 {
        codex_windows_sandbox::run_windows_sandbox_wrapper_main();
    }
    if argv1 == CODEX_CORE_APPLY_PATCH_ARG1 {
        eprintln!("Error: {CODEX_CORE_APPLY_PATCH_ARG1} is not supported in this build.");
        std::process::exit(1);
    }

    // This modifies the environment, which is not thread-safe, so do this
    // before creating any threads/the Tokio runtime.
    load_dotenv();

    let (path_entry_guard, updated_path_env_var) = prepare_path_env_var_with_aliases(
        InstallContext::current(),
        std::env::var_os("PATH"),
        prepare_path_entry_for_codex_aliases,
    );
    if let Some(updated_path_env_var) = updated_path_env_var {
        // It is safe to call set_var() because our process is single-threaded at
        // this point in its execution.
        unsafe {
            std::env::set_var("PATH", updated_path_env_var);
        }
    }
    path_entry_guard
}

fn prepare_path_env_var_with_aliases(
    install_context: &InstallContext,
    existing_path: Option<OsString>,
    prepare_aliases: impl FnOnce(Option<OsString>) -> std::io::Result<(Arg0PathEntryGuard, OsString)>,
) -> (Option<Arg0PathEntryGuard>, Option<OsString>) {
    let package_path = path_env_with_package_path_dir(install_context, existing_path.clone());
    let path_for_aliases = package_path.clone().or(existing_path);

    match prepare_aliases(path_for_aliases) {
        Ok((path_entry, updated_path_env_var)) => (Some(path_entry), Some(updated_path_env_var)),
        Err(err) => {
            // It is possible that Codex will proceed successfully even if
            // creating helper aliases fails, so warn the user and move on.
            eprintln!("WARNING: proceeding, even though we could not create PATH aliases: {err}");
            (None, package_path)
        }
    }
}

/// While we want to deploy the Codex CLI as a single executable for simplicity,
/// we also want to expose some of its functionality as distinct CLIs, so we use
/// the "arg0 trick" to determine which CLI to dispatch. This effectively allows
/// us to simulate deploying multiple executables as a single binary on Mac and
/// Linux (but not Windows).
///
/// When the current executable is invoked through the hard-link or alias named
/// `codex-linux-sandbox` we *directly* execute
/// [`codex_linux_sandbox::run_main`] (which never returns). Otherwise we:
///
/// 1.  Load `.env` values from `~/.codex/.env` before creating any threads.
/// 2.  Spawn a main runtime thread with a controlled stack size.
/// 3.  Construct a Tokio multi-thread runtime.
/// 4.  Capture the current executable path and derive the
///     `codex-linux-sandbox` helper path (falling back to the current
///     executable if needed) so children can re-invoke the sandbox when running
///     on Linux.
/// 5.  Execute the provided async `main_fn` inside that runtime, forwarding any
///     error. Note that `main_fn` receives [`Arg0DispatchPaths`], which
///     contains the helper executable paths needed to construct
///     [`codex_core::config::Config`].
///
/// This function should be used to wrap any `main()` function in binary crates
/// in this workspace that depends on these helper CLIs.
pub fn arg0_dispatch_or_else<F, Fut>(main_fn: F) -> anyhow::Result<()>
where
    F: FnOnce(Arg0DispatchPaths) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>>,
{
    // Retain the TempDir so it exists for the lifetime of the invocation of
    // this executable. Admittedly, we could invoke `keep()` on it, but it
    // would be nice to avoid leaving temporary directories behind, if possible.
    let path_entry_guard = arg0_dispatch();
    let current_exe = std::env::current_exe().ok();

    // Regular invocation. Run the async entry point on a thread with the same
    // stack budget as Tokio workers; `Runtime::block_on` otherwise runs the
    // top-level future on the caller's OS stack.
    let handle = std::thread::Builder::new()
        .name("codex-main".to_string())
        .stack_size(TOKIO_WORKER_STACK_SIZE_BYTES)
        .spawn(move || {
            let runtime = build_runtime()?;
            runtime.block_on(run_main_with_arg0_guard(
                path_entry_guard,
                current_exe,
                main_fn,
            ))
        })?;
    match handle.join() {
        Ok(result) => result,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

async fn run_main_with_arg0_guard<F, Fut>(
    path_entry_guard: Option<Arg0PathEntryGuard>,
    current_exe: Option<PathBuf>,
    main_fn: F,
) -> anyhow::Result<()>
where
    F: FnOnce(Arg0DispatchPaths) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let paths = Arg0DispatchPaths {
        codex_self_exe: current_exe.clone(),
        codex_linux_sandbox_exe: if cfg!(target_os = "linux") {
            linux_sandbox_exe_path(path_entry_guard.as_ref(), current_exe)
        } else {
            None
        },
        main_execve_wrapper_exe: path_entry_guard
            .as_ref()
            .and_then(|path_entry| path_entry.paths().main_execve_wrapper_exe.clone()),
    };

    let result = main_fn(paths).await;
    // Keep the arg0 tempdir guard alive until the async entry point finishes;
    // runtime paths above can point at aliases inside that directory.
    drop(path_entry_guard);
    result
}

fn linux_sandbox_exe_path(
    path_entry_guard: Option<&Arg0PathEntryGuard>,
    current_exe: Option<PathBuf>,
) -> Option<PathBuf> {
    // Prefer the `codex-linux-sandbox` alias when available so callers can
    // re-exec through a path whose basename still triggers arg0 dispatch on
    // bubblewrap builds that do not support `--argv0`.
    path_entry_guard
        .and_then(|path_entry| path_entry.paths().codex_linux_sandbox_exe.clone())
        .or(current_exe)
}

fn build_runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    builder.thread_stack_size(TOKIO_WORKER_STACK_SIZE_BYTES);
    Ok(builder.build()?)
}

const ILLEGAL_ENV_VAR_PREFIX: &str = "AGENTX_";

/// Load env vars from ~/.agentx/.env.
///
/// Security: Do not allow `.env` files to create or modify any variables
/// with names starting with `AGENTX_`.
fn load_dotenv() {
    if let Ok(codex_home) = find_codex_home()
        && let Ok(iter) = dotenvy::from_path_iter(codex_home.join(".env"))
    {
        set_filtered(iter);
    }
}

/// Helper to set vars from a dotenvy iterator while filtering out `AGENTX_` keys.
fn set_filtered<I>(iter: I)
where
    I: IntoIterator<Item = Result<(String, String), dotenvy::Error>>,
{
    for (key, value) in iter.into_iter().flatten() {
        if !key.to_ascii_uppercase().starts_with(ILLEGAL_ENV_VAR_PREFIX) {
            // It is safe to call set_var() because our process is
            // single-threaded at this point in its execution.
            unsafe { std::env::set_var(&key, &value) };
        }
    }
}

/// Creates a temporary directory with either:
///
/// - UNIX: `apply_patch` symlink to the current executable
/// - WINDOWS: `apply_patch.bat` batch script to invoke the current executable
///   with the hidden `--codex-run-as-apply-patch` flag.
///
/// Returns the temporary directory guard and the PATH value that prepends the
/// temporary directory so `apply_patch` can be on the PATH without requiring the
/// user to install a separate executable, simplifying the deployment of Codex
/// CLI.
/// Note: In debug builds the temp-dir guard is disabled to ease local testing.
///
/// IMPORTANT: Callers must update PATH before multiple threads are spawned.
fn prepare_path_entry_for_codex_aliases(
    existing_path: Option<OsString>,
) -> std::io::Result<(Arg0PathEntryGuard, OsString)> {
    let codex_home = find_codex_home()?;
    #[cfg(not(debug_assertions))]
    {
        // Guard against placing helpers in system temp directories outside debug builds.
        let temp_root = std::env::temp_dir();
        if codex_home.starts_with(&temp_root) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Refusing to create helper binaries under temporary dir {temp_root:?} (codex_home: {codex_home:?})"
                ),
            ));
        }
    }

    std::fs::create_dir_all(&codex_home)?;
    // Use an AGENTX_HOME-scoped temp root to avoid cluttering the top-level directory.
    let temp_root = codex_home.join("tmp").join("arg0");
    std::fs::create_dir_all(&temp_root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // Ensure only the current user can access the temp directory.
        std::fs::set_permissions(&temp_root, std::fs::Permissions::from_mode(0o700))?;
    }

    // Best-effort cleanup of stale per-session dirs. Ignore failures so startup proceeds.
    if let Err(err) = janitor_cleanup(&temp_root) {
        eprintln!("WARNING: failed to clean up stale arg0 temp dirs: {err}");
    }

    let temp_dir = tempfile::Builder::new()
        .prefix("codex-arg0")
        .tempdir_in(&temp_root)?;
    let path = temp_dir.path();

    let lock_path = path.join(LOCK_FILENAME);
    let lock_file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file.try_lock()?;

    for filename in &[
        APPLY_PATCH_ARG0,
        MISSPELLED_APPLY_PATCH_ARG0,
        #[cfg(target_os = "linux")]
        CODEX_LINUX_SANDBOX_ARG0,
        #[cfg(unix)]
        EXECVE_WRAPPER_ARG0,
    ] {
        let exe = std::env::current_exe()?;

        #[cfg(unix)]
        {
            let link = path.join(filename);
            symlink(&exe, &link)?;
        }

        #[cfg(windows)]
        {
            let batch_script = path.join(format!("{filename}.bat"));
            let exe = exe.display();
            std::fs::write(
                &batch_script,
                format!(
                    r#"@echo off
"{exe}" {CODEX_CORE_APPLY_PATCH_ARG1} %*
"#,
                ),
            )?;
        }
    }

    let updated_path_env_var = path_env_with_entry(path, existing_path);

    let paths = Arg0DispatchPaths {
        codex_self_exe: std::env::current_exe().ok(),
        codex_linux_sandbox_exe: {
            #[cfg(target_os = "linux")]
            {
                Some(path.join(CODEX_LINUX_SANDBOX_ARG0))
            }
            #[cfg(not(target_os = "linux"))]
            {
                None
            }
        },
        main_execve_wrapper_exe: {
            #[cfg(unix)]
            {
                Some(path.join(EXECVE_WRAPPER_ARG0))
            }
            #[cfg(not(unix))]
            {
                None
            }
        },
    };

    Ok((
        Arg0PathEntryGuard::new(temp_dir, lock_file, paths),
        updated_path_env_var,
    ))
}

fn path_env_with_package_path_dir(
    install_context: &InstallContext,
    existing_path: Option<OsString>,
) -> Option<OsString> {
    let path_dir = install_context
        .package_layout
        .as_ref()
        .and_then(|package_layout| package_layout.path_dir.as_ref())?;
    Some(path_env_with_entry(path_dir.as_path(), existing_path))
}

fn path_env_with_entry(path_entry: &Path, existing_path: Option<OsString>) -> OsString {
    #[cfg(unix)]
    const PATH_SEPARATOR: &str = ":";

    #[cfg(windows)]
    const PATH_SEPARATOR: &str = ";";

    let capacity = path_entry.as_os_str().len()
        + existing_path
            .as_ref()
            .map_or(0, |existing_path| 1 + existing_path.len());
    let mut path_env_var = OsString::with_capacity(capacity);
    path_env_var.push(path_entry);
    if let Some(existing_path) = existing_path {
        path_env_var.push(PATH_SEPARATOR);
        path_env_var.push(existing_path);
    }
    path_env_var
}

fn janitor_cleanup(temp_root: &Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(temp_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Skip the directory if locking fails or the lock is currently held.
        let Some(_lock_file) = try_lock_dir(&path)? else {
            continue;
        };

        match std::fs::remove_dir_all(&path) {
            Ok(()) => {}
            // Expected TOCTOU race: directory can disappear after read_dir/lock checks.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

fn try_lock_dir(dir: &Path) -> std::io::Result<Option<File>> {
    let lock_path = dir.join(LOCK_FILENAME);
    let lock_file = match File::options().read(true).write(true).open(&lock_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };

    match lock_file.try_lock() {
        Ok(()) => Ok(Some(lock_file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

// Tests module deleted: used codex_install_context (dropped crate).
