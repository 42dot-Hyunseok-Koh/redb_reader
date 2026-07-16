use redb::{
    Database, MultimapTableHandle, ReadableDatabase, ReadableTable,
    ReadableTableMetadata, TableDefinition, TableHandle,
};
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

const CANDIDATE_TABLES: &[&str] = &[
    "log_entry",
    "log_metadata",
    "snapshot",
    "persistent_savepoints",
];

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        print_usage(&args[0]);
        return Ok(());
    }

    let db_path = PathBuf::from(&args[1]);
    let command = args[2].as_str();
    let limit = parse_limit(&args).unwrap_or(50);

    match command {
        "scan-strings" => scan_strings(&db_path)?,
        "list" => list_tables(&db_path)?,
        "dump" => {
            let table = args.get(3).map(|s| s.as_str());
            dump_tables(&db_path, table, limit)?;
        }
        _ => print_usage(&args[0]),
    }

    Ok(())
}

fn print_usage(bin: &str) {
    eprintln!("redb_reader - simple read-only redb inspector");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  {bin} <db-file> scan-strings");
    eprintln!("  {bin} <db-file> list");
    eprintln!("  {bin} <db-file> dump [table-name] [--limit N]");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {bin} dm_cluster_persistent.db scan-strings");
    eprintln!("  {bin} dm_cluster_persistent.db list");
    eprintln!("  {bin} dm_cluster_persistent.db dump log_metadata --limit 200");
}

fn parse_limit(args: &[String]) -> Option<usize> {
    args.windows(2)
        .find(|w| w[0] == "--limit")
        .and_then(|w| w[1].parse::<usize>().ok())
}

fn open_db(path: &PathBuf) -> Result<Database, Box<dyn Error>> {
    // Opens read-only in practice because this program only starts read transactions.
    // Make a backup before inspecting production DB files.
    Ok(Database::open(path)?)
}

fn scan_strings(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let mut current = Vec::new();
    let mut found = Vec::new();

    for b in bytes {
        if b.is_ascii_graphic() || b == b' ' {
            current.push(b);
        } else {
            if current.len() >= 4 {
                found.push(String::from_utf8_lossy(&current).to_string());
            }
            current.clear();
        }
    }
    if current.len() >= 4 {
        found.push(String::from_utf8_lossy(&current).to_string());
    }

    found.sort();
    found.dedup();

    for s in found {
        println!("{s}");
    }
    Ok(())
}

fn list_tables(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let db = open_db(path)?;
    let txn = db.begin_read()?;

    println!("Tables:");
    for handle in txn.list_tables()? {
        println!("- {}", handle.name());
    }

    println!("Multimap tables:");
    for handle in txn.list_multimap_tables()? {
        println!("- {}", handle.name());
    }

    Ok(())
}

fn dump_tables(path: &PathBuf, table: Option<&str>, limit: usize) -> Result<(), Box<dyn Error>> {
    let db = open_db(path)?;
    let txn = db.begin_read()?;

    let names: Vec<String> = if let Some(name) = table {
        vec![name.to_string()]
    } else {
        let listed: Vec<String> = txn.list_tables()?.map(|h| h.name().to_string()).collect();
        if listed.is_empty() {
            CANDIDATE_TABLES.iter().map(|s| s.to_string()).collect()
        } else {
            listed
        }
    };

    for name in names {
        println!("\n=== table: {name} ===");
        if dump_str_vec_u8(&txn, &name, limit).is_ok() {
            continue;
        }
        if dump_u64_vec_u8(&txn, &name, limit).is_ok() {
            continue;
        }
        if dump_str_str(&txn, &name, limit).is_ok() {
            continue;
        }
        eprintln!("Could not open '{name}' with supported type guesses.");
        eprintln!("Tried: <&str, Vec<u8>>, <u64, Vec<u8>>, <&str, &str>.");
    }

    Ok(())
}

fn dump_str_vec_u8(
    txn: &redb::ReadTransaction,
    name: &str,
    limit: usize,
) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<&str, Vec<u8>> = TableDefinition::new(name);
    let table = txn.open_table(def)?;
    println!("type guess: key=&str, value=Vec<u8>, len={}", table.len()?);
    for (idx, item) in table.iter()?.enumerate() {
        if idx >= limit {
            println!("... truncated at {limit} rows");
            break;
        }
        let (k, v) = item?;
        print_row(idx, k.value().as_bytes(), v.value().as_slice());
    }
    Ok(())
}

fn dump_u64_vec_u8(
    txn: &redb::ReadTransaction,
    name: &str,
    limit: usize,
) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<u64, Vec<u8>> = TableDefinition::new(name);
    let table = txn.open_table(def)?;
    println!("type guess: key=u64, value=Vec<u8>, len={}", table.len()?);
    for (idx, item) in table.iter()?.enumerate() {
        if idx >= limit {
            println!("... truncated at {limit} rows");
            break;
        }
        let (k, v) = item?;
        print_row(idx, &k.value().to_le_bytes(), v.value().as_slice());
        println!("  key_u64: {}", k.value());
    }
    Ok(())
}

fn dump_str_str(
    txn: &redb::ReadTransaction,
    name: &str,
    limit: usize,
) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<&str, &str> = TableDefinition::new(name);
    let table = txn.open_table(def)?;
    println!("type guess: key=&str, value=&str, len={}", table.len()?);
    for (idx, item) in table.iter()?.enumerate() {
        if idx >= limit {
            println!("... truncated at {limit} rows");
            break;
        }
        let (k, v) = item?;
        print_row(idx, k.value().as_bytes(), v.value().as_bytes());
    }
    Ok(())
}

fn print_row(idx: usize, key: &[u8], value: &[u8]) {
    println!("[{idx}]");
    println!("  key_hex:   {}", hex_preview(key, 256));
    println!("  key_text:  {}", text_preview(key, 256));
    println!("  val_len:   {}", value.len());
    println!("  val_hex:   {}", hex_preview(value, 512));
    println!("  val_text:  {}", text_preview(value, 512));
}

fn hex_preview(bytes: &[u8], max: usize) -> String {
    let take = bytes.len().min(max);
    let mut out = String::with_capacity(take * 3);
    for b in &bytes[..take] {
        out.push_str(&format!("{:02x}", b));
    }
    if bytes.len() > max {
        out.push_str("...");
    }
    out
}

fn text_preview(bytes: &[u8], max: usize) -> String {
    let take = bytes.len().min(max);
    let mut out = String::new();
    for &b in &bytes[..take] {
        match b {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            0x20..=0x7e => out.push(b as char),
            _ => out.push('.'),
        }
    }
    if bytes.len() > max {
        out.push_str("...");
    }
    out
}
