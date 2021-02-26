use serde_json::Value;

#[derive(Debug, Clone)]
pub struct AppState {
}

impl AppState {
    pub async fn new_from_config(_config: &Value) -> Self {
        Self {}
    }
}