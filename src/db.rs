use std::{time::{ SystemTime, UNIX_EPOCH }};
use redb::{Database, Error, ReadableDatabase, TableDefinition};
use std::path::{PathBuf};

// {path} => (last_played, duration_played, track_added)
const TABLE: TableDefinition<&str, ( u64, u64, u64 )> = TableDefinition::new("bragi.db");

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


// TODO could be async
// if an song_path already exsits in the table read, adn return
// otherwise create a new entry with default values,
pub fn read_or_insert(possible_db: &Option<Database>, query: &str) -> Option<( u64, u64, u64 )> {
    let Some(db) = possible_db else { return None; };

    {
        let read_tx = db.begin_read().ok()?;
        let table = read_tx.open_table(TABLE).ok()?;
        if let Some(tup) = table.get(query).ok()? {
            return Some(tup.value());
        }
    }
    // the value did not exist, create it and push to the db

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // {path} => (last_played, duration_played, track_added)
    let default: ( u64, u64, u64 ) = (now, 0, now);

    let write_tx = db.begin_write().ok()?;
    {
        let mut table = write_tx.open_table(TABLE).ok()?;
        table.insert(query, default).ok()?;
    }
    write_tx.commit().ok()?;

    Some(default)
}

// TODO could also be async
pub fn update_time_played(db: &Database, query: &str) -> Result<(), Error> {
    let read_tx = db.begin_read()?;
    let table = read_tx.open_table(TABLE)?;
    let old_entry = table.get(query).unwrap().expect("This entry should exist already. It does not.");
    drop(read_tx);


    let write_tx = db.begin_write()?;
    {
        let mut new_entry = old_entry.value();
        new_entry.1 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut table = write_tx.open_table(TABLE)?;
        table.insert(query, new_entry)?;
    }
    write_tx.commit()?;

    Ok(())
}
