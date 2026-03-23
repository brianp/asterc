use std::path::{Path, PathBuf};

use codegen::config::Profile;

/// Resolved paths for a build.
#[derive(Debug, Clone)]
pub struct BuildPaths {
    /// The build root: `.aster/build/<profile>/`
    pub root: PathBuf,
    /// Object file directory: `<root>/obj/`
    pub obj_dir: PathBuf,
    /// Generated file directory: `<root>/gen/`
    pub gen_dir: PathBuf,
    /// Binary output directory: `<root>/bin/`
    pub bin_dir: PathBuf,
}

impl BuildPaths {
    /// Object file path for a given source file.
    pub fn object_for(&self, source_name: &str) -> PathBuf {
        let stem = Path::new(source_name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        self.obj_dir.join(format!("{}.o", stem))
    }

    /// Binary path for a given source file.
    pub fn binary_for(&self, source_name: &str) -> PathBuf {
        let stem = Path::new(source_name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        self.bin_dir.join(stem.to_string())
    }

    /// Runtime C source path.
    pub fn runtime_c(&self) -> PathBuf {
        self.gen_dir.join("runtime.c")
    }

    /// Compiled runtime object path.
    pub fn runtime_o(&self) -> PathBuf {
        self.gen_dir.join("runtime.o")
    }

    /// Manifest path.
    pub fn manifest(&self) -> PathBuf {
        self.root.join("manifest.json")
    }

    /// Create all directories (lazily called before first write).
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.obj_dir)?;
        std::fs::create_dir_all(&self.gen_dir)?;
        std::fs::create_dir_all(&self.bin_dir)?;
        Ok(())
    }
}

/// Find the project root by walking up from `source_path`.
///
/// Looks for `.aster/` first, then `.git/`. Falls back to the source file's
/// parent directory.
pub fn find_project_root(source_path: &Path) -> PathBuf {
    let start = if source_path.is_file() {
        source_path.parent().unwrap_or(Path::new("."))
    } else {
        source_path
    };

    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".aster").is_dir() {
            return dir;
        }
        if dir.join(".git").exists() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }

    // Fallback: source file's parent directory
    start.to_path_buf()
}

/// Resolve build paths for a given source file and profile.
///
/// If `build_dir_override` is Some, use that as the build root directly.
/// Otherwise, find the project root and use `.aster/build/<profile>/`.
pub fn resolve_build_paths(
    source_path: &Path,
    profile: Profile,
    build_dir_override: Option<&Path>,
) -> BuildPaths {
    let profile_name = match profile {
        Profile::Debug => "debug",
        Profile::Release => "release",
    };

    let root = if let Some(override_dir) = build_dir_override {
        override_dir.join(profile_name)
    } else {
        let project_root = find_project_root(source_path);
        project_root.join(".aster").join("build").join(profile_name)
    };

    BuildPaths {
        obj_dir: root.join("obj"),
        gen_dir: root.join("gen"),
        bin_dir: root.join("bin"),
        root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- BuildPaths derived paths --

    #[test]
    fn object_for_strips_extension() {
        let paths = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Debug,
            Some(Path::new("/tmp/build")),
        );
        assert_eq!(paths.object_for("main.aster"), paths.obj_dir.join("main.o"));
    }

    #[test]
    fn binary_for_strips_extension() {
        let paths = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Debug,
            Some(Path::new("/tmp/build")),
        );
        assert_eq!(paths.binary_for("main.aster"), paths.bin_dir.join("main"));
    }

    #[test]
    fn runtime_paths() {
        let paths = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Debug,
            Some(Path::new("/tmp/build")),
        );
        assert_eq!(paths.runtime_c(), paths.gen_dir.join("runtime.c"));
        assert_eq!(paths.runtime_o(), paths.gen_dir.join("runtime.o"));
    }

    #[test]
    fn manifest_path() {
        let paths = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Debug,
            Some(Path::new("/tmp/build")),
        );
        assert_eq!(paths.manifest(), paths.root.join("manifest.json"));
    }

    // -- Profile directory separation --

    #[test]
    fn debug_profile_uses_debug_dir() {
        let paths = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Debug,
            Some(Path::new("/tmp/build")),
        );
        assert!(paths.root.ends_with("debug"));
    }

    #[test]
    fn release_profile_uses_release_dir() {
        let paths = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Release,
            Some(Path::new("/tmp/build")),
        );
        assert!(paths.root.ends_with("release"));
    }

    #[test]
    fn debug_and_release_are_separate_dirs() {
        let debug = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Debug,
            Some(Path::new("/tmp/build")),
        );
        let release = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Release,
            Some(Path::new("/tmp/build")),
        );
        assert_ne!(debug.root, release.root);
    }

    // -- Build dir override --

    #[test]
    fn build_dir_override() {
        let paths = resolve_build_paths(
            Path::new("/tmp/test/main.aster"),
            Profile::Debug,
            Some(Path::new("/custom/build")),
        );
        assert!(paths.root.starts_with("/custom/build"));
    }

    // -- Default path uses .aster/build/ --

    #[test]
    fn default_build_dir_under_aster() {
        // Use a temp dir with .git marker
        let tmp = std::env::temp_dir().join(format!("asterc_test_build_dir_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let _ = std::fs::create_dir_all(tmp.join(".git"));
        let source = tmp.join("main.aster");
        let _ = std::fs::write(&source, "def main() -> Int\n  42\n");

        let paths = resolve_build_paths(&source, Profile::Debug, None);
        assert!(paths.root.to_string_lossy().contains(".aster/build/debug"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // -- Project root detection --

    #[test]
    fn find_root_prefers_aster_dir() {
        let tmp = std::env::temp_dir().join(format!("asterc_test_root_pref_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let _ = std::fs::create_dir_all(tmp.join(".aster"));
        let _ = std::fs::create_dir_all(tmp.join(".git"));

        let root = find_project_root(&tmp.join("main.aster"));
        assert_eq!(root, tmp);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn find_root_falls_back_to_git() {
        let tmp = std::env::temp_dir().join(format!("asterc_test_root_git_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let _ = std::fs::create_dir_all(tmp.join(".git"));

        let root = find_project_root(&tmp.join("main.aster"));
        assert_eq!(root, tmp);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    // -- ensure_dirs --

    #[test]
    fn ensure_dirs_creates_subdirs() {
        let tmp = std::env::temp_dir().join(format!("asterc_test_ensure_dirs_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        let paths = resolve_build_paths(Path::new("/tmp/main.aster"), Profile::Debug, Some(&tmp));
        paths.ensure_dirs().unwrap();

        assert!(paths.obj_dir.is_dir());
        assert!(paths.gen_dir.is_dir());
        assert!(paths.bin_dir.is_dir());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
