use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionToken {
    pub user_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub caps: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterBeginRequest {
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterFinishRequest {
    pub username: String,
    pub credential: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthenticateBeginRequest {
    pub username: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthenticateFinishRequest {
    pub username: String,
    pub credential: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoverySetRequest {
    pub recovery_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecoveryAuthRequest {
    pub username: String,
    pub recovery_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        // SessionToken
        let session_token = SessionToken {
            user_id: "user-abc".to_string(),
            issued_at: 1700000000,
            expires_at: 1700003600,
            capabilities: vec!["auth".to_string(), "read".to_string()],
        };
        let json = serde_json::to_string(&session_token).expect("serialize SessionToken");
        let decoded: SessionToken = serde_json::from_str(&json).expect("deserialize SessionToken");
        assert_eq!(decoded.user_id, session_token.user_id);
        assert_eq!(decoded.issued_at, session_token.issued_at);
        assert_eq!(decoded.expires_at, session_token.expires_at);
        assert_eq!(decoded.capabilities, session_token.capabilities);

        // TokenClaims
        let claims = TokenClaims {
            sub: "user-xyz".to_string(),
            iat: 1700000000,
            exp: 1700003600,
            caps: vec!["write".to_string()],
        };
        let json = serde_json::to_string(&claims).expect("serialize TokenClaims");
        let decoded: TokenClaims = serde_json::from_str(&json).expect("deserialize TokenClaims");
        assert_eq!(decoded.sub, claims.sub);
        assert_eq!(decoded.iat, claims.iat);
        assert_eq!(decoded.exp, claims.exp);
        assert_eq!(decoded.caps, claims.caps);

        // RegisterBeginRequest
        let req = RegisterBeginRequest {
            username: "alice".to_string(),
        };
        let json = serde_json::to_string(&req).expect("serialize RegisterBeginRequest");
        let decoded: RegisterBeginRequest =
            serde_json::from_str(&json).expect("deserialize RegisterBeginRequest");
        assert_eq!(decoded.username, req.username);

        // RegisterFinishRequest
        let req = RegisterFinishRequest {
            username: "bob".to_string(),
            credential: serde_json::json!({"id": "cred-1", "type": "public-key"}),
        };
        let json = serde_json::to_string(&req).expect("serialize RegisterFinishRequest");
        let decoded: RegisterFinishRequest =
            serde_json::from_str(&json).expect("deserialize RegisterFinishRequest");
        assert_eq!(decoded.username, req.username);
        assert_eq!(decoded.credential, req.credential);

        // AuthenticateBeginRequest
        let req = AuthenticateBeginRequest {
            username: "carol".to_string(),
        };
        let json = serde_json::to_string(&req).expect("serialize AuthenticateBeginRequest");
        let decoded: AuthenticateBeginRequest =
            serde_json::from_str(&json).expect("deserialize AuthenticateBeginRequest");
        assert_eq!(decoded.username, req.username);

        // AuthenticateFinishRequest
        let req = AuthenticateFinishRequest {
            username: "dave".to_string(),
            credential: serde_json::json!({"response": {"authenticatorData": "abc"}}),
        };
        let json = serde_json::to_string(&req).expect("serialize AuthenticateFinishRequest");
        let decoded: AuthenticateFinishRequest =
            serde_json::from_str(&json).expect("deserialize AuthenticateFinishRequest");
        assert_eq!(decoded.username, req.username);
        assert_eq!(decoded.credential, req.credential);

        // RecoverySetRequest
        let req = RecoverySetRequest {
            recovery_key: "my-recovery-key-123".to_string(),
        };
        let json = serde_json::to_string(&req).expect("serialize RecoverySetRequest");
        let decoded: RecoverySetRequest =
            serde_json::from_str(&json).expect("deserialize RecoverySetRequest");
        assert_eq!(decoded.recovery_key, req.recovery_key);

        // RecoveryAuthRequest
        let req = RecoveryAuthRequest {
            username: "eve".to_string(),
            recovery_key: "recovery-456".to_string(),
        };
        let json = serde_json::to_string(&req).expect("serialize RecoveryAuthRequest");
        let decoded: RecoveryAuthRequest =
            serde_json::from_str(&json).expect("deserialize RecoveryAuthRequest");
        assert_eq!(decoded.username, req.username);
        assert_eq!(decoded.recovery_key, req.recovery_key);

        // TokenResponse
        let resp = TokenResponse {
            token: "eyJ...".to_string(),
            expires_at: 1700003600,
        };
        let json = serde_json::to_string(&resp).expect("serialize TokenResponse");
        let decoded: TokenResponse =
            serde_json::from_str(&json).expect("deserialize TokenResponse");
        assert_eq!(decoded.token, resp.token);
        assert_eq!(decoded.expires_at, resp.expires_at);

        // ErrorResponse
        let resp = ErrorResponse {
            error: "something went wrong".to_string(),
        };
        let json = serde_json::to_string(&resp).expect("serialize ErrorResponse");
        let decoded: ErrorResponse =
            serde_json::from_str(&json).expect("deserialize ErrorResponse");
        assert_eq!(decoded.error, resp.error);
    }
}
