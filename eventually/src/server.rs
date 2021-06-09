use chrono::prelude::*;
use compass::*;
use serde_json::json;
use serde_json::Value as JSONValue;
use std::collections::HashMap;
use std::fs::File;
use std::io::prelude::*;

use rocket::fairing::{self, Fairing};
use rocket::{get, http::Header, launch, routes, Request, Response};
use rocket_sync_db_pools::{database, postgres};

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

#[get("/events?<req..>")]
async fn search(
    mut req: HashMap<String, String>,
    db: CompassConn,
    schema: Schema,
) -> Result<JSONValue, CompassError> {
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
        .mount("/", routes![search])
}
