use serde::{Deserialize, Serialize};

/// JWT claims — identical shape to auth-worker so tokens minted there validate
/// here (used for both the bearer gate and the `sub` used for the subscription DO).
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub caps: Vec<String>,
    #[serde(default)]
    pub token_id: Option<String>,
}
