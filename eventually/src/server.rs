use chrono::prelude::*;
use compass::*;
use serde_json::json;
use serde_json::Value as JSONValue;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;

use postgres::error::Error as PGError;
use postgres::fallible_iterator::FallibleIterator;

use rocket::fairing::{self, Fairing};
use rocket::response::stream::{Event, EventStream};
use rocket::{get, http::Header, http::Status, launch, routes, Request, Response};
use rocket_sync_db_pools::{database, postgres};

use rocket::request::{self, FromRequest, Outcome};

#[derive(Debug, Clone)]
struct Query(HashMap<String, String>);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Query {
    type Error = CompassError;
    async fn from_request(req: &'r Request<'_>) -> request::Outcome<Self, Self::Error> {
        match req.uri().query() {
            Some(q) => {
                let mut hash = HashMap::new();
                for (k, v) in q.segments() {
                    hash.insert(k.to_owned(), v.to_owned());
                }
                Outcome::Success(Query(hash))
            }
            None => Outcome::Failure((Status::BadRequest, CompassError::FieldNotFound)),
        }
    }
}

#[database("eventually")]
struct CompassConn(postgres::Client);

struct CORS;
#[rocket::async_trait]
impl Fairing for CORS {
    fn info(&self) -> fairing::Info {
        fairing::Info {
            name: "CORS headers",
            kind: fairing::Kind::Response,
        }
    }

    async fn on_response<'r>(&self, _: &'r Request<'_>, response: &mut Response<'r>) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new("Access-Control-Allow-Methods", "GET"));
        response.set_header(Header::new("Access-Control-Allow-Headers", "*"));
    }
}

#[get("/events")]
async fn search(
    mut raw_req: Query,
    db: CompassConn,
    schema: Schema,
) -> Result<JSONValue, CompassError> {
    let mut req = raw_req.0;

    if let Some(mut before) = req.get_mut("before") {
        if before.parse::<i64>().is_err() {
            *before = before
                .parse::<DateTime<Utc>>()
                .unwrap()
                .timestamp()
                .to_string();
        }
    }

    if let Some(mut after) = req.get_mut("after") {
        if after.parse::<i64>().is_err() {
            *after = after
                .parse::<DateTime<Utc>>()
                .unwrap()
                .timestamp()
                .to_string();
        }
    }

    db.run(move |mut c| json_search(&mut c, &schema, &req).map(|val| json!(val)))
        .await
}

#[get("/one_of_each_type")]
async fn distinct_events(db: CompassConn) -> Result<JSONValue, CompassError> {
    db.run(move |mut c| {
        let mut evs: Vec<JSONValue> = Vec::new();
        for event_type in c.query("SELECT DISTINCT (object->'type')::integer FROM documents",&[]).map_err(CompassError::PGError)? {
            let etype: i32 = event_type.get(0);
            let row = c.query_opt(format!("SELECT object FROM documents WHERE object @@ '(($.metadata.redacted == false) || !exists($.metadata.redacted)) && $.type == {}' LIMIT 1",etype).as_str(),&[])?;

            if let Some(r) = row {
                let mut ev: JSONValue = r.get(0);
                if let Some(timest) = ev["created"].as_i64() {
                    ev["created"] = json!(DateTime::<Utc>::from_utc(
                        NaiveDateTime::from_timestamp(timest, 0),
                        Utc,
                    ).to_rfc3339());
                    evs.push(ev);
                }
            }
        }
        Ok(json!(evs))
    })
    .await
}
//
// #[get("/sse")]
// async fn events(mut conn: CompassConn) -> Result<EventStream![],CompassError> {
//     conn.run(move |c| {
//         c.execute("LISTEN events",&[]).map_err(CompassError::PGError)?;
//         Ok(EventStream! {
//             let mut notification_iter = c.notifications().blocking_iter();
//             loop {
//                 if let Ok(event) = notification_iter.next() {
//                     if let Some(e) = event {
//                         yield Event::data(e.payload());
//                     }
//                 } else {
//                     break;
//                 }
//             }
//         })
//     }).await
// }

#[launch]
fn rocket() -> _ {
    let mut file = File::open("schema.yaml").unwrap();
    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    let schema: Schema = serde_yaml::from_str(&s).unwrap();
    rocket::build()
        .manage(schema)
        .attach(CompassConn::fairing())
        .attach(CORS)
        .mount("/", routes![search,distinct_events])
}
