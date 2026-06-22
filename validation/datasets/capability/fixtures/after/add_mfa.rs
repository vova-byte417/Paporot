pub fn health_check() -> String {
    "ok".to_string()
}

pub fn login(username: String, password: String) -> Result<Token, AuthError> {
    Ok(Token::new(username))
}

pub fn login_with_mfa(username: String, password: String, mfa_code: String) -> Result<Token, AuthError> {
    if mfa_code.is_empty() {
        return Err(AuthError { message: "MFA required".into() });
    }
    Ok(Token::new(username))
}

pub struct Token {
    pub value: String,
    pub expires_at: i64,
}

pub struct AuthError {
    pub message: String,
}

impl Token {
    pub fn new(username: String) -> Self {
        Token { value: format!("token_{}", username), expires_at: 0 }
    }
}
