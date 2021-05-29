use compass::*;
use actix_web::{get, web, App, HttpServer, HttpResponse, Error, middleware::Logger};
use deadpool_postgres::{Config, ManagerConfig, Client, Pool, RecyclingMethod };
use tokio_postgres::{NoTls};
use std::fs::File;
use std::io::prelude::*;
use serde_json::Value as JSONValue;
use serde_json::json;
use chrono::prelude::*;
use actix_cors::Cors;

#[get("/events")]
async fn search(mut req: web::Query<JSONValue>, db_pool: web::Data<Pool>, schema: web::Data<Schema>) -> Result<HttpResponse,Error> {
    let client: Client = db_pool.get().await.map_err(CompassError::PoolError)?;

    if let Some(mut before) = req.get_mut("before") {
        if before.as_str().unwrap().parse::<i64>().is_err() {
            *before = json!(before.as_str().unwrap().parse::<DateTime<Utc>>().unwrap().timestamp().to_string());
        }
    }

    if let Some(mut after) = req.get_mut("after") {
        if after.as_str().unwrap().parse::<i64>().is_err() {
            *after = json!(after.as_str().unwrap().parse::<DateTime<Utc>>().unwrap().timestamp().to_string());
        }
    }

    let res = json_search(&client,&schema,&req).await.map(|val| json!(val))?;
    Ok(HttpResponse::Ok().json(res))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let cfg = Config::from_env("PG").unwrap();
    let pool = cfg.create_pool(NoTls).unwrap();

    let mut file = File::open("schema.yaml").unwrap();
    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    let schema: Schema = serde_yaml::from_str(&s).unwrap();

    let server = HttpServer::new(move || {
        let cors = Cors::default()
                    .allow_any_origin()
                    .allowed_methods(vec!["GET"]);

        App::new()
            .data(pool.clone())
            .data(schema.clone())
            .wrap(cors)
            .wrap(Logger::default())
            .service(search)
    })
    .bind("0.0.0.0:4445".to_string())?
    .run();

    server.await
}
