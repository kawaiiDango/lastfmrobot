use std::{error::Error, time::Duration};

use http_cache_reqwest::{Cache, CacheMode, CacheOptions, HttpCache, MokaManager};
use once_cell::sync::Lazy;
use reqwest::{header::HeaderValue, Request, Response, StatusCode, Url};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware, Middleware, Next};
use serde_json::Value;
use strum_macros::{Display, EnumString, IntoStaticStr};
use task_local_extensions::Extensions;

use crate::{config, consts};

#[derive(Debug)]
pub struct Track {
    pub name: String,
    pub album: Option<String>,
    pub artist: String,
    pub album_art_url: Option<String>,
    pub date: Option<u64>,
    pub duration: u64,
    pub listeners: u64,
    pub playcount: u64,
    pub user_playcount: u64,
    pub user_loved: bool,
    pub now_playing: bool,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct Album {
    pub name: String,
    pub artist: String,
    pub album_art_url: Option<String>,
    pub playcount: u64,
    pub listeners: u64,
    pub user_playcount: u64,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct Artist {
    pub name: String,
    pub playcount: u64,
    pub listeners: u64,
    pub user_playcount: u64,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct ScrobbleUser {
    pub username: String,
    pub playcount: u64,
    pub artist_count: u64,
    pub album_count: u64,
    pub track_count: u64,
    pub profile_pic_url: Option<String>,
    pub registered_date: Option<u64>,
}

#[derive(Debug, PartialEq, EnumString, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ApiType {
    Lastfm,
    Librefm,
    Listenbrainz,
}

#[derive(Debug, PartialEq, EnumString, Display, IntoStaticStr)]
pub enum TimePeriod {
    #[strum(serialize = "1 week")]
    OneWeek,
    #[strum(serialize = "1 month")]
    OneMonth,
    #[strum(serialize = "3 months")]
    ThreeMonths,
    #[strum(serialize = "6 months")]
    SixMonths,
    #[strum(serialize = "1 year")]
    OneYear,
    #[strum(serialize = "All time")]
    AllTime,
}

#[derive(Debug, PartialEq, EnumString, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EntryType {
    Artist,
    Album,
    Track,
}

struct ForceCacheMiddleware {}

#[async_trait::async_trait]
impl Middleware for ForceCacheMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let no_cache = req
            .headers()
            .get("cache-control")
            .map(|h| h.to_str().unwrap_or_default())
            .unwrap_or_default()
            .contains("no-cache");

        if !no_cache {
            req.headers_mut().append(
                "cache-control",
                HeaderValue::from_str("max-stale=300").unwrap(),
            );
        }

        let mut resp = next.run(req, extensions).await?;

        if !no_cache {
            resp.headers_mut().insert(
                "cache-control",
                HeaderValue::from_str("max-age=300, public, immutable").unwrap(),
            );
        }
        Ok(resp)
    }
}

struct Response200Middleware {}
#[async_trait::async_trait]
impl Middleware for Response200Middleware {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        let resp = next.run(req, extensions).await?;
        if resp.status().is_success() {
            Ok(resp)
        } else {
            let display_msg = match resp.status() {
                StatusCode::NOT_FOUND => consts::USER_NOT_FOUND,
                StatusCode::FORBIDDEN => consts::PRIVATE_PROFILE,
                _ => resp.status().canonical_reason().unwrap_or(consts::ERR_MSG),
            };

            return Err(reqwest_middleware::Error::Middleware(anyhow::anyhow!(
                display_msg
            )));
        }
    }
}

pub static CLIENT: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(
        reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(25))
            .https_only(true)
            .build()
            .unwrap(),
    )
    .with(Response200Middleware {})
    .with(ForceCacheMiddleware {})
    .with(Cache(HttpCache {
        mode: CacheMode::Default,
        manager: MokaManager::new(
            moka::future::Cache::builder()
                .max_capacity(100)
                .time_to_live(Duration::from_secs(300))
                .build(),
        ),
        options: http_cache_reqwest::HttpCacheOptions {
            cache_options: CacheOptions {
                shared: false,
                cache_heuristic: 0.1, // 10% matches IE
                immutable_min_time_to_live: Duration::from_secs(300),
                ignore_cargo_cult: true,
            }
            .into(),
            cache_key: None,
        },
    }))
    .build()
});

