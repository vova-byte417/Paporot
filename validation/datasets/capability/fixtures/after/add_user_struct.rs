pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
    pub role: Role,
}

pub enum Role {
    Admin,
    User,
    Guest,
}

pub fn create_user(name: String, email: String) -> User {
    User { id: 1, name, email, role: Role::User }
}

pub fn promote_to_admin(user: &mut User) {
    user.role = Role::Admin;
}
