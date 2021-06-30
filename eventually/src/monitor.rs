use chrono::prelude::*;
use log::{error, info, debug};
use postgres::{Client as DBClient, NoTls};
use reqwest::StatusCode;
use serde_json::{json, Value as JSONValue};
use std::env;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

fn main() {
    env_logger::init();

    let client = reqwest::blocking::Client::builder()
        .user_agent("Eventually/0.1 (+https://cat-girl.gay)")
        .build().unwrap();
    let mut db = DBClient::connect(&env::var("DB_URL").unwrap(), NoTls).unwrap();

    let sleep_for =
        Duration::from_millis((&env::var("POLL_DELAY").unwrap()).parse::<u64>().unwrap());

    let library_poll_delay = Duration::from_secs(
        (&env::var("LIBRARY_POLL_DELAY").unwrap_or("120".to_owned()))
            .parse::<u64>()
            .unwrap(),
    );

    let mut last_library_fetch = Instant::now();

    let mut latest = String::new();

    'poll_loop: loop {
        // library fetch time
        if last_library_fetch.elapsed() >= library_poll_delay {
            match client.get("https://raw.githubusercontent.com/xSke/blaseball-site-files/main/data/library.json").send() {
                Ok(res) => {
                    if let Ok(library) = res.json::<JSONValue>() {
                        for book in library.as_array().unwrap_or(&vec![]) {
                            for chapter in book["chapters"].as_array().unwrap_or(&vec![]) {
                                if !chapter["redacted"].as_bool().unwrap_or(false) {
                                    match client.get("https://www.blaseball.com/database/feed/story").query(&vec![("id",chapter["id"].as_str())]).send() {
                                        Ok(r) => {
                                            if r.status() == StatusCode::OK {
                                                if let Ok(events) = r.json::<JSONValue>() {
                                                    let new_events = events
                                                        .as_array()
                                                        .unwrap()
                                                        .into_iter()
                                                        .cloned()
                                                        .map(|mut e| {
                                                            e["metadata"]["_eventually_book_title"] = book["title"].clone();
                                                            e["metadata"]["_eventually_chapter_id"] = chapter["id"].clone();
                                                            e
                                                        })
                                                        .collect::<Vec<JSONValue>>();
                                                    ingest(new_events, &mut db);
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
                        .cloned()
                        .collect::<Vec<JSONValue>>();

                    if new_events.len() < 1 {
                        thread::sleep(sleep_for);
                        continue 'poll_loop;
                    }

                    info!("Ingesting {} new events!", new_events.len());

                    latest = ingest(new_events, &mut db);
                } else {
                    error!("Couldn't parse response from blaseball as JSON");
                }
            }
            Err(e) => {
                error!("Couldn't reach Blaseball API: {:?}", e);
            }
        }

        match client
            .get("https://api.sibr.dev/upnuts/gc/ingested")
            .send()
        {
            Ok(res) => {
                if let Ok(events) = res.json::<JSONValue>() {
                    let new_events = events
                        .as_array()
                        .unwrap()
                        .into_iter()
                        .cloned()
                        .collect::<Vec<JSONValue>>();

                    if new_events.len() < 1 {
                        thread::sleep(sleep_for);
                        continue 'poll_loop;
                    }

                    info!("Ingesting {} new events from upnuts!", new_events.len());

                    ingest(new_events, &mut db);
                } else {
                    error!("Couldn't parse response from upnuts as JSON");
                }
            }
            Err(e) => {
                error!("Couldn't reach upnuts API: {:?}", e);
            }
        }


        thread::sleep(sleep_for);
    }
}

fn ingest(
    new_events: Vec<JSONValue>,
    db: &mut DBClient
) -> String {

    let mut trans = db.transaction().unwrap(); // trans rights!
    let latest = new_events[new_events.len() - 1]["created"]
        .as_str()
        .unwrap()
        .to_owned();

    for mut e in new_events {
        e["created"] = json!(e["created"]
            .as_str()
            .unwrap()
            .parse::<DateTime<Utc>>()
            .unwrap()
            .timestamp());

        e["metadata"]["_eventually_ingest_time"] = json!(Utc::now().timestamp());

        let id = Uuid::parse_str(e["id"].as_str().unwrap()).unwrap();

        match trans.query_one(
            "INSERT INTO documents (doc_id, object) VALUES ($1, $2) ON CONFLICT (doc_id) DO UPDATE SET object = $2 RETURNING (xmax=0) AS inserted",
            &[&id, &e],
        ) {
            Ok(inserted_r) => {
                if !inserted_r.get::<&str,bool>("inserted") {
                    debug!("Event {} updated; checking if changed meaningfully",id);
                    match trans.query_opt(
                        "SELECT true AS existed FROM versions WHERE doc_id = $1 AND ((object::jsonb #- '{metadata,scales}') #- '{nuts}') @> (($2::jsonb #- '{metadata,scales}') #- '{nuts}') AND ((object::jsonb #- '{metadata,scales}') #- '{nuts}') <@ (($2::jsonb #- '{metadata,scales}') #- '{nuts}')",
                        &[&id, &e]
                    ) {
                        Ok(changed_r) => {
                            if let None = changed_r {
                                info!("Found changed event {:?}",id);
                                match trans.query_one(
                                    "INSERT INTO versions (doc_id,object,observed,hash) VALUES ($1,$2,$3,
                                        encode(
                                            sha256(
                                                convert_to(
                                                    ($2::jsonb #>> '{}'),
                                                    'UTF8'
                                                )
                                            ),
                                        'hex')
                                    )
                                    RETURNING hash",
                                    &[&id,&e,&(Utc::now().timestamp_millis())]
                                ) {
                                    Ok(version_r) => {
                                        info!("Inserted changed event {:?}",id);
                                        let e_hash = version_r.get::<&str,String>("hash");
                                        match trans.execute("SELECT pg_notify('changed_events',$1)", &[&e_hash]) {
                                            Ok(_) => {}
                                            Err(e) => error!("Couldn't send event notification -> {:?}", e),
                                        };
                                    },
                                    Err(e) => {
                                        error!("Couldn't add changed event {:?}: {:?}", id, e);
                                    }
                                }
                            }
                        },
                        Err(e) => {
                            error!("Couldn't check for event {:?} in versions: {:?}", id, e);
                        }
                    }
                } else {
                    match trans.execute("SELECT pg_notify('new_events',$1)", &[&e["id"].as_str().unwrap()]) {
                        Ok(_) => {}
                        Err(e) => error!("Couldn't send event notification -> {:?}", e),
                    };
                }
            },
            Err(e) => error!("Couldn't add event to database -> {:?}", e),
        };
    }

    match trans.commit() {
        Ok(_) => {}
        Err(e) => error!("Couldn't commit transaction -> {:?}", e),
    };

    latest
}
