use chrono::prelude::*;
use postgres::{Client, NoTls};
use serde_json::{json, Value};
use std::env;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use uuid::Uuid;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let file = File::open(&args[1])?;
    let reader = BufReader::new(file);
    let mut client = Client::connect(&args[2], NoTls).unwrap();

    let mut trans = client.transaction().unwrap();

    let statement = trans
        .prepare("INSERT INTO documents (doc_id, object) VALUES ($1,$2) ON CONFLICT (doc_id) DO UPDATE SET object = $2")
        .unwrap();

    for (i, l) in reader.lines().enumerate() {
        println!("#{}", i);
        let mut v: Value = serde_json::from_str(&l.unwrap()).unwrap();
        let uuid = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
        v["created"] = json!(v["created"]
            .as_str()
            .unwrap()
            .parse::<DateTime<Utc>>()
            .unwrap()
            .timestamp());
        trans.execute(&statement, &[&uuid, &v]).unwrap();
    }

    trans.commit().unwrap();

    Ok(())
}
