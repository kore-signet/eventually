use compass::*;
use rocket::{launch, routes};
use rustventually::*;
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
            routes![
                eventually::search,
                eventually::distinct_events,
                eventually::get_versions,
                cors_preflight
            ],
        )
        .mount("/sachet", routes![sachet::get_packets])
}
