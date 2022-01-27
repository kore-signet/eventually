use crate::*;
use serde_json::json;
use serde_json::Value as JSONValue;

use lazy_static::lazy_static;
use rocket::get;

static TIME_MAP_BIN: &[u8] = include_bytes!("../../time_map.bin");

lazy_static! {
    static ref TIME_MAP: TimeMap = bincode::deserialize(TIME_MAP_BIN).unwrap();
}

async fn get_time(
    db: CompassConn,
    schema: Schema,
    sim: String,
    season: i32,
    day: Option<i32>,
) -> Result<JSONValue, EventuallyError> {
    if sim == "thisidisstaticyo" && season <= 10 && season > -1 {
        let season = TIME_MAP
            .get(season as usize)
            .ok_or(EventuallyError::TimeMapEntryNotFound)?;
        if let Some(d) = day {
            let day = season
                .days
                .get(d as usize)
                .ok_or(EventuallyError::TimeMapEntryNotFound)?;
            return Ok(json!({
                "start": day.0,
                "end": day.1
            }));
        } else {
            return Ok(json!({
                "start": season.lower_bound,
                "end": season.higher_bound
            }));
        }
    }

    let mut query = HashMap::from([
        ("sim".to_string(), sim),
        ("season".to_string(), season.to_string()),
        ("limit".to_string(), "1".to_string()),
    ]);
    if let Some(d) = day {
        query.insert("day".to_string(), d.to_string());
    }

    let first_q = query.clone();
    let first_s = schema.clone();
    let last_time = db
        .run(move |c| json_search(c, &first_s, &first_q, None))
        .await?
        .pop()
        .and_then(|mut v| v.as_object_mut().and_then(|a| a.remove("created")));

    query.insert("sortorder".to_string(), "asc".to_string());
    let first_time = db
        .run(move |c| json_search(c, &schema, &query, None))
        .await?
        .pop()
        .and_then(|mut v| v.as_object_mut().and_then(|a| a.remove("created")));

    Ok(json!({
        "start": first_time,
        "end": last_time
    }))
}

#[get("/time/<sim>/<season>")]
pub async fn season_time_map(
    db: CompassConn,
    schema: Schema,
    sim: String,
    season: i32,
) -> Result<JSONValue, EventuallyError> {
    get_time(db, schema, sim, season, None).await
}

#[get("/time/<sim>/<season>/<day>")]
pub async fn season_day_time_map(
    db: CompassConn,
    schema: Schema,
    sim: String,
    season: i32,
    day: i32,
) -> Result<JSONValue, EventuallyError> {
    get_time(db, schema, sim, season, Some(day)).await
}
