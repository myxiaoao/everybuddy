use super::*;

#[derive(Debug, Clone)]
pub(crate) struct PendingFileWrite {
    pub kind: TargetKind,
    pub configured_path: PathBuf,
    pub write_path: PathBuf,
    pub original: Option<Vec<u8>>,
    pub output: Vec<u8>,
}

impl Store {
    pub(crate) fn ensure_no_pending_file_writes(&self) -> CoreResult<()> {
        let pending: bool = self.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM pending_file_writes)",
            [],
            |row| row.get(0),
        )?;
        if pending {
            return Err(CoreError::Conflict("An interrupted file operation needs recovery; reopen the app before publishing or restoring".into()));
        }
        Ok(())
    }

    pub(crate) fn begin_file_writes(&self, writes: &[PendingFileWrite]) -> CoreResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pending: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM pending_file_writes)",
            [],
            |row| row.get(0),
        )?;
        if pending {
            return Err(CoreError::Conflict("An interrupted file operation needs recovery; reopen the app before publishing or restoring".into()));
        }
        for write in writes {
            transaction.execute(
                "INSERT INTO pending_file_writes (target, configured_path, write_path, original, output) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![write.kind.as_str(), write.configured_path.to_string_lossy(), write.write_path.to_string_lossy(), write.original, write.output],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn pending_file_writes(&self) -> CoreResult<Vec<PendingFileWrite>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT target, configured_path, write_path, original, output FROM pending_file_writes ORDER BY rowid DESC")?;
        let mut rows = statement.query([])?;
        let mut writes = Vec::new();
        while let Some(row) = rows.next()? {
            writes.push(PendingFileWrite {
                kind: TargetKind::from_str(&row.get::<_, String>(0)?)
                    .map_err(CoreError::Storage)?,
                configured_path: PathBuf::from(row.get::<_, String>(1)?),
                write_path: PathBuf::from(row.get::<_, String>(2)?),
                original: row.get(3)?,
                output: row.get(4)?,
            });
        }
        Ok(writes)
    }

    pub(crate) fn clear_file_write(&self, target: TargetKind) -> CoreResult<()> {
        self.connection()?.execute(
            "DELETE FROM pending_file_writes WHERE target = ?1",
            [target.as_str()],
        )?;
        Ok(())
    }
}