pub static CLIENT_NOCACHE: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(
        reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(25))
            .https_only(true)
            .build()
            .unwrap(),
    )
    .with(Response200Middleware {})
    .build()
});

fn get_base_url(api_type: &ApiType) -> &'static str {
    match api_type {
        ApiType::Lastfm => "https://ws.audioscrobbler.com/2.0/",
        ApiType::Librefm => "https://libre.fm/2.0/",
        ApiType::Listenbrainz => "https://api.listenbrainz.org/1/",
    }
}

fn get_biggest_lastfm_image(json_value: &serde_json::Value) -> Option<String> {
    let url = json_value["image"]
        .as_array()
        .and_then(|images| {
            images
                .iter()
                .last()
                .and_then(|image| image["#text"].as_str())
                .map(|text| text.to_string())
        })
        .unwrap_or_default();

    if url.is_empty() || url.contains("2a96cbd8b46e442fc41c2b86b821562f") {
        None
    } else {
        Some(url)
    }
}

pub async fn fetch_lastfm_track(
    username: String,
    artist: String,
    track: String,
) -> Result<Track, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(&ApiType::Lastfm);
    let url = Url::parse_with_params(
        base_url,
        &[
            ("method", "track.getInfo"),
            ("track", track.as_str()),
            ("artist", artist.as_str()),
            ("user", username.as_str()),
            ("api_key", config::LASTFM_API_KEY),
            ("format", "json"),
        ],
    );

    let response = CLIENT.get(url?).send().await?;

    let json = response.json::<serde_json::Value>().await?;
    let track_json = json["track"].as_object();
    if track_json.is_none() {
        return Err(Box::from("Track not found."));
    }
    let track_json = track_json.unwrap();
    let name = track_json["name"].as_str().unwrap_or_default().to_string();
    let album_obj = track_json.get("album");
    let album = if let Some(album_obj) = album_obj {
        let x = album_obj["title"].as_str().unwrap_or_default();
        (!x.is_empty()).then_some(x.to_string())
    } else {
        None
    };
    let artist = track_json["artist"]["name"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let listeners = track_json["listeners"]
        .as_str()
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or_default();
    let playcount = track_json["playcount"]
        .as_str()
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or_default();
    let duration = track_json["duration"]
        .as_str()
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or_default();
    let user_playcount_obj = track_json.get("userplaycount");
    let user_playcount = if let Some(user_playcount_obj) = user_playcount_obj {
        user_playcount_obj
            .as_str()
            .unwrap_or_default()
            .parse::<u64>()
            .unwrap_or_default()
    } else {
        0
    };

    let user_loved = track_json
        .get("userloved")
        .map(|x| x.as_str().unwrap_or_default() == "1")
        .unwrap_or_default();
    let tags = track_json["toptags"].get("tag").map(|x| {
        x.as_array()
            .into_iter()
            .flatten()
            .map(|x| x["name"].as_str().unwrap_or_default().to_string())
            .filter(|x| !x.is_empty())
            .collect::<Vec<_>>()
    });

    Ok(Track {
        name,
        album,
        artist,
        listeners,
        playcount,
        user_playcount,
        user_loved,
        duration,
        album_art_url: None,
        date: None,
        now_playing: false,
        tags,
    })
}

