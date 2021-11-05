use chrono::prelude::*;
use log::{debug, error, info};
use postgres::{Client as DBClient, NoTls};
use serde_json::{json, Value as JSONValue};
use std::env;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

macro_rules! report_error {
    ($e:expr, $where:expr) => {
        if let Err(err) = $e {
            error!("Error in {}: {}", $where, err);
        }
    };
}

fn poll_library(db: &mut DBClient, client: &reqwest::blocking::Client) -> anyhow::Result<()> {
    let library = client
        .get("https://raw.githubusercontent.com/xSke/blaseball-site-files/main/data/library.json")
        .send()
        .and_then(|r| r.json::<JSONValue>())?;
    for book in library.as_array().unwrap_or(&vec![]) {
        for chapter in book["chapters"].as_array().unwrap_or(&vec![]) {
            if !chapter["redacted"].as_bool().unwrap_or(false) {
                let events = client
                    .get("https://api.blaseball.com/database/feed/story")
                    .query(&vec![("id", chapter["id"].as_str())])
                    .send()
                    .and_then(|r| r.json::<Vec<JSONValue>>())?
                    .into_iter()
                    .map(|mut e| {
                        e["metadata"]["_eventually_book_title"] = book["title"].clone();
                        e["metadata"]["_eventually_chapter_id"] = chapter["id"].clone();
                        e["metadata"]["_eventually_chapter_title"] = chapter["title"].clone();
                        e
                    })
                    .collect::<Vec<JSONValue>>();
                info!(
                    "ingesting {} library events - book {}, chapter {}",
                    events.len(),
                    book["title"],
                    chapter["title"]
                );
                ingest(events, db, "blaseball.com_library".to_owned())?;
            }
        }
    }

    Ok(())
}

fn ingest_from_url(
    db: &mut DBClient,
    client: &reqwest::blocking::Client,
    source: &str,
    url: &str,
    parameters: Vec<(&str, String)>,
) -> anyhow::Result<Option<DateTime<Utc>>> {
    let events = client
        .get(url)
        .query(&parameters)
        .send()
        .and_then(|r| r.json::<Vec<JSONValue>>())?;
    if events.len() < 1 {
        info!("got no events from source {}", source);
        return Ok(None);
    }

    info!(
        "Ingesting {} new events from source {}!",
        events.len(),
        source
    );
    Ok(Some(ingest(events, db, source.to_owned())?))
}

fn poll_redacted(db: &mut DBClient, client: &reqwest::blocking::Client) -> anyhow::Result<()> {
    let redacted_events = db.query("SELECT object FROM documents WHERE object @@ '($.metadata.redacted == true) && (!exists($.metadata._eventually_book_title))'", &[])?;

    for redacted_e in redacted_events {
        let redacted_e_obj = redacted_e.get::<&str, JSONValue>("object");
        let timestamp = Utc
            .timestamp(redacted_e_obj["created"].as_i64().unwrap(), 0)
            .to_rfc3339();

        ingest_from_url(
            db,
            &client,
            "blaseball.com",
            "https://api.blaseball.com/database/feed/global",
            vec![
                ("limit", "100".to_owned()),
                ("sort", "1".to_owned()),
                ("start", timestamp),
            ],
        )?;
    }

    Ok(())
}

