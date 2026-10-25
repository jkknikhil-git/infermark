//! The local models folder.
//!
//! ```text
//! <models dir>/
//!   Qwen3-0.6B-Q4_K_M.gguf            a verified model
//!   Qwen3-0.6B-Q4_K_M.gguf.verified   its SHA-256, written only after the hash matched
//!   Llama-3.2-1B-Instruct-Q4_K_M.gguf.part   an interrupted download (resumed next time)
//! ```

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::catalog::CatalogModel;
use crate::download::{self, DownloadError, DownloadPolicy, Progress};

#[derive(Debug, Clone)]
pub struct ModelStore {
    root: PathBuf,
}

impl ModelStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, model: &CatalogModel) -> PathBuf {
        self.root.join(model.file_name())
    }

    fn marker(&self, model: &CatalogModel) -> PathBuf {
        self.root.join(format!("{}.verified", model.file_name()))
    }

    /// Downloaded, the right size, and its hash was checked.
    pub fn is_installed(&self, model: &CatalogModel) -> bool {
        let size_ok = fs::metadata(self.path(model))
            .map(|m| m.len() == model.size_bytes)
            .unwrap_or(false);
        let hash_ok = fs::read_to_string(self.marker(model))
            .map(|s| s.trim() == model.sha256)
            .unwrap_or(false);
        size_ok && hash_ok
    }

    /// Bytes already downloaded for an interrupted download.
    pub fn partial_bytes(&self, model: &CatalogModel) -> u64 {
        fs::metadata(download::part_path(&self.path(model)))
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Download (or resume) a catalog model, verify it, and mark it installed.
    pub fn install(
        &self,
        model: &CatalogModel,
        policy: &DownloadPolicy,
        on_progress: &mut dyn FnMut(Progress),
        cancel: &AtomicBool,
    ) -> Result<PathBuf, DownloadError> {
        fs::create_dir_all(&self.root)?;
        let dest = self.path(model);
        if self.is_installed(model) {
            return Ok(dest);
        }
        download::download(
            &model.url,
            &dest,
            model.size_bytes,
            &model.sha256,
            policy,
            on_progress,
            cancel,
        )?;
        fs::write(self.marker(model), &model.sha256)?;
        Ok(dest)
    }

    /// Delete a model, its marker and any partial download.
    pub fn remove(&self, model: &CatalogModel) -> io::Result<()> {
        let dest = self.path(model);
        for p in [dest.clone(), self.marker(model), download::part_path(&dest)] {
            match fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Catalog;

    #[test]
    fn installed_needs_size_and_hash_marker() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let mut m = Catalog::builtin().models[0].clone();
        m.size_bytes = 4;
        assert!(!store.is_installed(&m));
        fs::write(store.path(&m), b"GGUF").unwrap();
        assert!(!store.is_installed(&m), "no marker yet");
        fs::write(store.marker(&m), &m.sha256).unwrap();
        assert!(store.is_installed(&m));
        m.size_bytes = 5;
        assert!(!store.is_installed(&m), "size mismatch");
        store.remove(&m).unwrap();
        assert!(!store.path(&m).exists());
        store.remove(&m).unwrap();
    }
}
