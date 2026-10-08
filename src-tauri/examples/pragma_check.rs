use variable_lib::db;

fn main() {
    let arg = std::env::args().nth(1).expect("pass db path");
    let conn = db::open_conn(std::path::Path::new(&arg)).expect("open");
    let uv: i32 = conn.query_row("PRAGMA user_version;", [], |r| r.get(0)).unwrap();
    println!("user_version = {uv}");
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    println!("tables: {tables:?}");
    let ncolls: Vec<String> = conn
        .prepare("PRAGMA table_info(nodes)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    println!("node cols: {ncolls:?}");
}
