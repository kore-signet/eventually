use chrono::prelude::*;
use compass::*;
use std::collections::HashMap;

use rocket::fairing::{self, Fairing};
use rocket::{http::Header, http::Status, options, response, Request, Response};

use rocket_sync_db_pools::{database, postgres};

use rocket::request::{self, FromRequest, Outcome};

mod apis;
pub use apis::*;

#[derive(Debug, Clone)]
pub struct Query(HashMap<String, String>);

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
            None => Outcome::Success(Query(HashMap::new())),
        }
    }
}

#[database("eventually")]
pub struct CompassConn(postgres::Client);

pub struct CORS;
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
pub async fn cors_preflight() -> CORS {
    CORS
}
