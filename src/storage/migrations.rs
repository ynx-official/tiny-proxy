use sha2::{Digest, Sha384};
use sqlx::{
    SqlitePool,
    migrate::{AppliedMigration, Migrate, MigrateError, Migration, Migrator},
};
use std::borrow::Cow;

fn compatible_migrator(mut source: Migrator, applied: &[AppliedMigration]) -> Migrator {
    for migration in source.migrations.to_mut() {
        let lf = migration.sql.replace("\r\n", "\n");
        *migration = Migration::new(
            migration.version,
            migration.description.clone(),
            migration.migration_type,
            Cow::Owned(lf),
            migration.no_tx,
        );
        if let Some(previous) = applied
            .iter()
            .find(|entry| entry.version == migration.version)
        {
            let crlf = migration.sql.replace('\n', "\r\n");
            let windows_checksum = Sha384::digest(crlf.as_bytes());
            if previous.checksum.as_ref() == &windows_checksum[..] {
                // v1.0.0 Windows releases embedded CRLF scripts. Accept only
                // that exact alternate checksum, without rewriting history or
                // disabling SQLx's checks for changed, missing or dirty SQL.
                migration.checksum = previous.checksum.clone();
            }
        }
    }
    source
}

pub(super) async fn run(pool: &SqlitePool) -> Result<(), MigrateError> {
    let mut connection = pool.acquire().await?;
    connection.ensure_migrations_table().await?;
    let applied = connection.list_applied_migrations().await?;
    let migrator = compatible_migrator(sqlx::migrate!("./migrations"), &applied);
    migrator.run(&mut *connection).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::MigrationType;

    #[test]
    fn embedded_lf_and_crlf_scripts_resolve_to_the_same_new_migration() {
        let lf = "CREATE TABLE fixture(id INTEGER);\n";
        for sql in [lf.to_owned(), lf.replace('\n', "\r\n")] {
            let source = Migrator {
                migrations: Cow::Owned(vec![Migration::new(
                    1,
                    "fixture".into(),
                    MigrationType::Simple,
                    sql.into(),
                    false,
                )]),
                ..Migrator::DEFAULT
            };
            let canonical = compatible_migrator(source, &[]);
            let migration = canonical.iter().next().unwrap();
            assert_eq!(migration.sql.as_ref(), lf);
            assert_eq!(
                migration.checksum.as_ref(),
                &Sha384::digest(lf.as_bytes())[..]
            );
        }
    }

    #[test]
    fn embedded_windows_sql_accepts_only_the_two_exact_historical_checksums() {
        let lf = "CREATE TABLE fixture(id INTEGER);\n";
        let crlf = lf.replace('\n', "\r\n");
        let lf_checksum = Sha384::digest(lf.as_bytes()).to_vec();
        let crlf_checksum = Sha384::digest(crlf.as_bytes()).to_vec();
        for checksum in [lf_checksum.clone(), crlf_checksum, vec![0_u8; 48]] {
            let source = Migrator {
                migrations: Cow::Owned(vec![Migration::new(
                    1,
                    "fixture".into(),
                    MigrationType::Simple,
                    crlf.clone().into(),
                    false,
                )]),
                ..Migrator::DEFAULT
            };
            let previous = AppliedMigration {
                version: 1,
                checksum: checksum.clone().into(),
            };
            let compatible = compatible_migrator(source, std::slice::from_ref(&previous));
            let migration = compatible.iter().next().unwrap();
            assert_eq!(migration.sql.as_ref(), lf);
            if checksum == vec![0_u8; 48] {
                // Leave an unknown checksum mismatched so SQLx rejects it.
                assert_eq!(migration.checksum.as_ref(), lf_checksum.as_slice());
            } else {
                assert_eq!(migration.checksum, previous.checksum);
            }
        }
    }
}
