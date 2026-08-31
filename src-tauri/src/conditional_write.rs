use std::{fs, path::Path};

use uuid::Uuid;

use crate::{
    error::{CoreError, CoreResult},
    target::{atomic_write, fingerprint, read_target_file},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedWrite {
    pub fingerprint: String,
}

pub fn replace_exact(
    path: &Path,
    expected: Option<&[u8]>,
    output: &[u8],
    subject: &str,
) -> CoreResult<VerifiedWrite> {
    replace_exact_with(path, expected, output, subject, atomic_write)
}

pub fn rollback_exact(
    path: &Path,
    published: &[u8],
    original: Option<&[u8]>,
    subject: &str,
) -> CoreResult<()> {
    let current = current_bytes(path)?;
    if current.as_deref() == original {
        return Ok(());
    }
    if current.as_deref() != Some(published) {
        return Err(CoreError::Drift(format!(
            "{subject} changed after writing; external changes were preserved"
        )));
    }

    match original {
        Some(bytes) => replace_exact(path, Some(published), bytes, subject).map(|_| ()),
        None => rollback_created_file(path, published, subject),
    }
}

fn rollback_created_file(path: &Path, published: &[u8], subject: &str) -> CoreResult<()> {
    rollback_created_file_with(path, published, subject, |_| {})
}

fn rollback_created_file_with<F>(
    path: &Path,
    published: &[u8],
    subject: &str,
    after_move: F,
) -> CoreResult<()>
where
    F: FnOnce(&Path),
{
    let file_name = path
        .file_name()
        .ok_or_else(|| CoreError::Target("Target path has no file name".to_string()))?
        .to_string_lossy();
    let recovery_path = path.with_file_name(format!(
        ".{file_name}.everybuddy-rollback-{}",
        Uuid::new_v4().simple()
    ));
    match fs::rename(path, &recovery_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !path.exists() => {
            return Ok(())
        }
        Err(error) => {
            return Err(CoreError::Target(format!(
                "Could not prepare {} for rollback: {error}",
                path.display()
            )))
        }
    }
    after_move(&recovery_path);

    let moved = read_target_file(&recovery_path)?;
    if moved == published {
        fs::remove_file(&recovery_path).map_err(|error| {
            CoreError::Target(format!(
                "Could not finish rollback for {}: {error}",
                path.display()
            ))
        })?;
        return Ok(());
    }

    match fs::hard_link(&recovery_path, path) {
        Ok(()) => {
            fs::remove_file(&recovery_path).map_err(|error| {
                CoreError::Target(format!(
                    "Could not finish restoring {}: {error}",
                    path.display()
                ))
            })?;
            Err(CoreError::Drift(format!(
                "{subject} changed during rollback; external changes were restored"
            )))
        }
        Err(_) => Err(CoreError::Drift(format!(
            "{subject} changed during rollback; external changes were preserved at {}",
            recovery_path.display()
        ))),
    }
}

fn replace_exact_with<F>(
    path: &Path,
    expected: Option<&[u8]>,
    output: &[u8],
    subject: &str,
    write: F,
) -> CoreResult<VerifiedWrite>
where
    F: FnOnce(&Path, &[u8]) -> CoreResult<()>,
{
    if current_bytes(path)?.as_deref() != expected {
        return Err(CoreError::Drift(format!(
            "{subject} changed immediately before writing"
        )));
    }

    write(path, output)?;
    let written = current_bytes(path)?;
    if written.as_deref() != Some(output) {
        return Err(CoreError::Drift(format!(
            "{subject} changed before the write could be verified; external changes were preserved"
        )));
    }
    let written = written.expect("verified output exists");
    let written_fingerprint = fingerprint(&written);
    if written_fingerprint != fingerprint(output) {
        return Err(CoreError::Target(format!(
            "{subject} fingerprint verification failed after writing"
        )));
    }
    Ok(VerifiedWrite {
        fingerprint: written_fingerprint,
    })
}

fn current_bytes(path: &Path) -> CoreResult<Option<Vec<u8>>> {
    path.exists().then(|| read_target_file(path)).transpose()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::error::CoreError;

    #[test]
    fn stale_input_is_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let external = br#"[{"id":"external"}]"#;
        fs::write(&path, external).unwrap();

        let error =
            replace_exact(&path, Some(b"[]"), br#"[{"id":"managed"}]"#, "Target").unwrap_err();

        assert!(matches!(error, CoreError::Drift(_)));
        assert_eq!(fs::read(path).unwrap(), external);
    }

    #[test]
    fn readback_requires_exact_content_even_when_model_ids_match() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let expected = br#"[{"id":"gpt-5","name":"Expected"}]"#;
        let changed = br#"[{"id":"gpt-5","name":"Changed"}]"#;

        let error = replace_exact_with(&path, None, expected, "Target", |path, bytes| {
            atomic_write(path, bytes)?;
            fs::write(path, changed)?;
            Ok(())
        })
        .unwrap_err();

        assert!(matches!(error, CoreError::Drift(_)));
        assert_eq!(fs::read(path).unwrap(), changed);
    }

    #[test]
    fn rollback_preserves_external_changes_after_publish() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let published = br#"[{"id":"managed"}]"#;
        let external = br#"[{"id":"external"}]"#;
        fs::write(&path, external).unwrap();

        let error = rollback_exact(&path, published, Some(b"[]"), "Target").unwrap_err();

        assert!(matches!(error, CoreError::Drift(_)));
        assert_eq!(fs::read(path).unwrap(), external);
    }

    #[test]
    fn rollback_does_not_remove_a_file_created_after_the_conditional_move() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let published = br#"[{"id":"managed"}]"#;
        let external = br#"[{"id":"external"}]"#;
        fs::write(&path, published).unwrap();

        rollback_created_file_with(&path, published, "Target", |_| {
            fs::write(&path, external).unwrap();
        })
        .unwrap();

        assert_eq!(fs::read(path).unwrap(), external);
    }

    #[test]
    fn rollback_restores_a_change_captured_by_the_conditional_move() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("models.json");
        let external = br#"[{"id":"external"}]"#;
        fs::write(&path, external).unwrap();

        let error = rollback_created_file(&path, br#"[{"id":"managed"}]"#, "Target").unwrap_err();

        assert!(matches!(error, CoreError::Drift(_)));
        assert_eq!(fs::read(path).unwrap(), external);
    }
}
