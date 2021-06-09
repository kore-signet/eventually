use chrono::prelude::*;
use log::{error, info};
use postgres::{Client as DBClient, NoTls};
use serde_json::{json, Value as JSONValue};
use std::collections::HashSet;
use std::env;
use std::thread;
use std::time::Duration;
use uuid::Uuid;

fn main() {
    env_logger::init();

    let client = reqwest::blocking::Client::new();
    let mut db = DBClient::connect(&env::var("DB_URL").unwrap(), NoTls).unwrap();

    let sleep_for =
        Duration::from_millis((&env::var("POLL_DELAY").unwrap()).parse::<u64>().unwrap());

    let mut last_batch: HashSet<String> = HashSet::new();
    let mut latest = String::new();

    'poll_loop: loop {
        let parameters = if latest.len() > 0 {
            vec![("limit", "100"), ("sort", "1"), ("start", &latest)]
        } else {
            vec![("limit", "100"), ("sort", "0")]
        };

        if let Ok(res) = client
            .get("https://www.blaseball.com/database/feed/global")
            .query(&parameters)
            .send()
        {
            if let Ok(events) = res.json::<JSONValue>() {
                let new_events = events
                    .as_array()
                    .unwrap()
                    .into_iter()
                    .filter(|x| !(last_batch.contains(x["id"].as_str().unwrap())))
                    .cloned()
                    .collect::<Vec<JSONValue>>();

                if new_events.len() < 1 {
                    thread::sleep(sleep_for);
                    continue 'poll_loop;
                }

                info!("Ingesting {} new events!", new_events.len());

                last_batch = HashSet::new();
                latest = new_events[new_events.len() - 1]["created"]
                    .as_str()
                    .unwrap()
                    .to_owned();

                let mut trans = db.transaction().unwrap(); // trans rights!

                for mut e in new_events {
                    last_batch.insert(e["id"].as_str().unwrap().to_owned());

                    e["created"] = json!(e["created"]
                        .as_str()
                        .unwrap()
                        .parse::<DateTime<Utc>>()
                        .unwrap()
                        .timestamp());

                    let id = Uuid::parse_str(e["id"].as_str().unwrap()).unwrap();

                    match trans.execute(
                        "INSERT INTO documents (doc_id, object) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                        &[&id, &e],
                    ) {
                        Ok(_) => {}
                        Err(e) => error!("Couldn't add event to database -> {:?}", e),
                    };

                    // match trans.execute("NOTIFY events $1", &[&e.to_string()]) {
                    //     Ok(_) => {}
                    //     Err(e) => error!("Couldn't send event notification -> {:?}", e),
                    // };
                }

                match trans.commit() {
                    Ok(_) => {}
                    Err(e) => error!("Couldn't commit transaction -> {:?}", e),
                };
            } else {
                error!("Couldn't parse response from blaseball as JSON");
            }
        } else {
            error!("Couldn't reach Blaseball API!");
        }

        thread::sleep(sleep_for);
    }
}
