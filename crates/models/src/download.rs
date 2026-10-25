//! Verified downloads.
//!
//! * HTTPS only, and every hop (including redirects) must be on the host allow-list.
//! * Data goes to `<file>.part`; an interrupted download resumes with an HTTP range request.
//! * The finished file must have the expected size and SHA-256 before it is renamed into
//!   place. A file that fails the check is deleted, never used.
//! * Free disk space is checked up front, so a large model can't fill the drive.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};
use url::Url;

/// Keep this much space free on the drive after a download.
const HEADROOM_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REDIRECTS: usize = 10;
const MAX_UNZIPPED_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct DownloadPolicy {
    /// Exact host names, or `.suffix` entries that also match subdomains.
    pub allowed_hosts: Vec<String>,
}

impl Default for DownloadPolicy {
    /// Hugging Face (models) and GitHub releases (inference engines).
    fn default() -> Self {
        Self {
            allowed_hosts: [
                "huggingface.co",
                ".huggingface.co",
                ".hf.co",
                "github.com",
                ".githubusercontent.com",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

impl DownloadPolicy {
    pub fn allows(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        url.scheme() == "https"
            && self
                .allowed_hosts
                .iter()
                .any(|h| match h.strip_prefix('.') {
                    Some(suffix) => host.ends_with(&format!(".{suffix}")),
                    None => host == h,
                })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum Progress {
    Downloading { done: u64, total: u64 },
    Verifying { done: u64, total: u64 },
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("blocked: {0} is not an allowed download location")]
    Blocked(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("server answered HTTP {0}")]
    Status(u16),
    #[error("not enough disk space: needs {needed_gb:.1} GB, {free_gb:.1} GB free")]
    NoSpace { needed_gb: f64, free_gb: f64 },
    #[error("downloaded {got} bytes, expected {expected}")]
    Size { expected: u64, got: u64 },
    #[error("file is corrupt or was tampered with (SHA-256 mismatch); it was deleted")]
    Hash,
    #[error("cancelled")]
    Cancelled,
    #[error("bad archive: {0}")]
    Archive(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// Download `url` to `dest`, resuming a previous partial download, and verify size and hash.
pub fn download(
    url: &str,
    dest: &Path,
    size: u64,
    sha256: &str,
    policy: &DownloadPolicy,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    let part = part_path(dest);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > size {
        fs::remove_file(&part)?;
        have = 0;
    }

    if have < size {
        let needed = size - have + HEADROOM_BYTES;
        if let Some(free) = free_space(dest.parent().unwrap_or(Path::new("."))) {
            if free < needed {
                return Err(DownloadError::NoSpace {
                    needed_gb: needed as f64 / 1e9,
                    free_gb: free as f64 / 1e9,
                });
            }
        }

        let response = open(url, policy, have)?;
        let resumed = response.status() == 206 && content_range_start(&response) == Some(have);
        let mut out = if resumed {
            OpenOptions::new().append(true).open(&part)?
        } else {
            have = 0;
            File::create(&part)?
        };

        let mut reader = response.into_reader();
        let mut buf = vec![0u8; 1 << 20];
        let mut last = Instant::now();
        on_progress(Progress::Downloading {
            done: have,
            total: size,
        });
        loop {
            if cancel.load(Ordering::Relaxed) {
                out.flush()?;
                return Err(DownloadError::Cancelled);
            }
            let n = reader
                .read(&mut buf)
                .map_err(|e| DownloadError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            have += n as u64;
            if have > size {
                drop(out);
                fs::remove_file(&part)?;
                return Err(DownloadError::Size {
                    expected: size,
                    got: have,
                });
            }
            out.write_all(&buf[..n])?;
            if last.elapsed() >= Duration::from_millis(250) {
                on_progress(Progress::Downloading {
                    done: have,
                    total: size,
                });
                last = Instant::now();
            }
        }
        out.sync_all()?;
        on_progress(Progress::Downloading {
            done: have,
            total: size,
        });
    }

    if have != size {
        // Connection closed early: keep the part file so the next attempt resumes.
        return Err(DownloadError::Size {
            expected: size,
            got: have,
        });
    }

    let actual = hash_file(
        &part,
        &mut |done| on_progress(Progress::Verifying { done, total: size }),
        cancel,
    )?;
    if !actual.eq_ignore_ascii_case(sha256) {
        fs::remove_file(&part)?;
        return Err(DownloadError::Hash);
    }
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    fs::rename(&part, dest)?;
    Ok(())
}

/// Lowercase hex SHA-256 of a file.
pub fn hash_file(
    path: &Path,
    on_progress: &mut dyn FnMut(u64),
    cancel: &AtomicBool,
) -> Result<String, DownloadError> {
    let mut reader = BufReader::with_capacity(1 << 20, File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut last = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            on_progress(done);
            last = Instant::now();
        }
    }
    on_progress(done);
    Ok(hex::encode(hasher.finalize()))
}

/// GET with manual redirect handling so every hop is checked against the allow-list.
fn open(
    url: &str,
    policy: &DownloadPolicy,
    range_start: u64,
) -> Result<ureq::Response, DownloadError> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("InferMark/", env!("CARGO_PKG_VERSION")))
        .build();
    let mut current = Url::parse(url).map_err(|_| DownloadError::Blocked(url.to_string()))?;
    for _ in 0..=MAX_REDIRECTS {
        if !policy.allows(&current) {
            return Err(DownloadError::Blocked(redact(&current)));
        }
        let mut request = agent.request_url("GET", &current);
        if range_start > 0 {
            request = request.set("Range", &format!("bytes={range_start}-"));
        }
        match request.call() {
            Ok(r) if (300..400).contains(&r.status()) => {
                let location = r
                    .header("location")
                    .ok_or_else(|| DownloadError::Network("redirect without a location".into()))?;
                current = current
                    .join(location)
                    .map_err(|_| DownloadError::Network("invalid redirect".into()))?;
            }
            Ok(r) => return Ok(r),
            Err(ureq::Error::Status(code, _)) => return Err(DownloadError::Status(code)),
            Err(e) => return Err(DownloadError::Network(e.to_string())),
        }
    }
    Err(DownloadError::Network("too many redirects".into()))
}

/// Host and path only: signed CDN URLs carry tokens in the query string.
fn redact(url: &Url) -> String {
    format!("{}{}", url.host_str().unwrap_or("?"), url.path())
}

fn content_range_start(r: &ureq::Response) -> Option<u64> {
    // "bytes 1000-1999/2000"
    let v = r.header("content-range")?;
    v.strip_prefix("bytes ")?
        .split('-')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Free space on the drive holding `path`, if it can be determined.
pub fn free_space(path: &Path) -> Option<u64> {
    let path = fs::canonicalize(path).ok()?;
    let path = strip_verbatim(&path);
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// `\\?\D:\x` -> `D:\x` so it can be compared with mount points on Windows.
fn strip_verbatim(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p.to_path_buf(),
    }
}

/// Extract a zip archive into `dest`, refusing entries that would land outside it and
/// archives that expand to more than 4 GB.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<Vec<PathBuf>, DownloadError> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?)
        .map_err(|e| DownloadError::Archive(e.to_string()))?;
    fs::create_dir_all(dest)?;
    let mut written = Vec::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| DownloadError::Archive(e.to_string()))?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(DownloadError::Archive(format!(
                "unsafe path `{}`",
                entry.name()
            )));
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        total += entry.size();
        if total > MAX_UNZIPPED_BYTES {
            return Err(DownloadError::Archive("archive is too large".into()));
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&out_path)?;
        io::copy(&mut (&mut entry).take(MAX_UNZIPPED_BYTES), &mut out)?;
        written.push(out_path);
    }
    Ok(written)
}

/// Extract a `.tar.gz` archive into `dest`. The `tar` crate refuses entries (and links) that
/// would land outside `dest`.
pub fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), DownloadError> {
    fs::create_dir_all(dest)?;
    let gz = flate2::read::GzDecoder::new(BufReader::new(File::open(archive)?));
    let mut tar = tar::Archive::new(gz.take(MAX_UNZIPPED_BYTES));
    tar.set_preserve_permissions(true);
    tar.unpack(dest)
        .map_err(|e| DownloadError::Archive(e.to_string()))
}

/// Extract a `.zip` or `.tar.gz` archive, chosen by file name.
pub fn extract(archive: &Path, dest: &Path) -> Result<(), DownloadError> {
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") {
        extract_zip(archive, dest).map(|_| ())
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest)
    } else {
        Err(DownloadError::Archive(format!(
            "unknown archive type: {name}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list_matches_hosts_and_subdomains_over_https_only() {
        let p = DownloadPolicy::default();
        let ok = [
            "https://huggingface.co/x/resolve/main/a.gguf",
            "https://cas-bridge.xethub.hf.co/x",
            "https://cdn-lfs.huggingface.co/x",
            "https://release-assets.githubusercontent.com/x",
            "https://github.com/ggml-org/llama.cpp/releases/download/b1/x.zip",
        ];
        for u in ok {
            assert!(p.allows(&Url::parse(u).unwrap()), "{u}");
        }
        let blocked = [
            "http://huggingface.co/x",
            "https://huggingface.co.evil.com/x",
            "https://evilhf.co/x",
            "https://hf.co.evil.com/x",
            "https://example.com/x",
            "file:///etc/passwd",
        ];
        for u in blocked {
            assert!(!p.allows(&Url::parse(u).unwrap()), "{u}");
        }
    }

    #[test]
    fn blocked_url_never_touches_the_network() {
        let dir = tempfile::tempdir().unwrap();
        let err = download(
            "https://example.com/model.gguf",
            &dir.path().join("m.gguf"),
            10,
            &"0".repeat(64),
            &DownloadPolicy::default(),
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::Blocked(_)), "{err}");
    }

    #[test]
    fn complete_part_file_is_verified_and_moved_into_place() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.gguf");
        fs::write(part_path(&dest), b"hello").unwrap();
        // sha256("hello")
        let sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let mut phases = Vec::new();
        download(
            "https://example.com/unused",
            &dest,
            5,
            sha,
            &DownloadPolicy::default(),
            &mut |p| phases.push(p),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"hello");
        assert!(!part_path(&dest).exists());
        assert!(matches!(
            phases.last(),
            Some(Progress::Verifying { done: 5, total: 5 })
        ));
    }

    #[test]
    fn wrong_hash_deletes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m.gguf");
        fs::write(part_path(&dest), b"hellO").unwrap();
        let sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let err = download(
            "https://example.com/unused",
            &dest,
            5,
            sha,
            &DownloadPolicy::default(),
            &mut |_| {},
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(err, DownloadError::Hash));
        assert!(!part_path(&dest).exists() && !dest.exists());
    }

    #[test]
    fn free_space_is_known_for_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(free_space(dir.path()).unwrap_or(0) > 0);
    }

    #[test]
    fn zip_slip_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("evil.zip");
        {
            let mut w = zip::ZipWriter::new(File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("../escape.txt", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let err = extract_zip(&archive, &dir.path().join("out")).unwrap_err();
        assert!(matches!(err, DownloadError::Archive(_)));
        assert!(!dir.path().join("escape.txt").exists());
    }
}
