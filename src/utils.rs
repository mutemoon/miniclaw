use rust_i18n::t;

pub fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        // 使用 chars().take() 来确保在字符边界上截断
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}...", truncated)
    }
}

pub async fn run_claude_process(prompt: &str, repo_dir: &str) -> anyhow::Result<String> {
    tracing::info!(
        "Executing Claude CLI in {}: \"{}\"",
        repo_dir,
        truncate_with_ellipsis(prompt, 50)
    );

    let output = tokio::process::Command::new("claude")
        .arg("-p")
        .arg(prompt)
        .arg("--dangerously-skip-permissions")
        .arg("--append-system-prompt-file")
        .arg(".claude/claw/AGENTS.md")
        .current_dir(repo_dir)
        .output()
        .await?;

    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).to_string();
        tracing::info!(
            "Claude CLI response ({} chars): \"{}\"",
            result.len(),
            truncate_with_ellipsis(&result, 100)
        );
        Ok(result)
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(t!("claude_process_failed", error = err))
    }
}
