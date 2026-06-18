use std::time::{ SystemTime, UNIX_EPOCH };
use redb::{Database, Error, TableDefinition, ReadableDatabase};
use std::path::{PathBuf};

// {path} => timestamp
const TABLE: TableDefinition<String, u64> = TableDefinition::new("user_data");

pub fn db_setup() -> Option<Database> {
    let db_path: PathBuf = dirs::home_dir()
        .expect("could not find home dir")
        .join(".local/share/bragi/bragi.db");

    let db = Database::create(&db_path).ok()?;

    // Create table if it doesn't exist
    let write_tx = db.begin_write().ok()?;
    write_tx.open_table(TABLE).ok()?;
    write_tx.commit().ok()?;

    Some(db)
}

pub fn read_or_insert(db: &Database, query: &String) -> Result<u64, Error> {
    {
        let read_tx = db.begin_read()?;
        let table = read_tx.open_table(TABLE)?;
        if let Some(time) = table.get(query)? {
            return Ok(time.value());
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let write_tx = db.begin_write()?;
    {
        let mut table = write_tx.open_table(TABLE)?;
        table.insert(query, now)?;
    }
    write_tx.commit()?;

    Ok(now)
}
