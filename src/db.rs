use std::time::{ SystemTime, UNIX_EPOCH };
use redb::{Database, Error, ReadableDatabase, TableDefinition};
use std::path::{PathBuf};

// {path} => timestamp
const TABLE: TableDefinition<&str, u64> = TableDefinition::new("bragi.db");

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

pub fn read_or_insert(possible_db: &Option<Database>, query: &str) -> Option<u64> {
    let Some(db) = possible_db else { return None; };
    {
        let read_tx = db.begin_read().ok()?;
        let table = read_tx.open_table(TABLE).ok()?;
        if let Some(time) = table.get(query).ok()? {
            return Some(time.value());
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let write_tx = db.begin_write().ok()?;
    {
        let mut table = write_tx.open_table(TABLE).ok()?;
        table.insert(query, now).ok()?;
    }
    write_tx.commit().ok()?;

    Some(now)
}

pub fn update(db: &Database, query: &str) -> Result<(), Error> {
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

    Ok(())
}
