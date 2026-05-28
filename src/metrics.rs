use std::{collections::HashMap, process::Stdio};

use anyhow::{Context, Result};
use tokio::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct PaneMetrics {
    pub pid: u32,
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub elapsed: String,
    pub command: String,
}

pub async fn collect(pids: &[u32]) -> Result<HashMap<u32, PaneMetrics>> {
    if pids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut command = Command::new("ps");
    command
        .arg("-o")
        .arg("pid=")
        .arg("-o")
        .arg("%cpu=")
        .arg("-o")
        .arg("%mem=")
        .arg("-o")
        .arg("etime=")
        .arg("-o")
        .arg("comm=");

    for pid in pids {
        command.arg("-p").arg(pid.to_string());
    }

    command
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());

    let output = command.output().await.context("failed to execute `ps`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("`ps` failed: {stderr}");
    }

    let stdout = String::from_utf8(output.stdout).context("ps output was not valid UTF-8")?;
    let mut metrics = HashMap::new();

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        if let Some(metric) = parse_ps_line(line) {
            metrics.insert(metric.pid, metric);
        }
    }

    Ok(metrics)
}

fn parse_ps_line(line: &str) -> Option<PaneMetrics> {
    let mut fields = line.split_whitespace();
    let pid = fields.next()?.parse::<u32>().ok()?;
    let cpu_percent = fields.next()?.parse::<f32>().ok()?;
    let mem_percent = fields.next()?.parse::<f32>().ok()?;
    let elapsed = fields.next()?.to_owned();
    let command = fields.collect::<Vec<_>>().join(" ");

    Some(PaneMetrics {
        pid,
        cpu_percent,
        mem_percent,
        elapsed,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::{collect, parse_ps_line};

    #[test]
    fn parses_ps_line() {
        let metric = parse_ps_line("4242 12.5 1.7 01:23:45 ssh").expect("ps line should parse");

        assert_eq!(metric.pid, 4242);
        assert_eq!(metric.cpu_percent, 12.5);
        assert_eq!(metric.mem_percent, 1.7);
        assert_eq!(metric.elapsed, "01:23:45");
        assert_eq!(metric.command, "ssh");
    }

    #[test]
    fn parses_ps_line_with_multi_word_command() {
        let metric =
            parse_ps_line("4242 0.0 0.1 12-01:02:03 python worker.py").expect("line parses");

        assert_eq!(metric.pid, 4242);
        assert_eq!(metric.cpu_percent, 0.0);
        assert_eq!(metric.mem_percent, 0.1);
        assert_eq!(metric.elapsed, "12-01:02:03");
        assert_eq!(metric.command, "python worker.py");
    }

    #[test]
    fn parse_ps_line_rejects_missing_or_invalid_fields() {
        assert!(parse_ps_line("").is_none());
        assert!(parse_ps_line("not-a-pid 1.0 2.0 00:01 bash").is_none());
        assert!(parse_ps_line("4242 bad 2.0 00:01 bash").is_none());
        assert!(parse_ps_line("4242 1.0 bad 00:01 bash").is_none());
        assert!(parse_ps_line("4242 1.0 2.0").is_none());
    }

    #[tokio::test]
    async fn collect_empty_pid_list_does_not_spawn_ps() {
        let metrics = collect(&[]).await.expect("empty collect should succeed");

        assert!(metrics.is_empty());
    }

    #[tokio::test]
    async fn collect_current_process_metrics_when_ps_is_available() {
        let pid = std::process::id();
        let metrics = collect(&[pid])
            .await
            .expect("ps should collect current process metrics");

        let metric = metrics
            .get(&pid)
            .expect("current process should have a metrics row");
        assert_eq!(metric.pid, pid);
        assert!(metric.cpu_percent >= 0.0);
        assert!(metric.mem_percent >= 0.0);
        assert!(!metric.elapsed.is_empty());
    }
}
