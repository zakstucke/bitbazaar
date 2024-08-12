static CI_ENV_VARS: [&str; 4] = ["GITHUB_ACTIONS", "TRAVIS", "CIRCLECI", "GITLAB_CI"];

/// Returns true if the current process seems to be running in CI.
pub fn in_ci() -> bool {
    CI_ENV_VARS
        .iter()
        .any(|var| std::env::var_os(var).is_some())
}

/// Different common operating system types.
#[derive(Debug, PartialEq, Eq)]
pub enum OsType {
    /// Windows operating system.
    Windows,
    /// Linux operating system.
    Linux,
    /// MacOS operating system.
    Macos,
    /// Any other operating system not covered by the above.
    Unknown,
}

/// Returns the operating system type.
///
/// Note this uses the cfg!() based checks,
/// meaning may not be accurate after compilation for cross-compilation.
pub fn os_type() -> OsType {
    if cfg!(windows) {
        OsType::Windows
    } else if cfg!(unix) {
        if cfg!(target_os = "macos") {
            OsType::Macos
        } else {
            OsType::Linux
        }
    } else {
        OsType::Unknown
    }
}

/// Different common CPU architectures.
#[derive(Debug, PartialEq, Eq)]
pub enum Arch {
    /// 32-bit x86 architecture.
    X32,
    /// 64-bit x86 architecture.
    X64,
    /// Arm architecture.
    Arm,
    /// Any other architecture not covered by the above.
    Other(&'static str),
}

/// Returns the cpu architecture.
///
/// Note this uses the compile time architecture found in [`std::env::consts::ARCH`],
/// meaning may not be accurate after compilation for cross-compilation.
pub fn architecture_type() -> Arch {
    match std::env::consts::ARCH {
        "x86" => Arch::X32,
        "x86_64" => Arch::X64,
        "arm" | "aarch64" | "arm64" => Arch::Arm,
        other => Arch::Other(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testing::prelude::*;

    #[rstest]
    fn test_platform_os_type() {
        if cfg!(windows) {
            assert_eq!(OsType::Windows, os_type());
        } else if cfg!(unix) {
            if cfg!(target_os = "macos") {
                assert_eq!(OsType::Macos, os_type());
            } else {
                assert_eq!(OsType::Linux, os_type());
            }
        } else {
            assert_eq!(OsType::Unknown, os_type());
        }
    }

    #[rstest]
    fn test_platform_architecture_type() {
        // Just make sure anywhere being tested we get an actual arch back.
        if let Arch::Other(other) = architecture_type() {
            panic!("Unknown architecture: {}", other);
        }
    }
}
