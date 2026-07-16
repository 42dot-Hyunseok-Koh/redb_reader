// =============================================================
//  redb_reader v0.2  –  read-only redb inspector + CBOR decoder
//
//  Key fixes over v0.1:
//   1. Adds Vec<u8> value-type guesses.  The DB was built with
//      TableDefinition<K, Vec<u8>> but v0.1 only tried &[u8].
//      redb validates the stored TypeName ("Vec<u8>" ≠ "[u8]"),
//      so every open attempt silently failed.
//   2. Decodes CBOR-encoded values into human-readable JSON-style
//      output using the ciborium crate.
// =============================================================

use ciborium::Value as Cbor;
use redb::{
    Database, MultimapTableHandle, ReadableTable, ReadableTableMetadata,
    TableDefinition, TableHandle,
};
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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
            // args[3] may be a table name or a flag such as "--limit"
            let table = args.get(3).and_then(|s| {
                if s.starts_with('-') {
                    None
                } else {
                    Some(s.as_str())
                }
            });
            dump_tables(&db_path, table, limit)?;
        }
        _ => print_usage(&args[0]),
    }

    Ok(())
}

fn print_usage(bin: &str) {
    eprintln!("redb_reader v0.2 – read-only redb inspector with CBOR decoding");
    eprintln!();
    eprintln!("Usage:");
    eprintln!("  {bin} <db-file> scan-strings");
    eprintln!("  {bin} <db-file> list");
    eprintln!("  {bin} <db-file> dump [table-name] [--limit N]");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {bin} dm_cluster_persistent.db scan-strings");
    eprintln!("  {bin} dm_cluster_persistent.db list");
    eprintln!("  {bin} dm_cluster_persistent.db dump");
    eprintln!("  {bin} dm_cluster_persistent.db dump log_metadata");
    eprintln!("  {bin} dm_cluster_persistent.db dump log_entry --limit 10");
    eprintln!("  {bin} dm_cluster_persistent.db dump --limit 200 > dump.txt");
}

fn parse_limit(args: &[String]) -> Option<usize> {
    args.windows(2)
        .find(|w| w[0] == "--limit")
        .and_then(|w| w[1].parse::<usize>().ok())
}

// ─── DB open (work on a temp copy to avoid touching the original) ───────────

fn open_db(path: &PathBuf) -> Result<Database, Box<dyn Error>> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("database.redb");
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis();
    let temp = env::temp_dir().join(format!(
        "redb_reader_{}_{}_{}", std::process::id(), ts, sanitize(file_name)
    ));
    fs::copy(path, &temp)?;
    eprintln!("Temp copy: {}", temp.display());
    Ok(Database::open(temp)?)
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect()
}

// ─── scan-strings ────────────────────────────────────────────────────────────

fn scan_strings(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let mut cur = Vec::new();
    let mut found = Vec::new();

    for b in bytes {
        if b.is_ascii_graphic() || b == b' ' {
            cur.push(b);
        } else {
            if cur.len() >= 4 {
                found.push(String::from_utf8_lossy(&cur).to_string());
            }
            cur.clear();
        }
    }
    if cur.len() >= 4 {
        found.push(String::from_utf8_lossy(&cur).to_string());
    }
    found.sort();
    found.dedup();
    for s in found { println!("{s}"); }
    Ok(())
}

// ─── list ────────────────────────────────────────────────────────────────────

fn list_tables(path: &PathBuf) -> Result<(), Box<dyn Error>> {
    let db = open_db(path)?;
    let txn = db.begin_read()?;

    println!("Tables:");
    for h in txn.list_tables()? { println!("  - {}", h.name()); }
    println!("Multimap tables:");
    for h in txn.list_multimap_tables()? { println!("  - {}", h.name()); }
    Ok(())
}

// ─── dump ────────────────────────────────────────────────────────────────────

fn dump_tables(path: &PathBuf, table: Option<&str>, limit: usize) -> Result<(), Box<dyn Error>> {
    let db = open_db(path)?;
    let txn = db.begin_read()?;

    let names: Vec<String> = if let Some(t) = table {
        vec![t.to_string()]
    } else {
        let listed: Vec<_> = txn.list_tables()?.map(|h| h.name().to_string()).collect();
        if listed.is_empty() {
            CANDIDATE_TABLES.iter().map(|s| s.to_string()).collect()
        } else {
            listed
        }
    };

    for name in names {
        println!("\n=== table: {name} ===");

        // ── Vec<u8> value types (correct for this database) ──────────────
        if dump_u64_vec(&txn, &name, limit).is_ok() { continue; }
        if dump_str_vec(&txn, &name, limit).is_ok() { continue; }

        // ── &[u8] value types (fallback for other databases) ─────────────
        if dump_str_slice(&txn, &name, limit).is_ok() { continue; }
        if dump_u64_slice(&txn, &name, limit).is_ok() { continue; }
        if dump_str_str(&txn, &name, limit).is_ok() { continue; }

        eprintln!("  ! Could not open '{name}' with any type guess.");
        eprintln!("    Tried: <u64,Vec<u8>>, <&str,Vec<u8>>, <&str,&[u8]>, <u64,&[u8]>, <&str,&str>");
    }
    Ok(())
}

// ── Vec<u8> dumps ─────────────────────────────────────────────────────────────

fn dump_u64_vec(txn: &redb::ReadTransaction, name: &str, limit: usize) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<u64, Vec<u8>> = TableDefinition::new(name);
    let tbl = txn.open_table(def)?;
    let total = tbl.len()?;
    println!("  type=<u64, Vec<u8>>  rows={total}");
    for (idx, item) in tbl.iter()?.enumerate() {
        if idx >= limit { println!("  ... (first {limit} of {total} shown, use --limit to see more)"); break; }
        let (k, v) = item?;
        let val = v.value();
        println!("  [{idx}] key={}", k.value());
        println!("       val ({} B) = {}", val.len(), decode_cbor(&val));
    }
    Ok(())
}

