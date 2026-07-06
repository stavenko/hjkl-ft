use serde::{Deserialize, Serialize};

/// JWT claims — identical shape to auth-worker so tokens minted there validate
/// here. Only `sub` is used by this worker (one SyncDO per `sub`).
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    #[serde(default)]
    pub caps: Vec<String>,
    #[serde(default)]
    pub token_id: Option<String>,
}
