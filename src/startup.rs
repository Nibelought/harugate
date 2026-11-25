//! What to tell someone who double-clicked harugate and it did not start.
//!
//! A console window opened by a double-click closes the moment the process exits (Windows),
//! and a bare stack of log lines means little to most people, so a failed start prints a
//! plain message, and on Windows waits for Enter so the message can be read.

use std::io::BufRead;

use crate::server::{PortInUse, local_url};

/// A failed start, explained: the text to show and, if the UI is probably up already,
/// its address.
pub struct Report {
    pub message: String,
    pub open_url: Option<String>,
}

/// Turn the error from `server::run` into something a person can act on.
pub fn explain(err: &anyhow::Error) -> Report {
    if let Some(PortInUse { addr }) = err.downcast_ref::<PortInUse>() {
        let url = local_url(*addr, "/ui");
        return Report {
            message: format!(
                "harugate can't start: port {port} is already in use.\n\n\
                 harugate is probably already running (look for another harugate window).\n\
                 Its page: {url}\n\n\
                 If another program uses that port, change `listen` in the config file.",
                port = addr.port()
            ),
            open_url: Some(url),
        };
    }
    Report {
        message: format!(
            "harugate couldn't start:\n\n  {err:#}\n\n\
             If this names the config file, fix that file and start harugate again."
        ),
        open_url: None,
    }
}

/// Print the explanation, open the existing UI if asked to, optionally wait for Enter,
/// and exit with status 1.
pub fn fail(err: &anyhow::Error, open_browser: bool, pause: bool) -> ! {
    let report = explain(err);
    eprintln!("\n{}\n", report.message);
    if let (true, Some(url)) = (open_browser, &report.open_url) {
        let _ = webbrowser::open(url);
    }
    if pause {
        eprintln!("Press Enter to close this window.");
        let _ = std::io::stdin().lock().read_line(&mut String::new());
    }
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server;

    fn config_listening_on(
        dir: &tempfile::TempDir,
        addr: std::net::SocketAddr,
    ) -> std::path::PathBuf {
        let path = dir.path().join("config.toml");
        std::fs::write(&path, format!("listen = \"{addr}\"\n")).unwrap();
        path
    }

    #[tokio::test]
    async fn a_busy_port_is_reported_with_the_ui_address() {
        let taken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = taken.local_addr().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = config_listening_on(&dir, addr);

        let err = server::run(Some(path), false).await.unwrap_err();
        let report = explain(&err);
        assert!(
            report.message.contains("already in use"),
            "{}",
            report.message
        );
        assert_eq!(report.open_url, Some(format!("http://{addr}/ui")));
        assert!(report.message.contains(&format!("http://{addr}/ui")));
    }

    #[tokio::test]
    async fn a_broken_config_names_the_problem_and_has_no_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "routing = \"fastest\"\n").unwrap();

        let err = server::run(Some(path.clone()), false).await.unwrap_err();
        let report = explain(&err);
        assert!(report.open_url.is_none());
        assert!(
            report.message.contains("couldn't start"),
            "{}",
            report.message
        );
        assert!(report.message.contains("fastest"), "{}", report.message);
    }

    #[test]
    fn an_unrelated_error_is_passed_through() {
        let err = anyhow::anyhow!("disk on fire");
        let report = explain(&err);
        assert!(report.message.contains("disk on fire"));
    }
}
