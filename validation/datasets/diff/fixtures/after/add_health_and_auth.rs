pub fn calculate_total(items: &[f64]) -> f64 {
    items.iter().sum()
}

pub struct AppConfig {
    pub name: String,
}

pub fn init_config() -> AppConfig {
    AppConfig { name: "app".into() }
}

pub fn health_check() -> String {
    "ok".to_string()
}

pub fn login(username: String, password: String) -> Result<Token, AuthError> {
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
