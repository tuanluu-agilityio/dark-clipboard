// Fixture for the temp-store gate rule (Task 417). Not compiled.
#[cfg(test)]
mod tests {
    fn t() {
        let s = Connection::open("x");
    }
}

fn bad_open_after_inline_test_mod(path: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(path)?;
    Ok(c)
}
