#[cfg(unix)]
use std::io::BufRead as _;
#[cfg(unix)]
use std::io::BufReader as StdBufReader;
#[cfg(unix)]
use std::io::Read as _;
#[cfg(unix)]
use std::io::Write as _;
#[cfg(unix)]
use std::net::TcpStream;
use std::path::Path;
#[cfg(unix)]
use std::process::Stdio;
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use anyhow::Result;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::TempDir;

fn codex_command(codex_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("CODEX_HOME", codex_home);
    Ok(cmd)
}

#[test]
fn strict_config_rejects_unknown_config_fields_for_exec_server() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(
        codex_home.path().join("config.toml"),
        r#"
foo = "bar"
"#,
    )?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "exec-server",
        "--strict-config",
        "--listen",
        "http://127.0.0.1:0",
    ])
    .assert()
    .failure()
    .stderr(contains("unknown configuration field"));

    Ok(())
}

#[test]
fn local_exec_server_ignores_invalid_config_without_strict_config() -> Result<()> {
    let codex_home = TempDir::new()?;
    std::fs::write(codex_home.path().join("config.toml"), "not valid toml = [")?;

    let mut cmd = codex_command(codex_home.path())?;
    cmd.args(["exec-server", "--listen", "stdio"])
        .assert()
        .success()
        .stderr(contains("not valid toml").not());

    Ok(())
}

/// The standalone exec-server accepts an explicit per-connection concurrency limit.
#[test]
fn local_exec_server_accepts_concurrent_requests_flag() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    cmd.args([
        "exec-server",
        "--listen",
        "stdio",
        "--concurrent-requests",
        "2",
    ])
    .assert()
    .success();

    Ok(())
}

#[test]
fn local_exec_server_allows_disabled_parent_lifetime_environment_variable() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut cmd = codex_command(codex_home.path())?;
    cmd.env(
        codex_exec_server::CODEX_EXEC_SERVER_EXIT_ON_STDIN_CLOSE_ENV_VAR,
        "false",
    )
    .args(["exec-server", "--listen", "stdio"])
    .assert()
    .success();

    Ok(())
}

#[cfg(unix)]
#[test]
fn local_exec_server_exits_successfully_on_sigterm() -> Result<()> {
    let codex_home = TempDir::new()?;
    let mut child = std::process::Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .env("CODEX_HOME", codex_home.path())
        .args(["exec-server", "--listen", "ws://127.0.0.1:0"])
        .stdout(Stdio::piped())
        .spawn()?;
    let mut listen_url = String::new();
    StdBufReader::new(child.stdout.take().expect("child stdout")).read_line(&mut listen_url)?;
    assert!(listen_url.starts_with("ws://127.0.0.1:"), "{listen_url}");

    let listen_addr = listen_url
        .trim()
        .strip_prefix("ws://")
        .expect("listen URL should use ws://")
        .parse()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ready = false;
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        if let Ok(mut stream) =
            TcpStream::connect_timeout(&listen_addr, remaining.min(Duration::from_millis(100)))
        {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let request =
                format!("GET /readyz HTTP/1.1\r\nHost: {listen_addr}\r\nConnection: close\r\n\r\n");
            let mut response = String::new();
            if stream.write_all(request.as_bytes()).is_ok()
                && stream.read_to_string(&mut response).is_ok()
                && response.starts_with("HTTP/1.1 200")
            {
                ready = true;
                break;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready, "exec-server did not become ready at {listen_url}");

    // SAFETY: `child.id()` is the live process spawned above.
    let result = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    assert_eq!(result, 0);
    let status = child.wait()?;
    assert!(status.success(), "{status}");
    Ok(())
}