fn dump_str_vec(txn: &redb::ReadTransaction, name: &str, limit: usize) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<&str, Vec<u8>> = TableDefinition::new(name);
    let tbl = txn.open_table(def)?;
    let total = tbl.len()?;
    println!("  type=<&str, Vec<u8>>  rows={total}");
    for (idx, item) in tbl.iter()?.enumerate() {
        if idx >= limit { println!("  ... (first {limit} of {total} shown, use --limit to see more)"); break; }
        let (k, v) = item?;
        let val = v.value();
        println!("  [{idx}] key={:?}", k.value());
        println!("       val ({} B) = {}", val.len(), decode_cbor(&val));
    }
    Ok(())
}

// ── &[u8] fallback dumps ──────────────────────────────────────────────────────

fn dump_str_slice(txn: &redb::ReadTransaction, name: &str, limit: usize) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<&str, &[u8]> = TableDefinition::new(name);
    let tbl = txn.open_table(def)?;
    let total = tbl.len()?;
    println!("  type=<&str, &[u8]>  rows={total}");
    for (idx, item) in tbl.iter()?.enumerate() {
        if idx >= limit { println!("  ... truncated at {limit}"); break; }
        let (k, v) = item?;
        print_row(idx, k.value().as_bytes(), v.value());
    }
    Ok(())
}

fn dump_u64_slice(txn: &redb::ReadTransaction, name: &str, limit: usize) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<u64, &[u8]> = TableDefinition::new(name);
    let tbl = txn.open_table(def)?;
    let total = tbl.len()?;
    println!("  type=<u64, &[u8]>  rows={total}");
    for (idx, item) in tbl.iter()?.enumerate() {
        if idx >= limit { println!("  ... truncated at {limit}"); break; }
        let (k, v) = item?;
        println!("  [{idx}] key_u64={}", k.value());
        print_row(idx, &k.value().to_le_bytes(), v.value());
    }
    Ok(())
}

fn dump_str_str(txn: &redb::ReadTransaction, name: &str, limit: usize) -> Result<(), Box<dyn Error>> {
    let def: TableDefinition<&str, &str> = TableDefinition::new(name);
    let tbl = txn.open_table(def)?;
    let total = tbl.len()?;
    println!("  type=<&str, &str>  rows={total}");
    for (idx, item) in tbl.iter()?.enumerate() {
        if idx >= limit { println!("  ... truncated at {limit}"); break; }
        let (k, v) = item?;
        print_row(idx, k.value().as_bytes(), v.value().as_bytes());
    }
    Ok(())
}

// ─── CBOR decoder ────────────────────────────────────────────────────────────

fn decode_cbor(bytes: &[u8]) -> String {
    match ciborium::de::from_reader::<Cbor, _>(bytes) {
        Ok(v) => format_cbor(&v, 0),
        Err(_) => format!("(raw: {})", text_preview(bytes, 256)),
    }
}

fn format_cbor(v: &Cbor, depth: usize) -> String {
    // Indent constants: each level adds 4 spaces
    let pad   = "    ".repeat(depth);
    let inner = "    ".repeat(depth + 1);

    match v {
        Cbor::Text(s) => format!("{s:?}"),

        Cbor::Integer(i) => {
            // ciborium::Integer wraps i128; try common sizes first
            if let Ok(n) = u64::try_from(*i) {
                format!("{n}")
            } else if let Ok(n) = i64::try_from(*i) {
                format!("{n}")
            } else {
                format!("{v:?}")
            }
        }

        Cbor::Bytes(b) => format!("0x{}", hex_preview(b, 64)),

        Cbor::Bool(b) => format!("{b}"),

        Cbor::Null => "null".to_string(),

        Cbor::Float(f) => format!("{f}"),

        Cbor::Array(arr) => {
            if arr.is_empty() { return "[]".to_string(); }
            let items: Vec<String> = arr.iter()
                .map(|item| format!("{inner}{}", format_cbor(item, depth + 1)))
                .collect();
            format!("[\n{}\n{pad}]", items.join(",\n"))
        }

        Cbor::Map(pairs) => {
            if pairs.is_empty() { return "{}".to_string(); }
            let items: Vec<String> = pairs.iter()
                .map(|(k, val)| {
                    format!("{inner}{}: {}", format_cbor(k, depth + 1), format_cbor(val, depth + 1))
                })
                .collect();
            format!("{{\n{}\n{pad}}}", items.join(",\n"))
        }

        Cbor::Tag(tag, inner_val) => {
            format!("tag({tag}, {})", format_cbor(inner_val.as_ref(), depth))
        }

        // Catch-all for any future ciborium variants
        _ => format!("{v:?}"),
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn print_row(idx: usize, key: &[u8], value: &[u8]) {
    println!("  [{idx}]");
    println!("    key_text : {}", text_preview(key, 128));
    println!("    key_hex  : {}", hex_preview(key, 64));
    println!("    val ({} B): {}", value.len(), decode_cbor(value));
}

fn hex_preview(bytes: &[u8], max: usize) -> String {
    let take = bytes.len().min(max);
    let mut out = String::with_capacity(take * 2);
    for b in &bytes[..take] { out.push_str(&format!("{b:02x}")); }
    if bytes.len() > max { out.push_str("..."); }
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
    if bytes.len() > max { out.push_str("..."); }
    out
}
