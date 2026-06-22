pub fn calculate_total(items: &[f64]) -> f64 {
    items.iter().sum()
}

pub struct AppConfig {
    pub name: String,
}

pub fn init_config() -> AppConfig {
    AppConfig { name: "app".into() }
}
