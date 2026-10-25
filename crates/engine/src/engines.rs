//! Installable llama.cpp builds (`catalog/engines.toml`).
//!
//! ```text
//! <engines dir>/
//!   llama-cpp-b11669-windows-x86_64-vulkan/   extracted archive(s) + `.installed` marker
//!   downloads/                                archives while downloading (deleted after)
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use infermark_models::download::{self, DownloadError, DownloadPolicy, Progress};
use infermark_sdk::{GpuVendor, HardwareInfo};
use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("../../../catalog/engines.toml");
const MARKER: &str = ".installed";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EngineCatalog {
    pub schema: u32,
    #[serde(rename = "engine")]
    pub engines: Vec<EngineBuild>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EngineBuild {
    pub id: String,
    /// llama.cpp build tag, e.g. `b11669`.
    pub build: String,
    /// `std::env::consts::OS` value.
    pub os: String,
    /// `std::env::consts::ARCH` value.
    pub arch: String,
    pub accel: Accel,
    #[serde(rename = "archive")]
    pub archives: Vec<Archive>,
}

impl EngineBuild {
    pub fn download_bytes(&self) -> u64 {
        self.archives.iter().map(|a| a.size_bytes).sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Accel {
    Cpu,
    Vulkan,
    Cuda,
    Metal,
}

impl Accel {
    /// The id used in the catalog and on the command line.
    pub fn id(self) -> &'static str {
        match self {
            Accel::Cpu => "cpu",
            Accel::Vulkan => "vulkan",
            Accel::Cuda => "cuda",
            Accel::Metal => "metal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Accel::Cpu => "CPU",
            Accel::Vulkan => "Vulkan (GPU)",
            Accel::Cuda => "CUDA (NVIDIA GPU)",
            Accel::Metal => "Metal (Apple GPU)",
        }
    }
}

impl EngineCatalog {
    pub fn builtin() -> Self {
        let c: Self = toml::from_str(BUILTIN).expect("built-in engine catalog is valid");
        assert_eq!(c.schema, 1);
        c
    }

    /// Builds that run on this OS and CPU architecture.
    pub fn for_this_platform(&self) -> Vec<&EngineBuild> {
        self.engines
            .iter()
            .filter(|e| e.os == std::env::consts::OS && e.arch == std::env::consts::ARCH)
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<&EngineBuild> {
        self.engines.iter().find(|e| e.id == id)
    }

    /// The best build for this device: Metal on Apple silicon, CUDA with an NVIDIA GPU,
    /// Vulkan with any other GPU, otherwise CPU.
    pub fn recommended(&self, hw: &HardwareInfo) -> Option<&EngineBuild> {
        let builds = self.for_this_platform();
        let has = |a: Accel| builds.iter().copied().find(|e| e.accel == a);
        let nvidia = hw
            .gpus
            .iter()
            .any(|g| g.vendor == GpuVendor::Nvidia && !g.integrated);
        has(Accel::Metal)
            .or_else(|| if nvidia { has(Accel::Cuda) } else { None })
            .or_else(|| {
                if hw.gpus.is_empty() {
                    None
                } else {
                    has(Accel::Vulkan)
                }
            })
            .or_else(|| has(Accel::Cpu))
    }
}

#[derive(Debug, Clone)]
pub struct EngineStore {
    root: PathBuf,
}

impl EngineStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn dir(&self, build: &EngineBuild) -> PathBuf {
        self.root.join(&build.id)
    }

    /// Path to `llama-server` if this build is installed.
    pub fn server_path(&self, build: &EngineBuild) -> Option<PathBuf> {
        let dir = self.dir(build);
        if !dir.join(MARKER).exists() {
            return None;
        }
        find_file(&dir, server_file_name(), 3)
    }

    /// Installed builds for this platform, best acceleration first.
    pub fn installed<'c>(&self, catalog: &'c EngineCatalog) -> Vec<(&'c EngineBuild, PathBuf)> {
        let mut found: Vec<_> = catalog
            .for_this_platform()
            .into_iter()
            .filter_map(|b| self.server_path(b).map(|p| (b, p)))
            .collect();
        found.sort_by_key(|(b, _)| std::cmp::Reverse(b.accel));
        found
    }

    /// Download, verify and extract a build. `on_progress` gets the archive index too, for
    /// builds that come in more than one archive (CUDA).
    pub fn install(
        &self,
        build: &EngineBuild,
        policy: &DownloadPolicy,
        on_progress: &mut dyn FnMut(usize, usize, Progress),
        cancel: &AtomicBool,
    ) -> Result<PathBuf, DownloadError> {
        if let Some(path) = self.server_path(build) {
            return Ok(path);
        }
        let downloads = self.root.join("downloads");
        let staging = self.root.join(format!("{}.tmp", build.id));
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        let count = build.archives.len();
        for (i, archive) in build.archives.iter().enumerate() {
            let name = archive.url.rsplit('/').next().unwrap_or("engine.zip");
            let file = downloads.join(name);
            download::download(
                &archive.url,
                &file,
                archive.size_bytes,
                &archive.sha256,
                policy,
                &mut |p| on_progress(i, count, p),
                cancel,
            )?;
            download::extract(&file, &staging)?;
            fs::remove_file(&file)?;
        }
        fs::write(staging.join(MARKER), &build.id)?;
        let dir = self.dir(build);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        fs::rename(&staging, &dir)?;
        self.server_path(build).ok_or_else(|| {
            DownloadError::Archive("the archive does not contain llama-server".into())
        })
    }

    pub fn remove(&self, build: &EngineBuild) -> std::io::Result<()> {
        let dir = self.dir(build);
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}

pub fn server_file_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// `llama-server` on the PATH, for people who build llama.cpp themselves.
pub fn server_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(server_file_name()))
        .find(|p| p.is_file())
}

