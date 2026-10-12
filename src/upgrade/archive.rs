//! A release archive on disk: its line in `SHA256SUMS`, its digest, and the
//! one file taken out of it, the binary.

use anyhow::{Context as _, Result, bail};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

/// The name of the binary inside an archive.
pub const BINARY_NAME: &str = if cfg!(windows) { "2bec.exe" } else { "2bec" };

/// The digest `SHA256SUMS` lists for `name` (`sha256sum` format: hash, two
/// spaces or ` *`, name), lower-case.
pub fn expected_sha256(sums: &str, name: &str) -> Result<String> {
    for line in sums.lines() {
        let mut parts = line.split_whitespace();
        if let (Some(hash), Some(listed)) = (parts.next(), parts.next())
            && listed.trim_start_matches('*') == name
        {
            return Ok(hash.to_ascii_lowercase());
        }
    }
    bail!("SHA256SUMS does not list {name}")
}

/// The hexadecimal SHA-256 of a file, streamed.
pub fn sha256_of(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Refuse an archive whose digest is not the one `SHA256SUMS` lists.
pub fn verify(archive: &Path, sums: &str, name: &str) -> Result<()> {
    let expected = expected_sha256(sums, name)?;
    let actual = sha256_of(archive)?;
    if expected != actual {
        bail!(
            "checksum mismatch for {name} (expected {expected}, got {actual}): \
             the download is corrupt or tampered"
        );
    }
    Ok(())
}

/// Copy the binary of the archive (`<folder>/2bec`, the folder being the
/// archive's own name) into `dest`, which is created or truncated. Every
/// other entry is left alone; a missing binary is an error.
pub fn extract_binary(archive: &Path, folder: &str, dest: &Path) -> Result<()> {
    let wanted = format!("{folder}/{BINARY_NAME}");
    let file = std::fs::File::open(archive)
        .with_context(|| format!("cannot read {}", archive.display()))?;
    let mut out =
        std::fs::File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
    let found = copy_entry(file, &wanted, &mut out)
        .with_context(|| format!("cannot unpack {}", archive.display()))?;
    out.flush()
        .with_context(|| format!("cannot write {}", dest.display()))?;
    if !found {
        let _ = std::fs::remove_file(dest);
        bail!("the archive holds no {wanted}");
    }
    Ok(())
}

/// Stream the entry named `wanted` of a tar.gz into `out`; false when absent.
#[cfg(unix)]
fn copy_entry(file: std::fs::File, wanted: &str, out: &mut dyn Write) -> Result<bool> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.to_string_lossy() == wanted && entry.header().entry_type().is_file() {
            std::io::copy(&mut entry, out)?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Stream the entry named `wanted` of a zip into `out`; false when absent.
#[cfg(windows)]
fn copy_entry(file: std::fs::File, wanted: &str, out: &mut dyn Write) -> Result<bool> {
    let mut archive = zip::ZipArchive::new(file)?;
    let Ok(mut entry) = archive.by_name(wanted) else {
        return Ok(false);
    };
    if !entry.is_file() {
        return Ok(false);
    }
    std::io::copy(&mut entry, out)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A release archive holding `folder/2bec` with `body`, plus a sibling
    /// file, packed as the release pipeline packs it.
    pub(crate) fn pack(dir: &Path, folder: &str, body: &[u8]) -> std::path::PathBuf {
        #[cfg(unix)]
        {
            let path = dir.join(format!("{folder}.tar.gz"));
            let gz = flate2::write::GzEncoder::new(
                std::fs::File::create(&path).unwrap(),
                flate2::Compression::fast(),
            );
            let mut tar = tar::Builder::new(gz);
            let mut header = tar::Header::new_gnu();
            header.set_size(5);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, format!("{folder}/README.md"), &b"hello"[..])
                .unwrap();
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header, format!("{folder}/2bec"), body)
                .unwrap();
            tar.into_inner().unwrap().finish().unwrap();
            path
        }
        #[cfg(windows)]
        {
            let path = dir.join(format!("{folder}.zip"));
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file(format!("{folder}/README.md"), options)
                .unwrap();
            zip.write_all(b"hello").unwrap();
            zip.start_file(format!("{folder}/2bec.exe"), options)
                .unwrap();
            zip.write_all(body).unwrap();
            zip.finish().unwrap();
            path
        }
    }

    #[test]
    fn finds_the_digest_of_a_name() {
        let sums =
            "aa  2bec-0.2.0-linux-x64.tar.gz\nBB *2bec-0.2.0-windows-x64.zip\ncc  SHA256SUMS\n";
        assert_eq!(
            expected_sha256(sums, "2bec-0.2.0-linux-x64.tar.gz").unwrap(),
            "aa"
        );
        assert_eq!(
            expected_sha256(sums, "2bec-0.2.0-windows-x64.zip").unwrap(),
            "bb"
        );
        let err = expected_sha256(sums, "2bec-0.2.0-macos-x64.tar.gz").unwrap_err();
        assert!(err.to_string().contains("does not list"), "{err}");
    }

    #[test]
    fn hashes_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(
            sha256_of(&f).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_refuses_a_wrong_digest() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.tar.gz");
        std::fs::write(&f, b"abc").unwrap();
        let good = format!("{}  a.tar.gz\n", sha256_of(&f).unwrap());
        verify(&f, &good, "a.tar.gz").unwrap();
        let err = verify(&f, "00  a.tar.gz\n", "a.tar.gz").unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
    }

    #[test]
    fn extracts_only_the_binary() {
        let dir = tempfile::tempdir().unwrap();
        let archive = pack(
            dir.path(),
            "2bec-0.2.0-test",
            b"#!/bin/sh\necho 2bec 0.2.0\n",
        );
        let dest = dir.path().join("2bec.new");
        extract_binary(&archive, "2bec-0.2.0-test", &dest).unwrap();
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            b"#!/bin/sh\necho 2bec 0.2.0\n"
        );
        assert!(!dir.path().join("2bec-0.2.0-test").exists());
    }

    #[test]
    fn a_missing_binary_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let archive = pack(dir.path(), "2bec-0.2.0-test", b"x");
        let dest = dir.path().join("2bec.new");
        let err = extract_binary(&archive, "2bec-0.3.0-test", &dest).unwrap_err();
        assert!(
            err.to_string().contains("holds no 2bec-0.3.0-test/"),
            "{err}"
        );
        assert!(!dest.exists());
    }

    #[test]
    fn a_corrupt_archive_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("bad.tar.gz");
        std::fs::write(&archive, b"not an archive").unwrap();
        let err = extract_binary(&archive, "x", &dir.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("cannot unpack"), "{err}");
    }
}
