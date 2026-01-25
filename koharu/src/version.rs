pub const APP_VERSION: &str = git_version::git_version!(
    args = ["--always", "--dirty=-dirty", "--tags"],
    fallback = "unknown"
);

pub fn current() -> &'static str {
    APP_VERSION
}