fn find_file(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    let candidate = dir.join(name);
    if candidate.is_file() {
        return Some(candidate);
    }
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    subdirs.sort();
    subdirs
        .into_iter()
        .find_map(|d| find_file(&d, name, depth - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_valid() {
        let c = EngineCatalog::builtin();
        let mut ids = std::collections::HashSet::new();
        for e in &c.engines {
            assert!(ids.insert(&e.id), "duplicate {}", e.id);
            assert!(!e.archives.is_empty(), "{}", e.id);
            for a in &e.archives {
                assert!(
                    a.url
                        .starts_with("https://github.com/ggml-org/llama.cpp/releases/download/"),
                    "{}",
                    a.url
                );
                assert_eq!(a.sha256.len(), 64);
                assert!(a.size_bytes > 0);
            }
        }
        // Every desktop platform we ship for has a CPU build.
        for (os, arch) in [
            ("windows", "x86_64"),
            ("linux", "x86_64"),
            ("macos", "aarch64"),
        ] {
            assert!(
                c.engines.iter().any(|e| e.os == os && e.arch == arch),
                "{os} {arch}"
            );
        }
    }

    #[test]
    fn finds_server_in_nested_folder() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("build").join("bin");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join(server_file_name()), b"").unwrap();
        assert_eq!(
            find_file(dir.path(), server_file_name(), 3),
            Some(nested.join(server_file_name()))
        );
        assert_eq!(find_file(dir.path(), server_file_name(), 1), None);
    }

    #[test]
    fn install_marker_is_required() {
        let dir = tempfile::tempdir().unwrap();
        let store = EngineStore::new(dir.path());
        let c = EngineCatalog::builtin();
        let b = &c.engines[0];
        fs::create_dir_all(store.dir(b)).unwrap();
        fs::write(store.dir(b).join(server_file_name()), b"").unwrap();
        assert!(
            store.server_path(b).is_none(),
            "half-installed build must not be used"
        );
        fs::write(store.dir(b).join(MARKER), &b.id).unwrap();
        assert!(store.server_path(b).is_some());
    }
}
