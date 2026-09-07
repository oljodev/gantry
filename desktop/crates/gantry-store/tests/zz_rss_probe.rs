//! TEMPORARY probe - deleted after running.
use gantry_store::{Connection, Store};
use std::time::Instant;

fn rss_kb() -> i64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    for l in s.lines() {
        if let Some(v) = l.strip_prefix("VmRSS:") {
            return v.trim().trim_end_matches(" kB").trim().parse().unwrap();
        }
    }
    -1
}
fn vsz_kb() -> i64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    for l in s.lines() {
        if let Some(v) = l.strip_prefix("VmSize:") {
            return v.trim().trim_end_matches(" kB").trim().parse().unwrap();
        }
    }
    -1
}
fn mark(tag: &str) {
    println!("PROBE {tag}: rss={} kB vsz={} kB", rss_kb(), vsz_kb());
}

#[test]
fn probe_store_memory() {
    mark("00-baseline");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.db");
    let store = Store::open(&path).unwrap();
    mark("01-after-store-open(1 writer + 3 readers)");

    // pragma values actually in force on a pooled read connection
    for p in [
        "cache_size",
        "page_size",
        "mmap_size",
        "temp_store",
        "journal_mode",
        "wal_autocheckpoint",
        "synchronous",
        "journal_size_limit",
        "threads",
        "soft_heap_limit",
        "hard_heap_limit",
    ] {
        let v: String = store
            .read(|c| {
                Ok(c.query_row(&format!("PRAGMA {p}"), [], |r| {
                    r.get::<_, rusqlite_value::V>(0)
                })
                .map(|x| x.0)?)
            })
            .unwrap_or_else(|_| "?".into());
        println!("PROBE pragma {p} = {v}");
    }

    // 100 extra connections, to price a connection
    let before = rss_kb();
    let mut conns = Vec::new();
    for _ in 0..100 {
        let c = Connection::open(&path).unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        conns.push(c);
    }
    let after = rss_kb();
    println!(
        "PROBE 100-idle-connections: delta={} kB => {} kB/conn",
        after - before,
        (after - before) as f64 / 100.0
    );

    // touch each connection with a real query so its page cache warms
    for c in &conns {
        let _: i64 = c
            .query_row("SELECT count(*) FROM chats", [], |r| r.get(0))
            .unwrap();
    }
    let after2 = rss_kb();
    println!(
        "PROBE 100-warm-connections: delta={} kB => {} kB/conn",
        after2 - before,
        (after2 - before) as f64 / 100.0
    );
    drop(conns);
    mark("02-after-dropping-100-conns");
}

mod rusqlite_value {
    pub struct V(pub String);
    impl rusqlite::types::FromSql for V {
        fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
            Ok(V(match v {
                rusqlite::types::ValueRef::Null => "NULL".into(),
                rusqlite::types::ValueRef::Integer(i) => i.to_string(),
                rusqlite::types::ValueRef::Real(f) => f.to_string(),
                rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                rusqlite::types::ValueRef::Blob(_) => "blob".into(),
            }))
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_writer_throughput() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("t.db")).unwrap();
    mark("10-open");

    // one transaction per write() call, exactly what PersistSink does per 16 ms flush
    let n = 2000;
    let t = Instant::now();
    for i in 0..n {
        store
            .write(move |c| {
                c.execute(
                    "INSERT INTO settings (key, value_json, updated_at) VALUES (?1, '1', 0)
                     ON CONFLICT(key) DO UPDATE SET value_json = '2'",
                    [format!("k{i}")],
                )?;
                Ok(())
            })
            .await
            .unwrap();
    }
    let el = t.elapsed();
    println!(
        "PROBE writer: {n} single-row transactions in {:?} => {:.0} tx/s ({:.3} ms/tx)",
        el,
        n as f64 / el.as_secs_f64(),
        el.as_secs_f64() * 1000.0 / n as f64
    );

    // batch of 20 rows per transaction (a fuller flush)
    let t = Instant::now();
    let batches = 500;
    for b in 0..batches {
        store
            .write(move |c| {
                let tx = c.transaction()?;
                for j in 0..20 {
                    tx.execute(
                        "INSERT INTO settings (key, value_json, updated_at) VALUES (?1, '1', 0)
                         ON CONFLICT(key) DO UPDATE SET value_json='3'",
                        [format!("b{b}-{j}")],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
            .unwrap();
    }
    let el = t.elapsed();
    println!(
        "PROBE writer: {batches} x 20-row transactions in {:?} => {:.0} tx/s, {:.0} rows/s",
        el,
        batches as f64 / el.as_secs_f64(),
        (batches * 20) as f64 / el.as_secs_f64()
    );
    let wal = dir.path().join("t.db-wal");
    if let Ok(m) = std::fs::metadata(&wal) {
        println!("PROBE wal size after writes: {} bytes", m.len());
    }
    mark("11-after-writes");
}
