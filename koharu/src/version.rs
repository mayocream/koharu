pub const APP_VERSION: &str = match option_env!("KOHARU_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn current() -> &'static str {
    APP_VERSION
}
