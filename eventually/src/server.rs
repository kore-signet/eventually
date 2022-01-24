use compass::*;
use rocket::{launch, routes};
use rustventually::*;
use sled::Db as SledDB;
use std::fs::File;
use std::io::Read;

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

#[derive(serde::Deserialize, Debug)]
#[serde(default)]
struct EventuallyConfig {
    cache_zstd: bool,
    cache_temporary: bool,
    cache_path: Option<String>,
    cache_mem_size: Option<u64>,
}

impl Default for EventuallyConfig {
    fn default() -> Self {
        EventuallyConfig {
            cache_zstd: true,
            cache_temporary: true,
            cache_path: None,
            cache_mem_size: None,
        }
    }
}

impl EventuallyConfig {
    fn into_db(&self) -> sled::Result<SledDB> {
        let mut config = sled::Config::default()
            .use_compression(self.cache_zstd)
            .temporary(self.cache_temporary);
        if let Some(ref p) = self.cache_path {
            config = config.path(p);
        }

        if let Some(size) = self.cache_mem_size {
            config = config.cache_capacity(size);
        }

        config.open()
    }
}

#[launch]
fn rocket() -> _ {
    let rocket = rocket::build();
    let figment = rocket.figment();

    let config: EventuallyConfig = figment.extract().unwrap_or_default();
    let db = config.into_db().expect("couldn't open sled cache");

    let mut file = File::open("schema.yaml").unwrap();
    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    let schema: Schema = serde_yaml::from_str(&s).unwrap();

    rocket
        .manage(schema)
        .manage(db)
        .attach(CompassConn::fairing())
        .attach(CORS)
        .mount(
            "/",
            routes![
                eventually::search,
                eventually::count,
                eventually::distinct_events,
                eventually::get_versions,
                eventually::season_day_time_map,
                eventually::season_time_map,
                cors_preflight
            ],
        )
        .mount("/sachet", routes![sachet::get_packets])
}
