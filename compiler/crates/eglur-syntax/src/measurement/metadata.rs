use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::report::{ExecutionContext, HostContext, RevisionContext, ToolchainContext};

pub(super) fn execution_context(repository_root: &Path) -> Result<ExecutionContext, String> {
    let rustc_program = std::env::var_os("RUSTC").unwrap_or_else(|| OsString::from("rustc"));
    let rustc_command = rustc_program.to_string_lossy().into_owned();
    let rustc_verbose =
        command_output(repository_root, &rustc_program, &["--version", "--verbose"])?;
    let rustc_host = rustc_verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| "rustc verbose output did not contain a host target".to_owned())?
        .to_owned();
    let commit = git_output(repository_root, &["rev-parse", "HEAD"])?;
    let dirty = !git_output(
        repository_root,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    )?
    .is_empty();
    let cargo_lock_object_format =
        git_output(repository_root, &["rev-parse", "--show-object-format"])?;
    let cargo_lock_object_id = hash_file(repository_root, &repository_root.join("Cargo.lock"))?;
    let captured_at_unix_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock precedes the Unix epoch: {error}"))?
        .as_secs();

    Ok(ExecutionContext {
        captured_at_unix_seconds,
        revision: RevisionContext { commit, dirty },
        toolchain: ToolchainContext {
            rustc_command,
            rustc_verbose,
            rustc_host,
            cargo_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            cargo_lock_object_format,
            cargo_lock_object_id,
        },
        host: HostContext {
            os: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            kernel: optional_command_output(repository_root, "uname", &["-sr"]),
            cpu_model: cpu_model(repository_root),
            logical_parallelism: std::thread::available_parallelism().ok().map(usize::from),
            memory_bytes: memory_bytes(repository_root),
        },
    })
}

pub(super) fn git_object_identity(
    repository_root: &Path,
    path: &Path,
) -> Result<(String, String), String> {
    Ok((
        git_output(repository_root, &["rev-parse", "--show-object-format"])?,
        hash_file(repository_root, path)?,
    ))
}

fn hash_file(repository_root: &Path, path: &Path) -> Result<String, String> {
    let path = path
        .strip_prefix(repository_root)
        .map_err(|_| format!("{} is outside the repository", path.display()))?;
    let path = path
        .to_str()
        .ok_or_else(|| format!("{} is not a UTF-8 path", path.display()))?;
    git_output(repository_root, &["hash-object", "--", path])
}

fn git_output(repository_root: &Path, arguments: &[&str]) -> Result<String, String> {
    command_output(repository_root, OsStr::new("git"), arguments)
}

fn command_output(
    working_directory: &Path,
    program: &OsStr,
    arguments: &[&str],
) -> Result<String, String> {
    let program_label = program.to_string_lossy();
    let output = Command::new(program)
        .args(arguments)
        .current_dir(working_directory)
        .output()
        .map_err(|error| format!("failed to run {program_label}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program_label} exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| format!("{program_label} emitted non-UTF-8 output: {error}"))
}

fn optional_command_output(
    working_directory: &Path,
    program: &str,
    arguments: &[&str],
) -> Option<String> {
    command_output(working_directory, OsStr::new(program), arguments)
        .ok()
        .filter(|value| !value.is_empty())
}

fn cpu_model(repository_root: &Path) -> Option<String> {
    if std::env::consts::OS == "macos" {
        return optional_command_output(
            repository_root,
            "sysctl",
            &["-n", "machdep.cpu.brand_string"],
        );
    }
    if std::env::consts::OS == "linux" {
        return fs::read_to_string("/proc/cpuinfo")
            .ok()?
            .lines()
            .find_map(|line| line.split_once(':'))
            .filter(|(key, _)| key.trim() == "model name")
            .map(|(_, value)| value.trim().to_owned());
    }
    None
}

fn memory_bytes(repository_root: &Path) -> Option<u64> {
    if std::env::consts::OS == "macos" {
        return optional_command_output(repository_root, "sysctl", &["-n", "hw.memsize"])?
            .parse()
            .ok();
    }
    if std::env::consts::OS == "linux" {
        let kilobytes = fs::read_to_string("/proc/meminfo")
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()?;
        return kilobytes.checked_mul(1024);
    }
    None
}