pub async fn fetch_lastfm_album(
    username: &str,
    artist: &str,
    album: &str,
) -> Result<Album, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(&ApiType::Lastfm);
    let url = Url::parse_with_params(
        base_url,
        &[
            ("method", "album.getInfo"),
            ("album", album),
            ("artist", artist),
            ("user", username),
            ("api_key", config::LASTFM_API_KEY),
            ("format", "json"),
        ],
    );
    let response = CLIENT.get(url?).send().await?;

    let json = response.json::<serde_json::Value>().await?;
    let album_json = json["album"].as_object();
    if album_json.is_none() {
        return Err(Box::from("Album not found."));
    }
    let album_json = album_json.unwrap();
    let name = album_json["name"].as_str().unwrap_or_default().to_string();
    let artist = album_json["artist"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let listeners = album_json["listeners"]
        .as_str()
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or_default();
    let playcount = album_json["playcount"]
        .as_str()
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or_default();
    let user_playcount_obj = album_json.get("userplaycount");
    let user_playcount = if let Some(user_playcount_obj) = user_playcount_obj {
        user_playcount_obj
            .as_str()
            .unwrap_or_default()
            .parse::<u64>()
            .unwrap_or_default()
    } else {
        0
    };
    let tags = album_json["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|x| x["name"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();

    Ok(Album {
        name,
        artist,
        listeners,
        playcount,
        user_playcount,
        album_art_url: None,
        tags: Some(tags),
    })
}

pub async fn fetch_lastfm_artist(
    username: String,
    artist: String,
) -> Result<Artist, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(&ApiType::Lastfm);
    let url = Url::parse_with_params(
        base_url,
        &[
            ("method", "artist.getInfo"),
            ("artist", artist.as_str()),
            ("user", username.as_str()),
            ("api_key", config::LASTFM_API_KEY),
            ("format", "json"),
        ],
    );
    let response = CLIENT.get(url?).send().await?;

    let json = response.json::<serde_json::Value>().await?;
    let artist_json = json["artist"].as_object();
    if artist_json.is_none() {
        return Err(Box::from("Artist not found."));
    }
    let artist_json = artist_json.unwrap();
    let name = artist_json["name"].as_str().unwrap_or_default().to_string();
    let listeners = artist_json["stats"]["listeners"]
        .as_str()
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or_default();
    let playcount = artist_json["stats"]["playcount"]
        .as_str()
        .unwrap_or_default()
        .parse::<u64>()
        .unwrap_or_default();
    let user_playcount_obj = artist_json["stats"].get("userplaycount");
    let user_playcount = if let Some(user_playcount_obj) = user_playcount_obj {
        user_playcount_obj
            .as_str()
            .unwrap_or_default()
            .parse::<u64>()
            .unwrap_or_default()
    } else {
        0
    };
    let tags = artist_json["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|x| x["name"].as_str().unwrap_or_default().to_string())
        .collect::<Vec<_>>();

    Ok(Artist {
        name,
        listeners,
        playcount,
        user_playcount,
        tags: tags.into(),
    })
}

pub fn parse_listenbrainz_tracks(
    json_arr: &Value,
) -> Result<Vec<Track>, Box<dyn Error + Send + Sync>> {
    parse_listenbrainz_tracks_np(json_arr, false)
}
pub fn parse_listenbrainz_tracks_np(
    json_arr: &Value,
    now_playing: bool,
) -> Result<Vec<Track>, Box<dyn Error + Send + Sync>> {
    let tracks = json_arr
        .as_array()
        .into_iter()
        .flatten()
        .map(|track_json| {
            let track_metadata = if let Some(m) = track_json.get("track_metadata") {
                m
            } else {
                track_json
            };

            let artist = track_metadata["artist_name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let album = track_metadata["release_name"]
                .as_str()
                .map(|s| s.to_string());
            let name = track_metadata["track_name"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            let album_art_url = track_metadata["release_mbid"]
                .as_str()
                .map(|mbid| format!("https://coverartarchive.org/release/{mbid}/front-250"));
            let user_playcount = track_metadata["listen_count"].as_u64().unwrap_or_default();
            let date = track_json["listened_at"].as_u64();

            Track {
                name,
                album,
                artist,
                album_art_url,
                date,
                user_loved: false,
                duration: 0,
                listeners: 0,
                playcount: 0,
                user_playcount,
                now_playing,
                tags: None,
            }
        })
        .collect::<Vec<_>>();

    Ok(tracks)
}

pub fn parse_lastfm_tracks(json_arr: &Value) -> Result<Vec<Track>, Box<dyn Error + Send + Sync>> {
    let tracks = json_arr
        .as_array()
        .into_iter()
        .flatten()
        .map(|track_json| {
            let artist_obj = &track_json["artist"];
            let artist = if let Some(artist_name) = artist_obj.get("#text") {
                artist_name.as_str().unwrap_or_default()
            } else if let Some(artist_name) = artist_obj.get("name") {
                artist_name.as_str().unwrap_or_default()
            } else {
                ""
            };

            let album_obj = track_json["album"].as_object();

            let album = if let Some(album_obj) = album_obj {
                let x = album_obj["#text"].as_str().unwrap_or_default();
                (!x.is_empty()).then_some(x.to_string())
            } else {
                None
            };

            let name = track_json["name"].as_str().unwrap_or_default().to_string();
            let album_art_url = get_biggest_lastfm_image(track_json);
            let date = track_json["date"]["uts"]
                .as_str()
                .unwrap_or_default()
                .parse::<u64>()
                .ok();
            let user_loved = track_json["loved"].as_str().unwrap_or_default() == "1";
            let now_playing = track_json["@attr"]
                .get("nowplaying")
                .map(|x| x.as_str().unwrap_or_default())
                .unwrap_or_default()
                == "true";

            Track {
                name,
                album,
                artist: artist.into(),
                album_art_url,
                date,
                user_loved,
                duration: 0,
                listeners: 0,
                playcount: 0,
                user_playcount: 0,
                now_playing,
                tags: None,
            }
        })
        .collect::<Vec<_>>();

    Ok(tracks)
}

// Get recent tracks for a given user
pub async fn fetch_recent_tracks(
    username: &str,
    api_type: &ApiType,
    prefer_cached: bool,
    actual_limit: usize,
) -> Result<Vec<Track>, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(api_type);
    let cache_control = if prefer_cached {
        "max-stale=300"
    } else {
        "no-cache, must-revalidate"
    };

    match api_type {
        ApiType::Listenbrainz => {
            let url = format!("{}user/{}/playing-now", base_url, username);
            let response = CLIENT
                .get(&url)
                .header("cache-control", cache_control)
                .send()
                .await?;

            let json = response.json::<serde_json::Value>().await?;

            let mut all_tracks = parse_listenbrainz_tracks_np(&json["payload"]["listens"], true)?;

            if !all_tracks.is_empty() && actual_limit == 1 {
                return Ok(all_tracks);
            }

            let url = format!("{}user/{}/listens?count=3", base_url, username);
            let response = CLIENT
                .get(&url)
                .header("cache-control", cache_control)
                .send()
                .await?;
            let json = response.json::<serde_json::Value>().await?;

            let tracks = parse_listenbrainz_tracks(&json["payload"]["listens"])?;

            all_tracks.extend(tracks);
            Ok(all_tracks)
        }

        ApiType::Librefm | ApiType::Lastfm => {
            let url = Url::parse_with_params(
                base_url,
                &[
                    ("method", "user.getrecenttracks"),
                    ("user", username),
                    ("extended", "1"),
                    ("limit", "3"),
                    ("api_key", config::LASTFM_API_KEY),
                    ("format", "json"),
                ],
            )?;

            let response = CLIENT
                .get(url)
                .header("cache-control", cache_control)
                .send()
                .await?;

            let json = response.json::<serde_json::Value>().await?;
            let err = json["error"]
                .as_object()
                .map(|x| x["#text"].as_str().unwrap_or_default());
            if let Some(err) = err {
                if !err.is_empty() {
                    return Err(Box::from(err));
                }
            }

            let tracks = parse_lastfm_tracks(&json["recenttracks"]["track"])?;

            Ok(tracks)
        }
    }
}

// Get loved tracks for a given user
pub async fn fetch_loved_tracks(
    username: &str,
    api_type: &ApiType,
) -> Result<Vec<Track>, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(api_type);

    match api_type {
        ApiType::Listenbrainz => {
            let url = format!(
                "{}user/{}/get-feedback?metadata=true&count=5",
                base_url, username
            );

            let response = CLIENT.get(&url).send().await?;
            let json = response.json::<serde_json::Value>().await?;
            let tracks = parse_listenbrainz_tracks(&json["feedback"])?;

            Ok(tracks)
        }

        ApiType::Librefm | ApiType::Lastfm => {
            let url = Url::parse_with_params(
                base_url,
                &[
                    ("method", "user.getlovedtracks"),
                    ("user", username),
                    ("limit", "5"),
                    ("api_key", config::LASTFM_API_KEY),
                    ("format", "json"),
                ],
            )?;

            let response = CLIENT.get(url).send().await?;
            let json = response.json::<serde_json::Value>().await?;
            let tracks = parse_lastfm_tracks(&json["lovedtracks"]["track"])?;

            Ok(tracks)
        }
    }
}

fn time_period_to_api_string<'a>(duration: &'a TimePeriod, api_type: &'a ApiType) -> &'a str {
    match api_type {
        ApiType::Lastfm | ApiType::Librefm => match duration {
            TimePeriod::OneWeek => "7day",
            TimePeriod::OneMonth => "1month",
            TimePeriod::ThreeMonths => "3month",
            TimePeriod::SixMonths => "6month",
            TimePeriod::OneYear => "12month",
            TimePeriod::AllTime => "overall",
        },
        ApiType::Listenbrainz => match duration {
            TimePeriod::OneWeek => "week",
            TimePeriod::OneMonth => "month",
            TimePeriod::ThreeMonths => "quarter",
            TimePeriod::SixMonths => "half_yearly",
            TimePeriod::OneYear => "year",
            TimePeriod::AllTime => "all_time",
        },
    }
}

// Get albums for a given user
pub async fn fetch_albums(
    username: &str,
    duration: &TimePeriod,
    api_type: &ApiType,
    limit: Option<usize>,
) -> Result<Vec<Album>, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(api_type);
    let duration_str = time_period_to_api_string(duration, api_type);

    match api_type {
        ApiType::Listenbrainz => {
            let url = format!(
                "{}stats/user/{}/releases?range={}&count={}",
                base_url,
                username,
                duration_str,
                limit.unwrap_or(100)
            );
            let response = CLIENT.get(&url).send().await?;

            let json = response.json::<serde_json::Value>().await?;

            let albums = json["payload"]["releases"]
                .as_array()
                .ok_or("Invalid JSON format: 'payload.releases' is not an array")
                .into_iter()
                .flatten()
                .map(|album_json| {
                    let artist = album_json["artist_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let name = album_json["release_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let album_art_url = album_json["release_mbid"].as_str().map(|mbid| {
                        format!("https://coverartarchive.org/release/{mbid}/front-500")
                    });
                    let user_playcount = album_json["listen_count"].as_u64().unwrap_or_default();

                    Album {
                        name,
                        artist,
                        album_art_url,
                        listeners: 0,
                        playcount: 0,
                        user_playcount,
                        tags: None,
                    }
                })
                .collect::<Vec<_>>();

            Ok(albums)
        }

        ApiType::Librefm | ApiType::Lastfm => {
            let url = Url::parse_with_params(
                base_url,
                &[
                    ("method", "user.gettopalbums"),
                    ("period", duration_str),
                    ("user", username),
                    ("limit", &limit.unwrap_or(200).to_string()),
                    ("api_key", config::LASTFM_API_KEY),
                    ("format", "json"),
                ],
            )?;
            let response = CLIENT.get(url).send().await?;
            let json = response.json::<serde_json::Value>().await?;

            let albums = json["topalbums"]["album"]
                .as_array()
                .ok_or("Invalid JSON format: 'topalbums.album' is not an array")
                .into_iter()
                .flatten()
                .map(|album_json| {
                    let artist = album_json["artist"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let name = album_json["name"].as_str().unwrap_or_default().to_string();
                    let album_art_url = get_biggest_lastfm_image(album_json);
                    let user_playcount = album_json["playcount"]
                        .as_str()
                        .unwrap_or_default()
                        .parse::<u64>()
                        .unwrap_or_default();

                    Album {
                        name,
                        artist,
                        album_art_url,
                        listeners: 0,
                        playcount: 0,
                        user_playcount,
                        tags: None,
                    }
                })
                .collect::<Vec<_>>();

            Ok(albums)
        }
    }
}

// Get artists for a given user
pub async fn fetch_artists(
    username: &str,
    duration: &TimePeriod,
    api_type: &ApiType,
    limit: Option<usize>,
) -> Result<Vec<Artist>, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(api_type);
    let duration_str = time_period_to_api_string(duration, api_type);

    match api_type {
        ApiType::Listenbrainz => {
            let url = format!(
                "{}stats/user/{}/artists?range={}&count={}",
                base_url,
                username,
                duration_str,
                limit.unwrap_or(100)
            );
            let response = CLIENT.get(&url).send().await?;

            let json = response.json::<serde_json::Value>().await?;

            let artists = json["payload"]["artists"]
                .as_array()
                .ok_or("Invalid JSON format: 'payload.artists' is not an array")
                .into_iter()
                .flatten()
                .map(|artists_json| {
                    let name = artists_json["artist_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    let user_playcount = artists_json["listen_count"].as_u64().unwrap_or_default();

                    Artist {
                        name,
                        listeners: 0,
                        playcount: 0,
                        user_playcount,
                        tags: None,
                    }
                })
                .collect::<Vec<_>>();
            Ok(artists)
        }

        ApiType::Librefm | ApiType::Lastfm => {
            let url = Url::parse_with_params(
                base_url,
                &[
                    ("method", "user.gettopartists"),
                    ("period", duration_str),
                    ("user", username),
                    ("limit", &limit.unwrap_or(200).to_string()),
                    ("api_key", config::LASTFM_API_KEY),
                    ("format", "json"),
                ],
            )?;
            let response = CLIENT.get(url).send().await?;
            let json = response.json::<serde_json::Value>().await?;

            let artists = json["topartists"]["artist"]
                .as_array()
                .ok_or("Invalid JSON format: 'topartists.artist' is not an array")
                .into_iter()
                .flatten()
                .map(|artist_json| {
                    let name = artist_json["name"].as_str().unwrap_or_default().to_string();
                    let user_playcount = artist_json["playcount"]
                        .as_str()
                        .unwrap_or_default()
                        .parse::<u64>()
                        .unwrap_or_default();

                    Artist {
                        name,
                        listeners: 0,
                        playcount: 0,
                        user_playcount,
                        tags: None,
                    }
                })
                .collect::<Vec<_>>();

            Ok(artists)
        }
    }
}

// Get tracks for a given user
pub async fn fetch_tracks(
    username: &str,
    duration: &TimePeriod,
    api_type: &ApiType,
    limit: Option<usize>,
) -> Result<Vec<Track>, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(api_type);
    let duration_str = time_period_to_api_string(duration, api_type);

    match api_type {
        ApiType::Listenbrainz => {
            let url = format!(
                "{}stats/user/{}/recordings?range={}&count={}",
                base_url,
                username,
                duration_str,
                limit.unwrap_or(100)
            );
            let response = CLIENT.get(&url).send().await?;

            let json = response.json::<serde_json::Value>().await?;

            let tracks = parse_listenbrainz_tracks(&json["payload"]["recordings"])?;
            Ok(tracks)
        }

        ApiType::Librefm | ApiType::Lastfm => {
            let url = Url::parse_with_params(
                base_url,
                &[
                    ("method", "user.gettoptracks"),
                    ("period", duration_str),
                    ("user", username),
                    ("limit", &limit.unwrap_or(200).to_string()),
                    ("api_key", config::LASTFM_API_KEY),
                    ("format", "json"),
                ],
            )?;
            let response = CLIENT.get(url).send().await?;
            let json = response.json::<serde_json::Value>().await?;

            let tracks = json["toptracks"]["track"]
                .as_array()
                .ok_or("Invalid JSON format: 'toptracks.track' is not an array")
                .into_iter()
                .flatten()
                .map(|track_json| {
                    let name = track_json["name"].as_str().unwrap_or_default().to_string();
                    let user_playcount = track_json["playcount"]
                        .as_str()
                        .unwrap_or_default()
                        .parse::<u64>()
                        .unwrap_or_default();
                    let artist = track_json["artist"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string();

                    Track {
                        name,
                        album: None,
                        artist,
                        album_art_url: None,
                        date: None,
                        duration: 0,
                        listeners: 0,
                        playcount: 0,
                        user_playcount,
                        now_playing: false,
                        user_loved: false,
                        tags: None,
                    }
                })
                .collect::<Vec<_>>();

            Ok(tracks)
        }
    }
}

// Get info for a given user
pub async fn fetch_user_info(
    username: &str,
    api_type: &ApiType,
) -> Result<ScrobbleUser, Box<dyn Error + Send + Sync>> {
    let base_url = get_base_url(api_type);

    match api_type {
        ApiType::Listenbrainz => {
            let url = format!("https://api.listenbrainz.org/1/user/{username}/listen-count");
            let response = CLIENT.get(&url).send().await?;
            let json = response.json::<serde_json::Value>().await?;
            let playcount = json["payload"]["count"].as_u64().unwrap_or_default();

            let url = format!("https://api.listenbrainz.org/1/stats/user/{username}/artists");
            let response = CLIENT.get(&url).send().await?;
            let json = response.json::<serde_json::Value>().await?;
            let artist_count = json["payload"]["total_artist_count"]
                .as_u64()
                .unwrap_or_default();

            let url = format!("https://api.listenbrainz.org/1/stats/user/{username}/releases");
            let response = CLIENT.get(&url).send().await?;
            let json = response.json::<serde_json::Value>().await?;
            let track_count = json["payload"]["total_release_count"]
                .as_u64()
                .unwrap_or_default();

            let url = format!("https://api.listenbrainz.org/1/stats/user/{username}/recordings");
            let response = CLIENT.get(&url).send().await?;
            let json = response.json::<serde_json::Value>().await?;
            let album_count = json["payload"]["total_recording_count"]
                .as_u64()
                .unwrap_or_default();

            let user = ScrobbleUser {
                username: username.to_owned(),
                playcount,
                artist_count,
                track_count,
                album_count,
                profile_pic_url: None,
                registered_date: None,
            };
            Ok(user)
        }
        ApiType::Librefm | ApiType::Lastfm => {
            let url = Url::parse_with_params(
                base_url,
                &[
                    ("method", "user.getInfo"),
                    ("user", username),
                    ("api_key", config::LASTFM_API_KEY),
                    ("format", "json"),
                ],
            )?;
            let response = CLIENT.get(url).send().await?;
            let json = response.json::<serde_json::Value>().await?;
            let user_json = &json["user"];
            let playcount = user_json["playcount"]
                .as_str()
                .unwrap_or_default()
                .parse::<u64>()
                .unwrap_or_default();
            let artist_count = user_json["artist_count"]
                .as_str()
                .unwrap_or_default()
                .parse::<u64>()
                .unwrap_or_default();
            let track_count = user_json["track_count"]
                .as_str()
                .unwrap_or_default()
                .parse::<u64>()
                .unwrap_or_default();
            let album_count = user_json["album_count"]
                .as_str()
                .unwrap_or_default()
                .parse::<u64>()
                .unwrap_or_default();
            let registered_date = if let Some(registered) = user_json["registered"].get("#text") {
                registered.as_u64()
            } else {
                None
            };
            let profile_pic_url = get_biggest_lastfm_image(user_json);
            let user = ScrobbleUser {
                username: username.to_owned(),
                playcount,
                artist_count,
                track_count,
                album_count,
                profile_pic_url,
                registered_date,
            };
            Ok(user)
        }
    }
}
