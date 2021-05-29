use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use tokio_postgres::{NoTls, Error};
use serde_json::{Value,json};
use uuid::Uuid;
use std::env;
use chrono::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let file = File::open(&args[1])?;
    let mut reader = BufReader::new(file);
    let (mut client, connection) = tokio_postgres::connect(&args[2], NoTls).await.unwrap();

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    let mut trans = client.transaction().await.unwrap();

    let statement = trans.prepare("INSERT INTO documents (doc_id, object) VALUES ($1,$2)").await.unwrap();

    for (i,l) in reader.lines().enumerate() {
        println!("#{}",i);
        let mut v: Value = serde_json::from_str(&l.unwrap()).unwrap();
        let uuid = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
        v["created"] = json!(v["created"].as_str().unwrap().parse::<DateTime<Utc>>().unwrap().timestamp());
        trans.execute(&statement,&[&uuid,&v]).await.unwrap();
    }

    trans.commit().await.unwrap();

    Ok(())
}
