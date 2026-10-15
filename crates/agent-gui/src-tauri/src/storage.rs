//! Canonical application storage roots and the non-destructive LiveAgent compatibility bridge.
//!
//! OneAgent writes to `~/.oneagent`. Existing `~/.liveagent` data is copied only
//! when the canonical destination is absent; the legacy directory is never
//! removed or overwritten.
use std::fs;
use std::path::{Path, PathBuf};

pub const CANONICAL_STORAGE_DIR_NAME: &str = ".oneagent";
pub const LEGACY_STORAGE_DIR_NAME: &str = ".liveagent";
const MIGRATION_MARKER: &str = ".liveagent-migrated-to-oneagent";

pub fn canonical_storage_dir() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(CANONICAL_STORAGE_DIR_NAME))
        .ok_or_else(|| "无法定位用户目录".to_string())
}

pub fn legacy_storage_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(LEGACY_STORAGE_DIR_NAME))
}

/// Resolve the canonical root and import only missing legacy entries.
pub fn ensure_canonical_storage() -> Result<PathBuf, String> {
    let canonical = canonical_storage_dir()?;
    // Migrate only during the first creation of the canonical root. Once it
    // exists, never merge from a possibly-active legacy tree on every access;
    // this avoids copying SQLite files/WALs while the old application is still
    // running and makes the migration genuinely one-time and predictable.
    fs::create_dir_all(&canonical).map_err(|e| format!("创建应用目录失败：{e}"))?;
    let marker = canonical.join(MIGRATION_MARKER);
    if !marker.exists() {
        if let Some(legacy) = legacy_storage_dir().filter(|path| path.is_dir()) {
            copy_tree_without_overwrite(&legacy, &canonical)?;
        }
        // The marker is created only after the best-effort copy succeeds. A
        // later launch can retry if a transient file-system error interrupted
        // migration; legacy data is never removed and canonical files win.
        fs::write(&marker, b"OneAgent storage migration complete\n")
            .map_err(|e| format!("写入应用迁移标记失败：{e}"))?;
    }
    Ok(canonical)
}

fn copy_tree_without_overwrite(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(|e| format!("读取旧应用目录失败：{e}"))? {
        let entry = entry.map_err(|e| format!("读取旧应用目录项失败：{e}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(|e| format!("读取旧应用目录项类型失败：{e}"))?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).map_err(|e| format!("创建应用目录失败：{e}"))?;
            copy_tree_without_overwrite(&source_path, &destination_path)?;
        } else if file_type.is_file() && !destination_path.exists() {
            fs::copy(&source_path, &destination_path).map_err(|e| format!("迁移旧应用文件失败：{e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::copy_tree_without_overwrite;
    use std::fs;

    #[test]
    fn legacy_copy_never_overwrites_canonical_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let legacy = temp.path().join("legacy");
        let canonical = temp.path().join("canonical");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&canonical).unwrap();
        fs::write(legacy.join("existing.txt"), "legacy").unwrap();
        fs::write(canonical.join("existing.txt"), "canonical").unwrap();
        fs::write(legacy.join("new.txt"), "new").unwrap();
        copy_tree_without_overwrite(&legacy, &canonical).unwrap();
        assert_eq!(fs::read_to_string(canonical.join("existing.txt")).unwrap(), "canonical");
        assert_eq!(fs::read_to_string(canonical.join("new.txt")).unwrap(), "new");
    }
}
