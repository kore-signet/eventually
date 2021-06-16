use chrono::prelude::*;
use log::{error, info};
use postgres::{Client as DBClient, NoTls};
use reqwest::StatusCode;
use serde_json::{json, Value as JSONValue};
use std::collections::HashSet;
use std::env;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn main() {
    env_logger::init();

    let client = reqwest::blocking::Client::new();
    let mut db = DBClient::connect(&env::var("DB_URL").unwrap(), NoTls).unwrap();

    let sleep_for =
        Duration::from_millis((&env::var("POLL_DELAY").unwrap()).parse::<u64>().unwrap());

    let library_poll_delay = Duration::from_secs(
        (&env::var("LIBRARY_POLL_DELAY").unwrap_or("120".to_owned()))
            .parse::<u64>()
            .unwrap(),
    );

    let mut last_library_fetch = Instant::now();

    let mut last_batch: HashSet<String> = HashSet::new();
    let mut latest = String::new();

    'poll_loop: loop {
        // library fetch time
        if last_library_fetch.elapsed() >= library_poll_delay {
            match client.get("https://raw.githubusercontent.com/xSke/blaseball-site-files/main/data/library.json").send() {
                Ok(res) => {
                    if let Ok(library) = res.json::<JSONValue>() {
                        for book in library.as_array().unwrap_or(&vec![]) {
                            for chapter in book["chapters"].as_array().unwrap_or(&vec![]) {
                                match client.get("https://www.blaseball.com/database/feed/story").query(&vec![("id",chapter["id"].as_str())]).send() {
                                    Ok(r) => {
                                        if r.status() == StatusCode::OK {
                                            if let Ok(events) = r.json::<JSONValue>() {
                                                let mut fake_batch = HashSet::new();
                                                let new_events = events
                                                    .as_array()
                                                    .unwrap()
                                                    .into_iter()
                                                    .cloned()
                                                    .collect::<Vec<JSONValue>>();
                                                ingest(new_events, &mut db, &mut fake_batch);
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        error!("Couldn't fetch events from library {:?}",e);
                                    }
                                }
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Couldn't fetch library json {:?}",e);
                }
            }

            last_library_fetch = Instant::now();
        }

        let parameters = if latest.len() > 0 {
            vec![("limit", "100"), ("sort", "1"), ("start", &latest)]
        } else {
            vec![("limit", "100"), ("sort", "0")]
        };

        match client
            .get("https://www.blaseball.com/database/feed/global")
            .query(&parameters)
            .send()
        {
            Ok(res) => {
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

                    latest = ingest(new_events, &mut db, &mut last_batch);
                } else {
                    error!("Couldn't parse response from blaseball as JSON");
                }
            }
            Err(e) => {
                error!("Couldn't reach Blaseball API: {:?}", e);
            }
        }

        thread::sleep(sleep_for);
    }
}

fn ingest(
    new_events: Vec<JSONValue>,
    db: &mut DBClient,
    last_batch: &mut HashSet<String>,
) -> String {
    *last_batch = HashSet::new();

    let mut trans = db.transaction().unwrap(); // trans rights!
    let latest = new_events[new_events.len() - 1]["created"]
        .as_str()
        .unwrap()
        .to_owned();

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
            "INSERT INTO documents (doc_id, object) VALUES ($1, $2) ON CONFLICT DO UPDATE SET object = $2",
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

    latest
}
