use serde::{Deserialize, Serialize};
use serde_json::Value as JSONValue;

use rocket::get;
use rocket::serde::json::Json as RocketJson;

use futures_util::{pin_mut, StreamExt};

use crab::chron::{self, v1};

use crate::*;
use compass::*;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChronGameUpdate {
    #[serde(skip_serializing)]
    timestamp: DateTime<Utc>,
    data: GameUpdate,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GameUpdate {
    #[serde(rename(serialize = "gameId"))]
    id: String,
    phase: Option<i64>,
    home_team: Option<String>,
    away_team: Option<String>,
    home_batter: Option<String>,
    away_batter: Option<String>,
    home_batter_mod: Option<String>,
    away_batter_mod: Option<String>,
    home_pitcher: Option<String>,
    away_pitcher: Option<String>,
    home_pitcher_mod: Option<String>,
    away_pitcher_mod: Option<String>,
    home_outs: Option<i64>,
    home_strikes: Option<i64>,
    home_balls: Option<i64>,
    home_bases: Option<i64>,
    away_outs: Option<i64>,
    away_strikes: Option<i64>,
    away_balls: Option<i64>,
    away_bases: Option<i64>,
    stadium_id: Option<String>,
    weather: Option<i64>,
    series_length: Option<i64>,
    series_index: Option<i64>,
    is_post_season: Option<bool>,
    is_title_match: Option<bool>,
    inning: Option<i64>,
    top_of_inning: Option<bool>,
    half_inning_score: Option<f64>,
    home_score: Option<f64>,
    away_score: Option<f64>,
    at_bat_balls: Option<i64>,
    at_bat_strikes: Option<i64>,
    half_inning_outs: Option<i64>,
    baserunner_count: Option<i64>,
    bases_occupied: Vec<Option<i64>>,
    base_runner_mods: Vec<String>,
    last_update: Option<String>,
    score_ledger: Option<String>,
    score_update: Option<String>,
    away_team_batter_count: Option<i64>,
    home_team_batter_count: Option<i64>,
    shame: Option<bool>,
    outcomes: Vec<String>,
    secret_baserunner: Option<String>,
    state: Option<JSONValue>,
    #[serde(skip_serializing)]
    play_count: i64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FeedEvent {
    id: String,
    player_tags: Option<Vec<String>>,
    team_tags: Option<Vec<String>>,
    game_tags: Option<Vec<String>>,
    metadata: Option<JSONValue>,
    created: DateTime<Utc>,
    day: i64,
    season: i64,
    phase: i64,
    tournament: i64,
    r#type: i64,
    category: i64,
    description: String,
    nuts: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Packet {
    play_count: i64,
    sub_play: i64,
    _packet_incomplete: bool,
    #[serde(flatten)]
    feed: FeedEvent,
    #[serde(flatten)]
    game_update: Option<GameUpdate>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Pallet {
    play_count: i64,
    sub_play: i64,
    #[serde(flatten)]
    feed: Vec<FeedEvent>,
    #[serde(flatten)]
    game_update: Option<ChronGameUpdate>,
}

#[get("/packets?<id>")]
pub async fn get_packets(
    db: CompassConn,
    id: String,
    schema: Schema,
) -> Result<RocketJson<Vec<Packet>>, CompassError> {
    let mut pallets: HashMap<i64, Pallet> = HashMap::new();
    let game = id.clone();
    for event in db
        .run(move |mut c| {
            json_search(
                &mut c,
                &schema,
                &(vec![
                    ("gameTags".to_owned(), game),
                    ("limit".to_owned(), "10000000".to_owned()),
                ]
                .into_iter()
                .collect::<HashMap<String, String>>()),
            )
        })
        .await?
    {
        let packet = pallets
            .entry(event["metadata"]["play"].as_i64().unwrap())
            .or_insert(Pallet {
                play_count: event["metadata"]["play"].as_i64().unwrap(),
                sub_play: event["metadata"]["subPlay"].as_i64().unwrap(),
                feed: vec![],
                game_update: None,
            });
        packet
            .feed
            .push(serde_json::from_value::<FeedEvent>(event)?);
    }

    let client = reqwest::Client::default();

    let req: v1::GameUpdatesRequest = v1::GameUpdatesRequestBuilder::default()
        .game(id)
        .count(1000usize)
        .build()
        .unwrap();
    let s = chron::v1::fetch::<ChronGameUpdate, v1::GameUpdatesRequest>(
        &client,
        "https://api.sibr.dev/chronicler/v1/games/updates",
        req,
    );

    pin_mut!(s);

    while let Some(val) = s.next().await {
        if let Some(count) = val.as_ref().ok().map(|v| v.data.play_count) {
            let packet = pallets.entry(count - 1).or_insert(Pallet {
                play_count: count - 1,
                sub_play: -1,
                feed: vec![],
                game_update: None,
            });

            let timestamp: DateTime<Utc> = val.as_ref().unwrap().timestamp;
            if timestamp
                > packet
                    .game_update
                    .as_ref()
                    .map(|v| v.timestamp)
                    .unwrap_or(Utc.timestamp(0, 0))
            {
                packet.game_update = val.ok();
            }
        }
    }

    let mut packets: Vec<Packet> = pallets
        .into_iter()
        .map(|(_, v)| {
            let mut packets = Vec::new();
            for ev in v.feed {
                packets.push(if let Some(g_update) = v.game_update.as_ref() {
                    Packet {
                        play_count: v.play_count,
                        sub_play: v.sub_play,
                        _packet_incomplete: false,
                        feed: ev,
                        game_update: Some(g_update.data.clone()),
                    }
                } else {
                    Packet {
                        play_count: v.play_count,
                        sub_play: v.sub_play,
                        _packet_incomplete: true,
                        feed: ev,
                        game_update: None,
                    }
                });
            }
            packets
        })
        .flatten()
        .collect();
    packets.sort_by_key(|v| v.play_count);

    Ok(RocketJson(packets))
}
