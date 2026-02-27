pub fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

pub async fn run_claude_process(prompt: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("claude")
        .arg(prompt)
        .output()
        .await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Claude process failed: {}", err)
    }
}
