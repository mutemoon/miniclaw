use crate::wecom::WeComChannel;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub wecom: Option<Arc<WeComChannel>>,
}
