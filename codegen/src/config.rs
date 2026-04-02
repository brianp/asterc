//! Build configuration for the Aster compiler.

/// Cranelift optimization level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    /// No optimization — fastest compilation (debug default).
    None,
    /// Optimize for runtime speed (release default).
    Speed,
    /// Optimize for binary size.
    SpeedAndSize,
}

/// Build profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

/// Full build configuration.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    pub opt_level: OptLevel,
    pub profile: Profile,
    pub debug_info: bool,
    pub verbose: bool,
    /// When true, the build includes JIT support for `evaluate()` and `jit_run()`.
    pub jit: bool,
}

impl BuildConfig {
    /// Debug profile defaults: no optimization, debug info on.
    pub fn debug() -> Self {
        Self {
            opt_level: OptLevel::None,
            profile: Profile::Debug,
            debug_info: true,
            verbose: false,
            jit: false,
        }
    }

    /// Release profile defaults: speed optimization, debug info off.
    pub fn release() -> Self {
        Self {
            opt_level: OptLevel::Speed,
            profile: Profile::Release,
            debug_info: false,
            verbose: false,
            jit: false,
        }
    }

    /// Returns the Cranelift `opt_level` setting string.
    pub fn cranelift_opt_level(&self) -> &'static str {
        match self.opt_level {
            OptLevel::None => "none",
            OptLevel::Speed => "speed",
            OptLevel::SpeedAndSize => "speed_and_size",
        }
    }

    /// Returns the profile directory name.
    pub fn profile_dir(&self) -> &'static str {
        match self.profile {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self::debug()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Contract tests --

    #[test]
    fn debug_config_defaults() {
        let config = BuildConfig::debug();
        assert_eq!(config.opt_level, OptLevel::None);
        assert_eq!(config.profile, Profile::Debug);
        assert!(config.debug_info);
        assert!(!config.verbose);
        assert!(!config.jit);
    }

    #[test]
    fn release_config_defaults() {
        let config = BuildConfig::release();
        assert_eq!(config.opt_level, OptLevel::Speed);
        assert_eq!(config.profile, Profile::Release);
        assert!(!config.debug_info);
        assert!(!config.verbose);
        assert!(!config.jit);
    }

    #[test]
    fn jit_flag_can_be_set() {
        let mut config = BuildConfig::debug();
        config.jit = true;
        assert!(config.jit);
    }

    #[test]
    fn default_is_debug() {
        let config = BuildConfig::default();
        assert_eq!(config.profile, Profile::Debug);
    }

    // -- Cranelift mapping tests --

    #[test]
    fn cranelift_opt_level_mapping() {
        assert_eq!(BuildConfig::debug().cranelift_opt_level(), "none");
        assert_eq!(BuildConfig::release().cranelift_opt_level(), "speed");

        let mut config = BuildConfig::release();
        config.opt_level = OptLevel::SpeedAndSize;
        assert_eq!(config.cranelift_opt_level(), "speed_and_size");
    }

    // -- Profile directory tests --

    #[test]
    fn profile_dir_names() {
        assert_eq!(BuildConfig::debug().profile_dir(), "debug");
        assert_eq!(BuildConfig::release().profile_dir(), "release");
    }
}
