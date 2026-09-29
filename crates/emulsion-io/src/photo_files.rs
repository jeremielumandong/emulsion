//! Small recoverable file transactions for virtual-reference and sidecar updates.
use crate::{IoError, Result};
use std::{io::Write, path::PathBuf};
pub(crate) struct Change {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
}
pub(crate) fn apply(changes: &[Change]) -> Result<()> {
    for c in changes {
        if std::fs::symlink_metadata(&c.path)
            .is_ok_and(|m| !m.is_file() || m.file_type().is_symlink())
        {
            return Err(IoError::Manifest(
                "Recipe destination must be a regular file".into(),
            ));
        }
        let current = match std::fs::read(&c.path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        if current != c.before {
            return Err(IoError::Manifest(
                "Photo files changed during relink; retry".into(),
            ));
        }
    }
    for (index, c) in changes.iter().enumerate() {
        if let Err(error) = crate::write_atomic(&c.path, |f| {
            f.write_all(&c.after)?;
            Ok(())
        }) {
            for previous in changes[..index].iter().rev() {
                match &previous.before {
                    Some(bytes) => {
                        crate::write_atomic(&previous.path, |f| {
                            f.write_all(bytes)?;
                            Ok(())
                        })?;
                    }
                    None => {
                        std::fs::remove_file(&previous.path)?;
                    }
                }
            }
            return Err(error);
        }
    }
    Ok(())
}
