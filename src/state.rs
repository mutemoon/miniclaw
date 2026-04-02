use crate::channels::wecom::WeComChannel;
use std::collections::HashMap;
use std::sync::Arc;

/// 每个 agent 的运行时实体
#[derive(Clone)]
pub struct AgentEntry {
    /// 企微 channel（如果配置了）
    pub wecom: Option<Arc<WeComChannel>>,
    /// 仓库根路径，claude 命令在此目录执行
    pub repo: String,
}

#[derive(Clone)]
pub struct AppState {
    /// key = agent name，对应路由 /wecom/<name>
    pub agents: HashMap<String, AgentEntry>,
}