fn main() {
    env_logger::init();

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(
            env::var("REQUEST_TIMEOUT")
                .unwrap_or("5".to_owned())
                .parse::<u64>()
                .unwrap(),
        ))
        .user_agent("Eventually/0.1 (+https://cat-girl.gay)")
        .build()
        .unwrap();
    let mut db = DBClient::connect(&env::var("DB_URL").unwrap(), NoTls).unwrap();

    let sleep_for =
        Duration::from_millis((&env::var("POLL_DELAY").unwrap()).parse::<u64>().unwrap());

    let library_poll_delay = Duration::from_secs(
        (&env::var("LIBRARY_POLL_DELAY").unwrap_or("120".to_owned()))
            .parse::<u64>()
            .unwrap(),
    );

    let _redacted_poll_delay = Duration::from_secs(
        (&env::var("REDACTED_POLL_DELAY").unwrap_or("120".to_owned()))
            .parse::<u64>()
            .unwrap(),
    );

    let mut last_library_fetch = Instant::now();
    let _last_redacted_fetch = Instant::now();

    let mut latest: Option<DateTime<Utc>> = None;

    loop {
        if last_library_fetch.elapsed() >= library_poll_delay {
            report_error!(poll_library(&mut db, &client), "library ingest");
            last_library_fetch = Instant::now();
        }

        let blaseball_params = if let Some(timestamp) = latest {
            vec![
                ("limit", "100".to_owned()),
                ("sort", "1".to_owned()),
                ("start", timestamp.to_rfc3339()),
            ]
        } else {
            vec![("limit", "100".to_owned()), ("sort", "0".to_owned())]
        };

        match ingest_from_url(
            &mut db,
            &client,
            "blaseball.com",
            "https://api.blaseball.com/database/feed/global",
            blaseball_params,
        ) {
            Ok(time) => {
                if time.is_some() {
                    latest = time;
                }
            }
            Err(e) => {
                error!("Error in main ingest: {}", e);
            }
        };

        // report_error!(
        //     ingest_from_url(
        //         &mut db,
        //         &client,
        //         "upnuts",
        //         "https://api.sibr.dev/upnuts/gc/ingested",
        //         vec![],
        //     ),
        //     "upnuts ingest"
        // );

        // println!("{:?}",sleep_for);
        thread::sleep(sleep_for);
    }
}

fn ingest(
    new_events: Vec<JSONValue>,
    db: &mut DBClient,
    source: String,
) -> anyhow::Result<DateTime<Utc>> {
    let mut trans = db.transaction().unwrap(); // trans rights!
    let latest = new_events[new_events.len() - 1]["created"]
        .as_str()
        .unwrap()
        .parse::<DateTime<Utc>>()
        .unwrap();

    let mut changed_events = 0;

    for mut e in new_events {
        e["created"] = json!(e["created"]
            .as_str()
            .unwrap()
            .parse::<DateTime<Utc>>()
            .unwrap()
            .timestamp_millis());

        e["metadata"]["_eventually_ingest_source"] = json!(&source);

        e["metadata"]["_eventually_ingest_time"] = json!(Utc::now().timestamp());

        let id = Uuid::parse_str(e["id"].as_str().unwrap()).unwrap();

        let possible_old_event =
            trans.query_opt("SELECT object FROM documents WHERE doc_id = $1", &[&id])?;

        let inserted_r = trans.query_one(
            "INSERT INTO documents (doc_id, object) VALUES ($1, $2) ON CONFLICT (doc_id) DO UPDATE SET object = $2 RETURNING (xmax=0) AS inserted",
            &[&id, &e],
        )?;

        if !inserted_r.get::<&str, bool>("inserted") {
            debug!("Event {} updated; checking if changed meaningfully", id);
            let changed_r = trans.query(
                        "SELECT true AS existed FROM versions WHERE doc_id = $1 AND (((object::jsonb #- '{metadata,scales}') #- '{nuts}') #- '{metadata,_eventually_ingest_time}') @> ((($2::jsonb #- '{metadata,scales}') #- '{nuts}') #- '{metadata,_eventually_ingest_time}') AND (((object::jsonb #- '{metadata,scales}') #- '{nuts}') #- '{metadata,_eventually_ingest_time}') <@ ((($2::jsonb #- '{metadata,scales}') #- '{nuts}') #- '{metadata,_eventually_ingest_time}')",                    &[&id, &e]
            ).unwrap();

            if changed_r.len() < 1 {
                debug!("Found changed event {:?}", id);
                if let Some(old_e) = possible_old_event {
                    let insert_statement = trans.prepare(
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
                    )?;
                    trans.query_one(
                        &insert_statement,
                        &[
                            &id,
                            &old_e.get::<&str, JSONValue>("object"),
                            &(Utc::now().timestamp_millis()),
                        ],
                    )?;
                    let _e_hash = trans
                        .query_one(
                            &insert_statement,
                            &[&id, &e, &(Utc::now().timestamp_millis())],
                        )?
                        .get::<&str, String>("hash");
                    changed_events += 1;
                    // trans.execute("SELECT pg_notify('changed_events',$1)", &[&e_hash])?;
                } else {
                    trans.execute(
                        "SELECT pg_notify('new_events',$1)",
                        &[&e["id"].as_str().unwrap()],
                    )?;
                }
            }
        };
    }

    trans.commit()?;

    info!(
        "ingested {} changed events from source {}",
        changed_events, source
    );

    Ok(latest)
}
