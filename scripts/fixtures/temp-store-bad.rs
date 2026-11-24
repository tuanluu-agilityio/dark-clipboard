// Fixture for the temp-store gate rule (Task 417). Not compiled.
fn bad_open(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    c.pragma_update(None, "foreign_keys", true)?;
    Ok(c)
}
