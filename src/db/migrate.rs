use sqlx::PgPool;

/// Execute the schema DDL (all CREATE TABLE / CREATE INDEX IF NOT EXISTS).
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!("../../sql/schema.sql"))
        .execute(pool)
        .await?;
    Ok(())
}
