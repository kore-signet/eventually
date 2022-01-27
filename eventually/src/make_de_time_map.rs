use chrono::{DateTime, Utc};
use rustventually::{TimeMap, TimeMapSeason};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct V1Games {
    data: Vec<Game>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Game {
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    data: GameData,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct GameData {
    season: i64,
    day: i64,
}

struct BuildTimeMapSeason {
    lower_bound: DateTime<Utc>,
    higher_bound: DateTime<Utc>,
    days: BTreeMap<i64, (DateTime<Utc>, DateTime<Utc>)>,
}

fn main() {
    let games: V1Games = reqwest::blocking::get(
        "https://api.sibr.dev/chronicler/v1/games?before=2020-10-30T21:22:59.000Z",
    )
    .unwrap()
    .json()
    .unwrap();
    let mut entries: BTreeMap<i64, BuildTimeMapSeason> = BTreeMap::new();
    for game in games.data {
        if !entries.contains_key(&game.data.season) {
            entries.insert(
                game.data.season,
                BuildTimeMapSeason {
                    lower_bound: game.start_time,
                    higher_bound: game.end_time,
                    days: BTreeMap::from([(game.data.day, (game.start_time, game.end_time))]),
                },
            );
        } else {
            let mut season = entries.get_mut(&game.data.season).unwrap();

            if game.start_time < season.lower_bound {
                season.lower_bound = game.start_time;
            }

            if game.end_time > season.higher_bound {
                season.higher_bound = game.end_time;
            }

            let mut day = season
                .days
                .entry(game.data.day)
                .or_insert((game.start_time, game.end_time));
            if game.start_time < day.0 {
                day.0 = game.start_time;
            }

            if game.end_time > day.1 {
                day.1 = game.end_time;
            }
        }
    }

    let mut time_map: TimeMap = Vec::new();

    for entry in entries.into_values() {
        time_map.push(TimeMapSeason {
            lower_bound: entry.lower_bound,
            higher_bound: entry.higher_bound,
            days: entry.days.into_values().collect(),
        });
    }

    std::fs::write("time_map.bin", bincode::serialize(&time_map).unwrap()).unwrap();
}
