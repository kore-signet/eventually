use crate::*;
use serde_json::json;
use serde_json::Value as JSONValue;
use uuid::Uuid;

use rocket::get;
use rocket::serde::json::Json as RocketJson;

#[get("/count")]
pub async fn count(
    raw_req: Query,
    db: CompassConn,
    schema: Schema,
) -> Result<RocketJson<JSONValue>, CompassError> {
    let mut req = raw_req.0;

    if let Some(before) = req.get_mut("before") {
        if before.parse::<i64>().is_err() {
            *before = DateTime::parse_from_rfc3339(before)
                .unwrap()
                .timestamp_millis()
                .to_string();
        }
    }

    if let Some(after) = req.get_mut("after") {
        if after.parse::<i64>().is_err() {
            *after = DateTime::parse_from_rfc3339(after)
                .unwrap()
                .timestamp_millis()
                .to_string();
        }
    }

    db.run(move |c| json_count(c, &schema, &req).map(|v| RocketJson(json!({ "count": v }))))
        .await
}

#[get("/events")]
pub async fn search(
    raw_req: Query,
    db: CompassConn,
    schema: Schema,
) -> Result<RocketJson<Vec<JSONValue>>, CompassError> {
    let mut req = raw_req.0;

    if let Some(before) = req.get_mut("before") {
        if before.parse::<i64>().is_err() {
            *before = DateTime::parse_from_rfc3339(before)
                .unwrap()
                .timestamp_millis()
                .to_string();
        }
    }

    if let Some(after) = req.get_mut("after") {
        if after.parse::<i64>().is_err() {
            *after = DateTime::parse_from_rfc3339(after)
                .unwrap()
                .timestamp_millis()
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
    let expand_siblings = req
        .remove("expand_siblings")
        .and_then(|c| c.parse::<bool>().ok())
        .unwrap_or(false);

    let raw_query = req.remove("raw_query");

    db.run(move |c| match json_search(c, &schema, &req, raw_query) {
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
                            c,
                            &schema,
                            &children
                                .iter()
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
                        event["metadata"]["parent"] =
                            json!(get_by_ids(c, &schema, &vec![parent])?.first());
                    }
                }

                if expand_siblings {
                    if let Some(children) = event
                        .get("metadata")
                        .and_then(|i| i.get("siblingIds"))
                        .and_then(|i| i.as_array())
                    {
                        event["metadata"]["_eventually_siblingEvents"] = json!(get_by_ids(
                            c,
                            &schema,
                            &children
                                .iter()
                                .filter_map(|i| i.as_str())
                                .filter_map(|i| Uuid::parse_str(i).ok())
                                .collect()
                        )?);
                    }
                }

                Ok(event)
            })
            .collect::<Result<Vec<JSONValue>, CompassError>>()
            .map(RocketJson),
        Err(e) => Err(e),
    })
    .await
}

#[get("/one_of_each_type")]
pub async fn distinct_events(db: CompassConn) -> Result<JSONValue, CompassError> {
    db.run(move |c| {
        let mut evs: Vec<JSONValue> = Vec::new();
        for event_type in c.query("SELECT DISTINCT (object->'type')::integer FROM documents_millis",&[]).map_err(CompassError::PGError)? {
            let etype: i32 = event_type.get(0);
            let row = c.query_opt(format!("SELECT object FROM documents_millis WHERE object @@ '(($.metadata.redacted == false) || !exists($.metadata.redacted)) && $.type == {}' LIMIT 1",etype).as_str(),&[])?;

            if let Some(r) = row {
                let mut ev: JSONValue = r.get(0);
                if let Some(timest) = ev["created"].as_i64() {
                    ev["created"] = json!(Utc.timestamp_millis(timest).to_rfc3339_opts(chrono::SecondsFormat::Millis,true));
                    evs.push(ev);
                }
            }
        }
        Ok(json!(evs))
    })
    .await
}

#[get("/versions?<id>")]
pub async fn get_versions(db: CompassConn, id: String) -> Result<JSONValue, CompassError> {
    db.run(move |c| {
        let id = Uuid::parse_str(id.as_str()).unwrap();
        let results = c
            .query("SELECT object FROM versions WHERE doc_id = $1", &[&id])
            .map_err(CompassError::PGError)?;
        Ok(json!(results
            .into_iter()
            .map(|row| {
                let mut ev: JSONValue = row.get(0);
                if let Some(timest) = ev["created"].as_i64() {
                    ev["created"] = json!(Utc
                        .timestamp_millis(timest)
                        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
                }
                ev
            })
            .collect::<Vec<JSONValue>>()))
    })
    .await
}

async fn get_time(
    db: CompassConn,
    schema: Schema,
    sim: String,
    season: i32,
    day: Option<i32>,
) -> Result<JSONValue, CompassError> {
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
) -> Result<JSONValue, CompassError> {
    get_time(db, schema, sim, season, None).await
}

#[get("/time/<sim>/<season>/<day>")]
pub async fn season_day_time_map(
    db: CompassConn,
    schema: Schema,
    sim: String,
    season: i32,
    day: i32,
) -> Result<JSONValue, CompassError> {
    get_time(db, schema, sim, season, Some(day)).await
}
