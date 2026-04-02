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

pub async fn run_claude_process(
    agent_name: &str,
    prompt: &str,
    repo_dir: &str,
) -> anyhow::Result<String> {
    tracing::info!(
        "Executing Claude CLI for agent \"{}\" in {}: \"{}\"",
        agent_name,
        repo_dir,
        truncate_with_ellipsis(prompt, 50)
    );

    let mut cmd = tokio::process::Command::new("claude");
    cmd.arg("-p")
        .arg(prompt)
        .arg("--dangerously-skip-permissions");

    // 1. Check project level prompt: repo_dir/.claude/claw/AGENTS.md
    let repo_prompt_path = std::path::Path::new(repo_dir)
        .join(".claude")
        .join("claw")
        .join("AGENTS.md");
    if repo_prompt_path.exists() {
        tracing::info!("Loading project system prompt: {:?}", repo_prompt_path);
        cmd.arg("--append-system-prompt-file")
            .arg(&repo_prompt_path);
    }

    // 2. Check user home level prompt: ~/.claude/claw/[agent_name].md
    let home_dir = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let home_prompt_dir = home_dir.join(".claude").join("claw");
    let home_prompt_path = home_prompt_dir.join(format!("{}.md", agent_name));

    if home_prompt_path.exists() {
        tracing::info!("Loading user system prompt: {:?}", home_prompt_path);
        cmd.arg("--append-system-prompt-file")
            .arg(&home_prompt_path);
    } else {
        // If neither exists, create home level prompt
        if !repo_prompt_path.exists() {
            tracing::info!(
                "No system prompt found. Creating default one at {:?}",
                home_prompt_path
            );
            if !home_prompt_dir.exists() {
                std::fs::create_dir_all(&home_prompt_dir)?;
            }
            std::fs::write(
                &home_prompt_path,
                "You are an AI assistant and a powerful coding tool.",
            )?;
            cmd.arg("--append-system-prompt-file")
                .arg(&home_prompt_path);
        }
    }

    let output = cmd.current_dir(repo_dir).output().await?;

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
