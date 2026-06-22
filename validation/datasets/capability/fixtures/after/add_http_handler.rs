pub fn handle_get_users() -> Vec<String> {
    vec!["user1".into(), "user2".into()]
}

pub fn handle_get_user(id: u64) -> Option<String> {
    if id == 0 { None } else { Some("user".into()) }
}
