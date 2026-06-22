pub fn hash_password(raw: &str) -> String {
    raw.to_string()
}

pub fn generate_token(user: &str) -> String {
    format!("token_{}", user)
}

pub fn verify_token(token: &str) -> bool {
    !token.is_empty()
}
