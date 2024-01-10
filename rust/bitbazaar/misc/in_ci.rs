static CI_ENV_VARS: [&str; 4] = ["GITHUB_ACTIONS", "TRAVIS", "CIRCLECI", "GITLAB_CI"];

/// Returns true if the current process seems to be running in CI.
pub fn in_ci() -> bool {
    CI_ENV_VARS
        .iter()
        .any(|var| std::env::var_os(var).is_some())
}
