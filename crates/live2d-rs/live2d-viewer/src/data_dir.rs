use std::path::PathBuf;

/// Returns the user data directory path: $XDG_DATA_HOME/live2d-rs/
/// Falls back to ~/.local/share/live2d-rs/ on Linux.
pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| {
        let home = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        home.join(".local").join("share")
    });
    base.join("live2d-rs")
}

/// Ensures the data directory exists, creating it if necessary.
pub fn ensure_data_dir() -> anyhow::Result<PathBuf> {
    let path = data_dir();
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Returns the full path to the SQLite database file.
pub fn db_path() -> PathBuf {
    data_dir().join("state.db")
}
