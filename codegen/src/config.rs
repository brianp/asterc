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

    /// Build the shared Cranelift ISA settings flags for this configuration.
    ///
    /// `is_pic` selects position-independent code (true for the AOT object
    /// backend, false for the JIT). Frame pointers are always preserved so a
    /// frame-pointer walk at throw time can traverse the native stack cleanly
    /// in both backends; this is load-bearing for captured stack traces.
    pub fn cranelift_flags(&self, is_pic: bool) -> cranelift_codegen::settings::Flags {
        use cranelift_codegen::settings::{self, Configurable};
        let mut b = settings::builder();
        b.set("opt_level", self.cranelift_opt_level()).unwrap();
        b.set("is_pic", if is_pic { "true" } else { "false" })
            .unwrap();
        // Force frame pointers on: the stack-trace walker relies on an intact
        // frame-pointer chain from every Aster and runtime frame.
        b.set("preserve_frame_pointers", "true").unwrap();
        settings::Flags::new(b)
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

    // -- Frame pointer / cranelift flag tests --

    #[test]
    fn test_cranelift_flags_force_frame_pointers_jit() {
        // The JIT backend (is_pic = false) must force frame pointers on so a
        // frame-pointer walk at throw time can traverse the native stack.
        let flags = BuildConfig::release().cranelift_flags(false);
        assert!(
            flags.preserve_frame_pointers(),
            "JIT ISA flags must preserve frame pointers"
        );
    }

    #[test]
    fn test_cranelift_flags_force_frame_pointers_aot() {
        // The AOT backend (is_pic = true) must force frame pointers on too so
        // AOT and JIT frames both walk cleanly.
        let flags = BuildConfig::release().cranelift_flags(true);
        assert!(
            flags.preserve_frame_pointers(),
            "AOT ISA flags must preserve frame pointers"
        );
    }

    #[test]
    fn test_cranelift_flags_opt_level_threaded_through() {
        let mut config = BuildConfig::release();
        config.opt_level = OptLevel::SpeedAndSize;
        let flags = config.cranelift_flags(false);
        assert_eq!(
            flags.opt_level(),
            cranelift_codegen::settings::OptLevel::SpeedAndSize
        );
    }
}
