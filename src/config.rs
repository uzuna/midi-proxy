use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub connection: Vec<TcpLinkInfo>,
}

/// TCP接続情報を表す構造体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TcpLinkInfo {
    pub addr: String,
    pub port: u16,
}
