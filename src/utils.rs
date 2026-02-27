use rust_i18n::t;

pub fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// 在指定仓库目录下调用 claude CLI 执行 prompt
pub async fn run_claude_process(prompt: &str, repo_dir: &str) -> anyhow::Result<String> {
    let output = tokio::process::Command::new("claude")
        .arg("-p")
        .arg(prompt)
        .arg("--dangerously-skip-permissions")
        .current_dir(repo_dir)
        .output()
        .await?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(t!("claude_process_failed", error = err))
    }
}
