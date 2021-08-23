use chrono::prelude::*;
use compass::*;
use serde_json::json;
use serde_json::Value as JSONValue;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;

use uuid::Uuid;

use postgres::error::Error as PGError;
use postgres::fallible_iterator::FallibleIterator;

use rocket::fairing::{self, Fairing};
use rocket::serde::json::Json as RocketJson;
use rocket::{
    get, http::Header, http::Status, launch, options, response, routes, Request, Response,
};

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

#[rocket::async_trait]
impl<'r> response::Responder<'r, 'static> for CORS {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'static> {
        Response::build()
            .header(Header::new("Access-Control-Allow-Origin", "*"))
            .header(Header::new("Access-Control-Allow-Methods", "GET"))
            .header(Header::new("Access-Control-Allow-Headers", "*"))
            .header(Header::new("Access-Control-Max-Age", "86400"))
            .header(Header::new("Allow", "OPTIONS, GET"))
            .status(Status::NoContent)
            .ok()
    }
}

#[options("/<_..>")]
async fn cors_preflight() -> CORS {
    CORS
}

#[get("/events")]
async fn search(
    mut raw_req: Query,
    db: CompassConn,
    schema: Schema,
) -> Result<RocketJson<Vec<JSONValue>>, CompassError> {
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

    let expand_children = req
        .remove("expand_children")
        .and_then(|c| c.parse::<bool>().ok())
        .unwrap_or(false);
    let expand_parent = req
        .remove("expand_parent")
        .and_then(|c| c.parse::<bool>().ok())
        .unwrap_or(false);

    db.run(move |mut c| match json_search(&mut c, &schema, &req) {
        Ok(v) => v
            .into_iter()
            .map(|mut event| {
                if expand_children {
                    if let Some(children) = event
                        .get("metadata")
                        .and_then(|i| i.get("children"))
                        .and_then(|i| i.as_array())
                    {
                        event["metadata"]["children"] = json!(get_by_ids(
                            &mut c,
                            &schema,
                            &children
                                .into_iter()
                                .filter_map(|i| i.as_str())
                                .filter_map(|i| Uuid::parse_str(i).ok())
                                .collect()
                        )?);
                    }
                }

                if expand_parent {
                    if let Some(parent) = event
                        .get("metadata")
                        .and_then(|i| i.get("parent"))
                        .and_then(|i| i.as_str())
                        .and_then(|i| Uuid::parse_str(i).ok())
                    {
                        event["metadata"]["parent"] = json!(get_by_ids(
                            &mut c,
                            &schema,
                            &vec![parent]
                        )?.first());
                    }
                }

                Ok(event)
            })
            .collect::<Result<Vec<JSONValue>, CompassError>>()
            .map(|v| RocketJson(v)),
        Err(e) => Err(e),
    })
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

#[get("/versions?<id>")]
async fn get_versions(db: CompassConn, id: String) -> Result<JSONValue, CompassError> {
    db.run(move |mut c| {
        let id = Uuid::parse_str(id.as_str()).unwrap();
        let results = c
            .query("SELECT object FROM versions WHERE doc_id = $1", &[&id])
            .map_err(CompassError::PGError)?;
        Ok(json!(results
            .into_iter()
            .map(|row| {
                let mut ev: JSONValue = row.get(0);
                if let Some(timest) = ev["created"].as_i64() {
                    ev["created"] = json!(DateTime::<Utc>::from_utc(
                        NaiveDateTime::from_timestamp(timest, 0),
                        Utc,
                    )
                    .to_rfc3339());
                }
                ev
            })
            .collect::<Vec<JSONValue>>()))
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
        .mount(
            "/",
            routes![search, distinct_events, get_versions, cors_preflight],
        )
}
