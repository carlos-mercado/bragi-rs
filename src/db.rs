use crate::TrackMetadata;
use redb::{Database, Error, MultimapTableDefinition, ReadableDatabase, TableDefinition};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// path => (last_played, duration_played, track_added)
const TRACK_STATS: TableDefinition<&str, (u64, u64, u64)> = TableDefinition::new("tracks.db");

// song_path => playlist
const PLAYLISTS: MultimapTableDefinition<&str, &str> = MultimapTableDefinition::new("playlists.db");

// song_path => metadata
const TRACK_METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("track_metadata.db");

pub fn db_setup() -> Option<Database> {
    let db_path: PathBuf = dirs::data_dir()
        .expect("could not find home dir")
        .join("bragi/bragi.db");

    let db = Database::create(&db_path).ok()?;

    // Create table if it doesn't exist
    let write_tx = db.begin_write().ok()?;
    write_tx.open_table(TRACK_STATS).ok()?;
    write_tx.open_table(TRACK_METADATA).ok()?;
    write_tx.open_multimap_table(PLAYLISTS).ok()?;
    write_tx.commit().ok()?;
    Some(db)
}

// if an song_path already exsits in the table read, adn return
// otherwise create a new entry with default values,
pub fn read_or_insert(possible_db: Option<&Database>, query: &str) -> Option<(u64, u64, u64)> {
    let db = possible_db.as_ref()?;
    {
        let read_tx = db.begin_read().ok()?;
        let table = read_tx.open_table(TRACK_STATS).ok()?;
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
    let default: (u64, u64, u64) = (now, 0, now);

    let write_tx = db.begin_write().ok()?;
    {
        let mut table = write_tx.open_table(TRACK_STATS).ok()?;
        table.insert(query, default).ok()?;
    }
    write_tx.commit().ok()?;

    Some(default)
}

pub fn update_last_played(db: &Database, query: &str) -> Result<(), Error> {
    let read_tx = db.begin_read()?;
    let table = read_tx.open_table(TRACK_STATS)?;
    let old_entry = table
        .get(query)
        .unwrap()
        .expect("This entry should exist already. It does not.");
    drop(read_tx);

    let write_tx = db.begin_write()?;
    {
        let mut new_entry = old_entry.value();
        new_entry.0 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut table = write_tx.open_table(TRACK_STATS)?;
        table.insert(query, new_entry)?;
    }
    write_tx.commit()?;

    Ok(())
}

pub fn update_duration_played(db: &Database, query: &str, duration: u64) -> Result<(), Error> {
    let read_tx = db.begin_read()?;
    let table = read_tx.open_table(TRACK_STATS)?;
    let old_entry = table
        .get(query)
        .unwrap()
        .expect("This entry should exist already. It does not.");
    drop(read_tx);

    let write_tx = db.begin_write()?;
    {
        let mut new_entry = old_entry.value();
        new_entry.1 += duration;
        let mut table = write_tx.open_table(TRACK_STATS)?;
        table.insert(query, new_entry)?;
    }
    write_tx.commit()?;

    Ok(())
}

pub fn add_playlist_labels(db: &Database, query_path: &str, label: &str) -> Result<(), Error> {
    let write_tx = db.begin_write().expect("couldn't begin_write");
    {
        let mut table = write_tx.open_multimap_table(PLAYLISTS)?;
        table.insert(query_path, label)?;
    }
    write_tx.commit().expect("couldn't commit");

    Ok(())
}

pub fn get_playlist_labels(
    possible_db: Option<&Database>,
    query_path: &str,
) -> Option<Vec<String>> {
    let db = possible_db?;

    let read_txn = db.begin_read().ok()?;
    let table = read_txn.open_multimap_table(PLAYLISTS).ok()?;
    let values = table.get(query_path).ok()?;
    let mut labels_vec = Vec::new();
    for v in values {
        labels_vec.push(v.ok()?.value().to_string());
    }

    Some(labels_vec)
}

pub fn remove_playlist_labels(db: &Database, query_path: &str, label: &str) -> Result<(), Error> {
    let write_tx = db.begin_write()?;
    {
        let mut table = write_tx.open_multimap_table(PLAYLISTS)?;
        table.remove(query_path, label)?;
    }
    write_tx.commit()?;
    Ok(())
}

pub fn wipe_playlist_labels(db: &Database, query_path: &str) -> Result<(), Error> {
    let write_tx = db.begin_write()?;
    {
        let mut table = write_tx.open_multimap_table(PLAYLISTS)?;
        table.remove_all(query_path)?;
    }
    write_tx.commit()?;
    Ok(())
}

pub fn cache_metadata(db: &Database, path: &str, meta: &TrackMetadata) -> Result<(), Error> {
    let bytes = wincode::serialize(meta).expect("serialization failed");
    let write_tx = db.begin_write()?;
    {
        let mut table = write_tx.open_table(TRACK_METADATA)?;
        table.insert(path, bytes.as_slice())?;
    }
    write_tx.commit()?;
    Ok(())
}

pub fn get_cached_metadata(db: &Database, path: &str) -> Option<TrackMetadata> {
    let read_tx = db.begin_read().ok()?;
    let table = read_tx.open_table(TRACK_METADATA).ok()?;
    let bytes = table.get(path).ok()??;
    wincode::deserialize(bytes.value()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::backends::InMemoryBackend;

    fn setup_test_db() -> Database {
        let db = Database::builder()
            .create_with_backend(InMemoryBackend::new())
            .unwrap();

        let write_tx = db.begin_write().unwrap();
        write_tx.open_table(TRACK_STATS).unwrap();
        write_tx.open_multimap_table(PLAYLISTS).unwrap();
        write_tx.commit().unwrap();

        db
    }

    fn labels() -> Vec<String> {
        vec![
            "Post-Rock".to_string(),
            "Prog".to_string(),
            "gym".to_string(),
            "spotify-wrapped_2025".to_string(),
        ]
    }

    #[test]
    fn test_labels() {
        let db = setup_test_db();
        let test_song = "/this/is/a/fakepath.mp3";
        let labels = labels();

        for (i, label) in labels.iter().enumerate() {
            add_playlist_labels(&db, test_song, label).unwrap();
            let returned_labels = get_playlist_labels(Some(&db), test_song).unwrap();
            assert_eq!(returned_labels, labels[..i + 1]);
        }
    }

    #[test]
    fn test_db_wipe() {
        let db = setup_test_db();
        let test_song = "/this/is/a/fakepath.mp3";
        let labels = labels();

        for label in labels {
            add_playlist_labels(&db, test_song, &label).unwrap();
        }
        wipe_playlist_labels(&db, test_song).unwrap();

        assert!(
            get_playlist_labels(Some(&db), test_song)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn non_labeled_song_returns_empty() {
        let db = setup_test_db();
        let query_path = "/this/is/a/fakepath.mp3";
        let tags = get_playlist_labels(Some(&db), query_path).unwrap();
        assert!(tags.is_empty());
    }
}
