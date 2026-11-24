// Fixture for the temp-store gate rule (Task 417). Not compiled.
fn good_open(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    db::keep_temp_in_memory(&c)?;
    Ok(c)
}

fn exempt_open(path: &Path) -> rusqlite::Result<Connection> {
    // temp-store: scratch database with no user data
    let c = Connection::open(path)?;
    Ok(c)
}

fn memory_open() -> rusqlite::Result<Connection> {
    Connection::open_in_memory()
}

#[cfg(test)]
mod tests {
    fn test_open(path: &Path) {
        let _ = Connection::open(path);
    }
}
