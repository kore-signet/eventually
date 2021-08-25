use serde::Serialize;
use serde_json::Value as JSONValue;

use rocket::get;
use rocket::serde::json::Json as RocketJson;

use futures_util::{pin_mut, StreamExt};

use crab::chron::{self, v1};

use crate::*;
use compass::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Packet {
    play_count: i64,
    feed: Vec<JSONValue>,
    game_updates: Vec<JSONValue>,
}

#[get("/packets?<id>")]
pub async fn get_packets(
    db: CompassConn,
    id: String,
    schema: Schema,
) -> Result<RocketJson<Vec<Packet>>, CompassError> {
    let mut packets: HashMap<i64, Packet> = HashMap::new();
    let game = id.clone();
    for event in db
        .run(move |mut c| {
            json_search(
                &mut c,
                &schema,
                &(vec![("gameTags".to_owned(), game), ("limit".to_owned(), "10000000".to_owned())]
                    .into_iter()
                    .collect::<HashMap<String, String>>()),
            )
        })
        .await?
    {
        let packet = packets
            .entry(event["metadata"]["play"].as_i64().unwrap())
            .or_insert(Packet {
                play_count: event["metadata"]["play"].as_i64().unwrap(),
                feed: vec![],
                game_updates: vec![],
            });
        packet.feed.push(event);
    }

    let client = reqwest::Client::default();

    let req: v1::GameUpdatesRequest = v1::GameUpdatesRequestBuilder::default()
        .game(id)
        .count(1000usize)
        .build()
        .unwrap();
    let s = chron::v1::fetch::<JSONValue, v1::GameUpdatesRequest>(
        &client,
        "https://api.sibr.dev/chronicler/v1/games/updates",
        req,
    );

    pin_mut!(s);

    while let Some(val) = s.next().await {
        if let Some(count) = val
            .as_ref()
            .unwrap()
            .get("data")
            .and_then(|v| v.get("playCount"))
            .and_then(|v| v.as_i64())
        {
            let packet = packets.entry(count - 1).or_insert(Packet {
                play_count: count - 1,
                feed: vec![],
                game_updates: vec![],
            });
            packet.game_updates.push(val.unwrap());
        }
    }

    let mut packets: Vec<Packet> = packets.into_iter().map(|(_,v)| v).collect();
    packets.sort_by_key(|v| v.play_count);

    Ok(RocketJson(packets))
}
