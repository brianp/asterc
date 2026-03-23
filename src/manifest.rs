use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Build manifest — tracks what was compiled and whether artifacts are stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildManifest {
    pub compiler_version: String,
    pub profile: String,
    pub opt_level: String,
    pub target: String,
    pub files: HashMap<String, FileEntry>,
    pub runtime_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub source_hash: String,
    pub object_path: String,
    pub compiled_at: String,
}

impl BuildManifest {
    pub fn new(profile: &str, opt_level: &str) -> Self {
        Self {
            compiler_version: env!("CARGO_PKG_VERSION").to_string(),
            profile: profile.to_string(),
            opt_level: opt_level.to_string(),
            target: std::env::consts::ARCH.to_string(),
            files: HashMap::new(),
            runtime_hash: String::new(),
        }
    }

    /// Load manifest from a JSON file. Returns None if file doesn't exist or is corrupt.
    pub fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Save manifest to a JSON file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let data = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, data)
    }

    /// Check if the entire manifest is valid for the current compiler + settings.
    pub fn is_compatible(&self, profile: &str, opt_level: &str) -> bool {
        self.compiler_version == env!("CARGO_PKG_VERSION")
            && self.profile == profile
            && self.opt_level == opt_level
    }

    /// Check if a source file's object is up-to-date.
    pub fn is_file_fresh(&self, source_name: &str, source_hash: &str) -> bool {
        self.files
            .get(source_name)
            .is_some_and(|entry| entry.source_hash == source_hash)
    }

    /// Check if the runtime object is up-to-date.
    pub fn is_runtime_fresh(&self, runtime_hash: &str) -> bool {
        !self.runtime_hash.is_empty() && self.runtime_hash == runtime_hash
    }

    /// Record a compiled file.
    pub fn record_file(&mut self, source_name: &str, source_hash: &str, object_path: &str) {
        self.files.insert(
            source_name.to_string(),
            FileEntry {
                source_hash: source_hash.to_string(),
                object_path: object_path.to_string(),
                compiled_at: chrono_now(),
            },
        );
    }
}

fn chrono_now() -> String {
    // Simple timestamp without external crate
    format!("{:?}", std::time::SystemTime::now())
}

/// Compute SHA-256 hash of a byte slice, returned as hex string.
pub fn sha256_hex(data: &[u8]) -> String {
    use std::fmt::Write;
    // Minimal SHA-256 using the sha2 crate
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for byte in hash {
        write!(hex, "{:02x}", byte).unwrap();
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Contract tests --

    #[test]
    fn new_manifest_has_compiler_version() {
        let m = BuildManifest::new("debug", "none");
        assert_eq!(m.compiler_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(m.profile, "debug");
        assert_eq!(m.opt_level, "none");
    }

    #[test]
    fn new_manifest_starts_empty() {
        let m = BuildManifest::new("debug", "none");
        assert!(m.files.is_empty());
        assert!(m.runtime_hash.is_empty());
    }

    // -- Compatibility checks --

    #[test]
    fn compatible_manifest() {
        let m = BuildManifest::new("debug", "none");
        assert!(m.is_compatible("debug", "none"));
    }

    #[test]
    fn incompatible_profile() {
        let m = BuildManifest::new("debug", "none");
        assert!(!m.is_compatible("release", "none"));
    }

    #[test]
    fn incompatible_opt_level() {
        let m = BuildManifest::new("debug", "none");
        assert!(!m.is_compatible("debug", "speed"));
    }

    #[test]
    fn incompatible_compiler_version() {
        let mut m = BuildManifest::new("debug", "none");
        m.compiler_version = "0.0.0-old".to_string();
        assert!(!m.is_compatible("debug", "none"));
    }

    // -- File freshness --

    #[test]
    fn fresh_file_with_matching_hash() {
        let mut m = BuildManifest::new("debug", "none");
        m.record_file("main.aster", "abc123", "obj/main.o");
        assert!(m.is_file_fresh("main.aster", "abc123"));
    }

    #[test]
    fn stale_file_with_different_hash() {
        let mut m = BuildManifest::new("debug", "none");
        m.record_file("main.aster", "abc123", "obj/main.o");
        assert!(!m.is_file_fresh("main.aster", "different_hash"));
    }

    #[test]
    fn unknown_file_is_not_fresh() {
        let m = BuildManifest::new("debug", "none");
        assert!(!m.is_file_fresh("unknown.aster", "abc123"));
    }

    // -- Runtime freshness --

    #[test]
    fn fresh_runtime() {
        let mut m = BuildManifest::new("debug", "none");
        m.runtime_hash = "rt_hash".to_string();
        assert!(m.is_runtime_fresh("rt_hash"));
    }

    #[test]
    fn stale_runtime() {
        let mut m = BuildManifest::new("debug", "none");
        m.runtime_hash = "old_hash".to_string();
        assert!(!m.is_runtime_fresh("new_hash"));
    }

    #[test]
    fn empty_runtime_hash_is_stale() {
        let m = BuildManifest::new("debug", "none");
        assert!(!m.is_runtime_fresh("any_hash"));
    }

    // -- Serialization round-trip --

    #[test]
    fn save_and_load_roundtrip() {
        let tmp =
            std::env::temp_dir().join(format!("asterc_test_manifest_{}.json", std::process::id()));

        let mut m = BuildManifest::new("release", "speed");
        m.record_file("main.aster", "hash1", "obj/main.o");
        m.runtime_hash = "rt_hash".to_string();
        m.save(&tmp).unwrap();

        let loaded = BuildManifest::load(&tmp).unwrap();
        assert_eq!(loaded.profile, "release");
        assert_eq!(loaded.opt_level, "speed");
        assert!(loaded.is_file_fresh("main.aster", "hash1"));
        assert!(loaded.is_runtime_fresh("rt_hash"));

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_missing_file_returns_none() {
        let result = BuildManifest::load(Path::new("/tmp/nonexistent_asterc_manifest.json"));
        assert!(result.is_none());
    }

    #[test]
    fn load_corrupt_file_returns_none() {
        let tmp = std::env::temp_dir().join(format!(
            "asterc_test_corrupt_manifest_{}.json",
            std::process::id()
        ));
        std::fs::write(&tmp, "not valid json").unwrap();

        let result = BuildManifest::load(&tmp);
        assert!(result.is_none());

        let _ = std::fs::remove_file(&tmp);
    }

    // -- SHA-256 hashing --

    #[test]
    fn sha256_deterministic() {
        let h1 = sha256_hex(b"hello world");
        let h2 = sha256_hex(b"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn sha256_different_inputs() {
        let h1 = sha256_hex(b"hello");
        let h2 = sha256_hex(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn sha256_correct_length() {
        let h = sha256_hex(b"test");
        assert_eq!(h.len(), 64); // 32 bytes = 64 hex chars
    }
}
