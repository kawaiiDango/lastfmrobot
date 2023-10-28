#![feature(lazy_cell)]
#![feature(iter_collect_into)]

use std::{
    cmp::min,
    collections::HashSet,
    error::Error,
    fs::File,
    io::{BufRead, BufReader},
    sync::Mutex,
};

use api_requester::{ApiType, TimePeriod};
use db::{Db, User};
use num_format::{Locale, ToFormattedString};
use once_cell::sync::{Lazy, OnceCell};
use rand::seq::SliceRandom;
use reqwest::Url;
use strum_macros::{Display, EnumString, IntoStaticStr};
use teloxide::{
    adaptors::{throttle::Limits, Throttle},
    payloads::SendMessageSetters,
    prelude::*,
    types::{
        BotCommand, InlineKeyboardButton, InlineKeyboardMarkup, InlineQueryResult,
        InlineQueryResultArticle, InlineQueryResultPhoto, InputFile, InputMediaPhoto,
        InputMessageContent, InputMessageContentText, Me, MessageEntityKind, ParseMode,
    },
    utils::command::BotCommands,
};
use tokio::task;
use utils::choose_the_from;

use crate::api_requester::EntryType;
mod anal;
mod api_requester;
mod collage;
mod config;
mod consts;
mod db;
mod utils;

type Bot = Throttle<teloxide::Bot>;

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
enum Command {
    #[command(description = "Vrooooooom!")]
    Start,
    #[command(description = "Your last played song")]
    Status,
    Np,
    #[command(description = "Your last 3 songs and album art")]
    #[allow(non_camel_case_types)]
    Status_Full,
    NpFull,
    #[command(description = "Your last 5 loved tracks")]
    Loved,
    #[command(description = "Your compatibility score")]
    Compat {
        arg: String,
    },
    #[command(description = "Create album collage")]
    Collage {
        arg: String,
    },
    #[command(description = "A random top artist/album/track")]
    Random {
        arg: String,
    },
    #[command(description = "Top 5 artists/albums/tracks as text")]
    Topkek {
        arg: String,
    },
    #[command(description = "Flewx your nuwmbers")]
    Flex,
    #[command(description = "Set your username")]
    Set {
        arg: String,
    },
    #[command(description = "Unlink yourself from this bot")]
    Unset,
    #[command(description = "Show/hide your profile link")]
    #[allow(non_camel_case_types)]
    User_Settings,
    #[command(description = "Weeeeelp!")]
    Help,
}

static DB: Lazy<Mutex<Db>> = Lazy::new(|| Mutex::new(Db::new()));
static ME: OnceCell<Me> = OnceCell::new();
static ACCEPTABLE_TAGS: Lazy<HashSet<String>> = Lazy::new(|| {
    BufReader::new(File::open("everynoise_genres.txt").unwrap())
        .lines()
        .map(|x| x.unwrap())
        .collect()
});

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    pretty_env_logger::init_timed();

    let bot = teloxide::Bot::new(config::BOT_TOKEN).throttle(Limits {
        messages_per_sec_chat: 1,
        messages_per_sec_overall: 30,
        messages_per_min_chat: 10,
        messages_per_min_channel: 10,
    });

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(message_handler))
        .branch(Update::filter_callback_query().endpoint(callback_handler))
        .branch(Update::filter_inline_query().endpoint(inline_query_handler))
        .branch(Update::filter_my_chat_member().endpoint(my_chat_member_handler))
        .branch(Update::filter_chosen_inline_result().endpoint(inline_result_handler));

    let visible_commands: HashSet<&str> = vec![
        "status",
        "status_full",
        "loved",
        "collage",
        "compat",
        "random",
        "topkek",
        "flex",
        "user_settings",
    ]
    .into_iter()
    .collect();

    bot.send_message(config::OWNER_ID.to_string(), consts::BOT_STARTED)
        .await?;
    ME.set(bot.get_me().await?).unwrap();

    let commands: Vec<BotCommand> = Command::bot_commands()
        .iter()
        .filter(|bc| visible_commands.contains(&bc.command[1..])) // because it includes the /
        .cloned()
        .collect();

    bot.set_my_commands(commands).await?;

    Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
    Ok(())
}

async fn track(event_type: &str, user: Option<&teloxide::types::User>) {
    anal::add_event(
        event_type,
        user,
        ME.get().unwrap().username.clone().unwrap(),
    )
    .await
    .unwrap_or_default();
}

async fn message_handler(bot: Bot, msg: Message) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(text) = msg.text() {
        let _from = msg.from().cloned();
        let from = _from.as_ref();

        if from.is_none() {
            return Ok(());
        }

        if from.unwrap().is_anonymous() {
            utils::send_or_edit_message(bot, consts::ANON_KUN, None, None, false, None, true)
                .await?;
            return Ok(());
        }

        let mut parsed_command = BotCommands::parse(text, ME.get().unwrap().username());

        // commands without a /
        if parsed_command.is_err() {
            let splits: Vec<_> = text.splitn(2, ' ').map(|x| x.to_lowercase()).collect();
            let first_word = splits.get(0).map(|x| x.as_str()); //.cloned();
            let second_word = splits.get(1).cloned();

            parsed_command = match first_word {
                Some("status") => Ok(Command::Status),
                Some("statusfull") => Ok(Command::Status_Full),
                Some("collage") => {
                    if second_word.is_some() {
                        Ok(Command::Collage {
                            arg: second_word.unwrap(),
                        })
                    } else {
                        parsed_command
                    }
                }
                Some("compat") => Ok(Command::Compat {
                    arg: second_word.unwrap_or_default(),
                }),
                _ => parsed_command,
            }
        }

        let user: User;
        match parsed_command {
            Ok(Command::Start) => {
                start_command(bot, msg.chat.id).await?;
                track("start", from).await;
                return Ok(());
            }
            Ok(Command::Help) => {
                bot.send_message(msg.chat.id, Command::descriptions().to_string())
                    .reply_to_message_id(msg.id)
                    .allow_sending_without_reply(true)
                    .await?;
                track("help", from).await;
                return Ok(());
            }
            Ok(Command::Set { arg }) => {
                set_command(bot.clone(), msg.clone(), None, &arg, false).await?;
                track("set", from).await;
                return Ok(());
            }
            Ok(_) => {
                let u =
                    get_registered_user(bot.clone(), msg.clone().into(), None, None, false).await;
                if let Ok(u) = u {
                    user = u;
                } else {
                    return Ok(());
                }
            }
            Err(_) => {
                return Ok(());
            }
        }

        match parsed_command {
            Ok(Command::Status) | Ok(Command::Np) => {
                status_command(
                    bot,
                    msg.into(),
                    None,
                    None,
                    false,
                    StatusType::Compact,
                    false,
                    user,
                )
                .await?;
                track("status_compact", from).await;
            }
            Ok(Command::Status_Full) | Ok(Command::NpFull) => {
                status_command(
                    bot,
                    msg.into(),
                    None,
                    None,
                    false,
                    StatusType::Expanded,
                    false,
                    user,
                )
                .await?;
                track("status_full", from).await;
            }
            Ok(Command::Loved) => {
                loved_command(bot, msg.into(), None, None, false, user).await?;
                track("loved", from).await;
            }
            Ok(Command::User_Settings) => {
                user_settings_command(bot, msg.into(), None, None, false, "", user).await?;
                track("user_settings", from).await;
            }
            Ok(Command::Collage { arg }) => {
                collage_command(bot, msg.into(), None, None, false, &arg, user).await?;
                track("collage", from).await;
            }
            Ok(Command::Topkek { arg }) => {
                top_command(bot, msg.into(), None, None, false, &arg, user).await?;
                track("top", from).await;
            }
            Ok(Command::Compat { arg }) => {
                compat_command(bot, msg, &arg, user).await?;
                track("compat", from).await;
            }
            Ok(Command::Unset) => {
                unset_command(bot, msg, user).await?;
                track("unset", from).await;
            }
            Ok(Command::Random { arg }) => {
                random_chooser_command(bot, msg.into(), None, None, false, &arg, user).await?;
                track("random", from).await;
            }
            Ok(Command::Flex) => {
                flex_command(bot, msg.into(), None, None, false, user).await?;
                track("flex", from).await;
            }

            Err(_) => {}

            _ => {}
        }
    }

    Ok(())
}

async fn get_registered_user(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
) -> Result<db::User, Box<dyn Error + Send + Sync>> {
    let from = utils::choose_the_from(msg.as_ref(), inline_from);

    let user = DB.lock().unwrap().fetch_user(from.id.0);
    match user {
        Some(user) => Ok(user),

        None => {
            utils::send_or_edit_message(
                bot,
                consts::NOT_REGISTERED,
                msg,
                inline_message_id,
                edit,
                None,
                true,
            )
            .await?;
            Err(Box::from(consts::NOT_REGISTERED))
        }
    }
}

async fn send_err_msg(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    edit: bool,
    e: Box<dyn Error + Send + Sync>,
) {
    log::error!("{e}");
    let text = if let Some(middleware_error) = e.downcast_ref::<reqwest_middleware::Error>() {
        middleware_error
            .source()
            .map(|e| e.to_string())
            .unwrap_or(consts::ERR_MSG.to_string())
    } else {
        consts::ERR_MSG.to_string()
    };

    utils::send_or_edit_message(bot, text.as_str(), msg, inline_message_id, edit, None, true)
        .await
        .unwrap_or_default();
}

async fn my_chat_member_handler(
    bot: Bot,
    me: Me,
    chat_member_updated: ChatMemberUpdated,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if chat_member_updated.new_chat_member.user.id == me.id {
        start_command(bot, chat_member_updated.chat.id).await?;
    }
    Ok(())
}

async fn start_command(bot: Bot, chat_id: ChatId) -> Result<(), Box<dyn Error + Send + Sync>> {
    bot.send_message(chat_id, consts::WELCOME_TEXT)
        .parse_mode(ParseMode::Html)
        .await?;
    Ok(())
}

#[derive(Debug, PartialEq, Display, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
enum StatusType {
    Compact,
    CompactWithCover,
    Expanded,
}

async fn status_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    status_type: StatusType,
    prefer_cached: bool,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let from = utils::choose_the_from(msg.as_ref(), inline_from);

    let limit = if status_type == StatusType::Expanded {
        4
    } else {
        1
    };
    let tracks = api_requester::fetch_recent_tracks(
        user.account_username.as_str(),
        &user.api_type(),
        prefer_cached,
        limit,
    )
    .await;

    match tracks {
        Ok(tracks) => {
            if tracks.is_empty() {
                let text = consts::NO_SCROBBLES;
                utils::send_or_edit_message(bot, text, msg, inline_message_id, edit, None, true)
                    .await?;

                return Ok(());
            }

            let album_art = if tracks[0].album_art_url.is_some() {
                format!(
                    "<a href=\"{}\">\u{200B}</a>\u{200B}",
                    tracks[0].album_art_url.as_ref().unwrap()
                )
            } else {
                "".to_string()
            };

            let mut user_playcount = 0;
            let mut tags_text: String = "".to_string();
            if user.api_type() == ApiType::Lastfm {
                let track_info = api_requester::fetch_lastfm_track(
                    user.account_username.clone(),
                    tracks[0].artist.clone(),
                    tracks[0].name.clone(),
                )
                .await;

                if let Ok(track_info) = track_info {
                    user_playcount = track_info.user_playcount;
                    tags_text = track_info
                        .tags
                        .unwrap_or_default()
                        .iter()
                        .map(|t| t.to_lowercase())
                        .filter(|t| t.split(' ').any(|x| ACCEPTABLE_TAGS.contains(x)))
                        .map(|t| {
                            t.replace(
                                &['(', ')', ',', '\"', '.', ';', ':', '\'', '-', ' ', '/'][..],
                                "_",
                            )
                        })
                        .filter(|x| !x.is_empty())
                        .map(|x| format!("#{x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                }
            }

            let mut first_track_info = if user_playcount > 0 {
                format!(", {user_playcount} plays")
            } else {
                "".to_owned()
            };

            if !tags_text.is_empty() {
                first_track_info = format!("{first_track_info}\n\n{tags_text}\n");
            }

            let tracks_text = tracks
                .iter()
                .take(limit)
                .map(|track| {
                    let time_ago = if track.date.is_some() {
                        ", ".to_owned() + &utils::convert_to_timeago(track.date.unwrap())
                    } else {
                        "".to_owned()
                    };

                    let spotify_url_str = format!("{} — {}", tracks[0].artist, tracks[0].name);
                    let fragment = url_escape::encode_fragment(&spotify_url_str);

                    let spotify_url =
                        Url::parse(&format!("https://open.spotify.com/search/{}", &fragment))
                            .unwrap();

                    let s = format!(
                        "🎧 <i>{}</i> — <a href=\"{}\"><b>{}</b></a>{}{}{}{}",
                        utils::replace_html_symbols(&track.artist),
                        spotify_url,
                        utils::replace_html_symbols(&track.name),
                        track
                            .album
                            .as_ref()
                            .map(|x| format!(", [{}]", utils::replace_html_symbols(x)))
                            .unwrap_or("".to_string()),
                        time_ago,
                        if track.user_loved { ", 💗 loved" } else { "" },
                        first_track_info,
                    );

                    first_track_info = "".to_owned();
                    s
                })
                .collect::<Vec<String>>()
                .join("\n");

            let text = format!(
                "{}{} {} listening to\n{}{}",
                album_art,
                utils::name_with_link(&from, &user),
                if tracks[0].now_playing {
                    "is now"
                } else {
                    "was"
                },
                tracks_text,
                first_track_info,
            );

            let mut keyboard = vec![vec![]];

            match status_type {
                StatusType::Expanded => {
                    keyboard[0].push(InlineKeyboardButton::callback(
                        "➖",
                        format!("{} status {}", from.id.0, StatusType::Compact),
                    ));
                }
                StatusType::Compact => {
                    if tracks[0].album_art_url.is_some() {
                        keyboard[0].push(InlineKeyboardButton::callback(
                            "🖼️",
                            format!("{} status {}", from.id.0, StatusType::CompactWithCover),
                        ));
                    }
                    keyboard[0].push(InlineKeyboardButton::callback(
                        "➕",
                        format!("{} status {}", from.id.0, StatusType::Expanded),
                    ));
                }
                StatusType::CompactWithCover => {
                    keyboard[0].push(InlineKeyboardButton::callback(
                        "➖",
                        format!("{} status {}", from.id.0, StatusType::Compact),
                    ));
                    keyboard[0].push(InlineKeyboardButton::callback(
                        "➕",
                        format!("{} status {}", from.id.0, StatusType::Expanded),
                    ));
                }
            }

            if inline_message_id.is_none() {
                keyboard[0].push(InlineKeyboardButton::callback("ℹ️", "0 info"));
            }

            keyboard[0].push(InlineKeyboardButton::callback(
                "🔃",
                format!("{} status_refresh {}", from.id.0, status_type),
            ));

            utils::send_or_edit_message(
                bot,
                &text,
                msg,
                inline_message_id,
                edit,
                Some(InlineKeyboardMarkup::new(keyboard)),
                status_type == StatusType::Compact,
            )
            .await?;
        }

        Err(e) => {
            send_err_msg(bot, msg, inline_message_id, edit, e).await;
        }
    }

    Ok(())
}

async fn loved_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let from = utils::choose_the_from(msg.as_ref(), inline_from);

    let tracks =
        api_requester::fetch_loved_tracks(user.account_username.as_str(), &user.api_type()).await;

    match tracks {
        Ok(tracks) => {
            if tracks.is_empty() {
                let text = consts::NO_SCROBBLES;
                utils::send_or_edit_message(bot, text, msg, inline_message_id, edit, None, true)
                    .await?;

                return Ok(());
            }

            let tracks_text = tracks
                .iter()
                .map(|track| {
                    let time_ago = if track.date.is_none() {
                        "".to_owned()
                    } else {
                        ", ".to_owned() + &utils::convert_to_timeago(track.date.unwrap())
                    };

                    let spotify_url_str = format!("{} — {}", track.artist, track.name);
                    let fragment = url_escape::encode_fragment(&spotify_url_str);
                    let spotify_url = format!("https://open.spotify.com/search/{}", &fragment);

                    format!(
                        "💗 <i>{}</i> — <a href=\"{}\"><b>{}</b></a>{}",
                        utils::replace_html_symbols(&track.artist),
                        spotify_url,
                        utils::replace_html_symbols(&track.name),
                        time_ago,
                    )
                })
                .collect::<Vec<String>>()
                .join("\n");

            let text = format!(
                "{}'s loved tracks:\n{}",
                utils::name_with_link(&from, &user),
                tracks_text,
            );

            utils::send_or_edit_message(bot, &text, msg, inline_message_id, edit, None, true)
                .await?;
        }

        Err(e) => {
            send_err_msg(bot, msg, inline_message_id, edit, e).await;
        }
    }

    Ok(())
}

async fn set_command(
    bot: Bot,
    msg: Message,
    inline_from: Option<&teloxide::types::User>,
    arg: &str,
    edit: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if arg.is_empty() {
        utils::send_or_edit_message(bot, consts::SET_CLICK, msg.into(), None, edit, None, true)
            .await?;
        return Ok(());
    }

    let from = choose_the_from(Some(&msg), inline_from);

    let arg_splits = arg.splitn(2, ' ').collect::<Vec<_>>();

    let username = arg_splits[0];
    let api_type_str = arg_splits.get(1).cloned().unwrap_or_default();
    let api_type = api_type_str.parse().unwrap_or(ApiType::Lastfm);

    let recent_tracks = api_requester::fetch_recent_tracks(username, &api_type, false, 1).await;

    let buttons = vec![ApiType::Lastfm, ApiType::Listenbrainz, ApiType::Librefm]
        .iter()
        .filter(|&x| x != &api_type)
        .map(|x| {
            InlineKeyboardButton::callback(
                x.to_string(),
                format!("{} set {} {}", from.id.0, username, x),
            )
        })
        .collect::<Vec<_>>();

    let keyboard = InlineKeyboardMarkup::new(vec![buttons]);

    let text = match recent_tracks {
        Ok(_) => {
            let new_user = db::User::new(from.id.0, username.to_owned(), &api_type, false);

            DB.lock().unwrap().upsert_user(&new_user)?;
            format!(
                "✅Username set for {0}!\n\nUse /user_settings to show/hide links to your {0} profile.\n\nNot {0}? Change your account type using the buttons.",
                api_type
            )
        }

        Err(e) => {
            log::error!("{e}");
            if let Some(middleware_error) = e.downcast_ref::<reqwest_middleware::Error>() {
                format!(
                    "{}\n\n{} for {}\n\nChange your account type using the buttons.",
                    middleware_error
                        .source()
                        .map(|e| e.to_string())
                        .unwrap_or(consts::ERR_MSG.to_string()),
                    consts::USER_NOT_FOUND,
                    api_type
                )
            } else {
                consts::ERR_MSG.to_string()
            }
        }
    };

    utils::send_or_edit_message(bot, &text, msg.into(), None, edit, keyboard.into(), true).await?;

    Ok(())
}

async fn user_settings_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    arg: &str,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let from = utils::choose_the_from(msg.as_ref(), inline_from);
    let mut user = user;
    match arg {
        "profile_show" => {
            user.profile_shown = true;
            DB.lock().unwrap().upsert_user(&user)?;
        }
        "profile_hide" => {
            user.profile_shown = false;
            DB.lock().unwrap().upsert_user(&user)?;
        }
        _ => {}
    }

    let mut buttons = vec![vec![]];

    if !user.profile_shown {
        buttons[0].push(InlineKeyboardButton::callback(
            format!("Show {} profile links", user.api_type()),
            format!("{} user_settings profile_show", from.id,),
        ));
    } else {
        buttons[0].push(InlineKeyboardButton::callback(
            format!("Hide {} profile links", user.api_type()),
            format!("{} user_settings profile_hide", from.id,),
        ));
    }

    let name_text = utils::name_with_link(&from, &user);
    utils::send_or_edit_message(
        bot,
        &format!("Settings for {}", name_text),
        msg,
        inline_message_id,
        edit,
        InlineKeyboardMarkup::new(buttons).into(),
        true,
    )
    .await?;

    Ok(())
}

async fn top_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    arg: &str,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let n = 5;
    let from = utils::choose_the_from(msg.as_ref(), inline_from);

    if arg.is_empty() {
        utils::send_or_edit_message(bot, consts::TOP_CLICK, msg, None, false, None, true).await?;
        return Ok(());
    }

    let (_, period, entry_type, _) = utils::parse_collage_arg(arg);

    let text = match entry_type {
        EntryType::Artist => {
            api_requester::fetch_artists(&user.account_username, &period, &user.api_type(), None)
                .await
                .map(|entries| {
                    entries
                        .iter()
                        .take(n)
                        .map(|entry| {
                            let fragment = url_escape::encode_fragment(&entry.name);
                            let spotify_url =
                                format!("https://open.spotify.com/search/{}", &fragment);

                            format!(
                                "<a href=\"{}\">{}</a> -> {} plays",
                                spotify_url,
                                utils::replace_html_symbols(&entry.name),
                                entry.user_playcount.to_formatted_string(&Locale::en)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
        }
        EntryType::Album => {
            api_requester::fetch_albums(&user.account_username, &period, &user.api_type(), None)
                .await
                .map(|entries| {
                    entries
                        .iter()
                        .take(n)
                        .map(|entry| {
                            let spotify_search_str = format!("{} {}", entry.name, entry.artist);
                            let fragment = url_escape::encode_fragment(spotify_search_str.as_str());
                            let spotify_url =
                                format!("https://open.spotify.com/search/{}", fragment);

                            format!(
                                "<a href=\"{}\">{} — {}</a> -> {} plays",
                                spotify_url,
                                utils::replace_html_symbols(&entry.artist),
                                utils::replace_html_symbols(&entry.name),
                                entry.user_playcount.to_formatted_string(&Locale::en)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
        }
        EntryType::Track => {
            api_requester::fetch_tracks(&user.account_username, &period, &user.api_type(), None)
                .await
                .map(|entries| {
                    entries
                        .iter()
                        .take(n)
                        .map(|entry| {
                            let spotify_search_str = format!("{} {}", entry.name, entry.artist);
                            let fragment = url_escape::encode_fragment(spotify_search_str.as_str());
                            let spotify_url =
                                format!("https://open.spotify.com/search/{}", &fragment);

                            format!(
                                "<a href=\"{}\">{} — {}</a> -> {} plays",
                                spotify_url,
                                utils::replace_html_symbols(&entry.artist),
                                utils::replace_html_symbols(&entry.name),
                                entry.user_playcount.to_formatted_string(&Locale::en)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
        }
    };
    utils::send_or_edit_message(
        bot,
        &format!(
            "{}'s top {}s for {}\n\n{}",
            utils::name_with_link(&from, &user),
            entry_type,
            period,
            text?
        ),
        msg,
        inline_message_id,
        edit,
        None,
        true,
    )
    .await?;

    Ok(())
}
async fn collage_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    arg: &str,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let from = utils::choose_the_from(msg.as_ref(), inline_from);

    if arg.is_empty() {
        utils::send_or_edit_message(bot, consts::COLLAGE_CLICK, msg, None, false, None, true)
            .await?;
        return Ok(());
    }

    if user.api_type() == ApiType::Librefm {
        utils::send_or_edit_message(
            bot,
            consts::COLLAGE_LIBREFM,
            msg,
            inline_message_id,
            edit,
            None,
            true,
        )
        .await?;
        return Ok(());
    }

    let (size, period, _, no_text) = utils::parse_collage_arg(arg);

    let albums =
        api_requester::fetch_albums(&user.account_username, &period, &user.api_type(), None).await;
    match albums {
        Ok(albums) => {
            let img = collage::create_collage(&albums, size, !no_text).await;
            match img {
                Ok(img) => {
                    let period_str = period.to_string();
                    let period_str_cb_data = period_str.replace(' ', "_");
                    let caption = format!(
                        "{}'s {} album collage",
                        utils::name_with_link(&from, &user),
                        period_str,
                    );

                    let notext_str = if no_text { "clean" } else { "" };
                    let notext_str_inverse = if no_text { "" } else { "clean" };

                    let mut buttons = vec![vec![]];

                    if size < collage::MAX_SIZE {
                        buttons[0].push(InlineKeyboardButton::callback(
                            "➕",
                            format!(
                                "{} collage {} {} {}",
                                from.id,
                                size + 1,
                                period_str_cb_data,
                                notext_str
                            ),
                        ));
                    }

                    if size > collage::MIN_SIZE {
                        buttons[0].push(InlineKeyboardButton::callback(
                            "➖",
                            format!(
                                "{} collage {} {} {}",
                                from.id,
                                size - 1,
                                period_str_cb_data,
                                notext_str
                            ),
                        ));
                    }

                    buttons[0].push(InlineKeyboardButton::callback(
                        "Aa",
                        format!(
                            "{} collage {} {} {}",
                            from.id, size, period_str_cb_data, notext_str_inverse
                        ),
                    ));

                    let keyboard = InlineKeyboardMarkup::new(buttons);

                    utils::send_or_edit_photo(
                        bot,
                        InputMediaPhoto::new(InputFile::memory(img))
                            .caption(caption)
                            .parse_mode(ParseMode::Html),
                        msg,
                        inline_message_id.as_ref(),
                        edit,
                        Some(keyboard),
                        true,
                    )
                    .await?;
                }
                Err(e) => {
                    log::error!("collage generator failed {e}");
                    send_err_msg(bot, msg, inline_message_id, edit, e.into()).await;
                }
            }
        }
        Err(e) => {
            log::error!("user.gettopalbums failed {e}");
            send_err_msg(bot, msg, inline_message_id, edit, e).await;
        }
    }

    Ok(())
}

async fn random_chooser_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    arg: &str,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if arg.is_empty() {
        let from = utils::choose_the_from(msg.as_ref(), inline_from);
        let user_id = from.id.0;
        let keyboard = InlineKeyboardMarkup::new(vec![vec![
            InlineKeyboardButton::callback("🎵 Track", format!("{} random track", user_id)),
            InlineKeyboardButton::callback("💿 Album", format!("{} random album", user_id)),
            InlineKeyboardButton::callback("🎙️ Artist", format!("{} random artist", user_id)),
        ]]);

        utils::send_or_edit_message(
            bot,
            "Choose:",
            msg,
            inline_message_id,
            edit,
            keyboard.into(),
            true,
        )
        .await?;
    } else {
        random_command(bot, msg, inline_message_id, inline_from, edit, arg, user).await?;
    }
    Ok(())
}

async fn random_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    arg: &str,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let from = utils::choose_the_from(msg.as_ref(), inline_from);

    let username = user.account_username.to_owned();
    let api_type = user.api_type();
    let limit = if api_type == ApiType::Listenbrainz {
        100
    } else {
        1000
    };

    let text: Option<String>;
    let mut search_text: Option<String> = None;
    match arg {
        "artist" => {
            let arr = api_requester::fetch_artists(
                &username,
                &TimePeriod::AllTime,
                &api_type,
                limit.into(),
            )
            .await?;
            text = arr.choose(&mut rand::thread_rng()).map(|x| {
                search_text = x.name.clone().into();
                format!(
                    "{}\n({} plays)",
                    utils::replace_html_symbols(&x.name),
                    x.user_playcount.to_formatted_string(&Locale::en)
                )
            });
        }
        "album" => {
            let arr = api_requester::fetch_albums(
                &username,
                &TimePeriod::AllTime,
                &api_type,
                limit.into(),
            )
            .await?;
            text = arr.choose(&mut rand::thread_rng()).map(|x| {
                search_text = (x.artist.clone() + " " + &x.name.clone()).into();
                format!(
                    "{} — {}\n({} plays)",
                    utils::replace_html_symbols(&x.artist),
                    utils::replace_html_symbols(&x.name),
                    x.user_playcount.to_formatted_string(&Locale::en)
                )
            });
        }
        "track" => {
            let arr = api_requester::fetch_tracks(
                &username,
                &TimePeriod::AllTime,
                &api_type,
                limit.into(),
            )
            .await?;
            text = arr.choose(&mut rand::thread_rng()).map(|x| {
                search_text = (x.artist.clone() + " " + &x.name.clone()).into();
                format!(
                    "{} — {}\n({} plays)",
                    utils::replace_html_symbols(&x.artist),
                    utils::replace_html_symbols(&x.name),
                    x.user_playcount.to_formatted_string(&Locale::en)
                )
            });
        }
        _ => {
            return Ok(());
        }
    }
    match text {
        Some(text) => {
            let search_text_str = search_text.unwrap();
            let fragment = url_escape::encode_fragment(&search_text_str);

            let spotify_url =
                Url::parse(&format!("https://open.spotify.com/search/{}", &fragment)).unwrap();

            let keyboard = InlineKeyboardMarkup::new(vec![vec![
                InlineKeyboardButton::url("🔎", spotify_url),
                InlineKeyboardButton::callback("🔃", format!("{} random {}", from.id.0, arg)),
            ]]);
            utils::send_or_edit_message(
                bot,
                &text,
                msg,
                inline_message_id,
                edit,
                keyboard.into(),
                true,
            )
            .await?;
        }
        None => {
            utils::send_or_edit_message(
                bot,
                consts::NOT_FOUND,
                msg,
                inline_message_id,
                edit,
                None,
                true,
            )
            .await?;
        }
    }

    Ok(())
}

async fn flex_command(
    bot: Bot,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    inline_from: Option<&teloxide::types::User>,
    edit: bool,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let from = utils::choose_the_from(msg.as_ref(), inline_from);

    let scrobble_user =
        api_requester::fetch_user_info(&user.account_username, &user.api_type()).await?;

    let profile_pic_url = scrobble_user.profile_pic_url.unwrap_or(
        "https://lastfm.freetls.fastly.net/i/u/avatar170s/818148bf682d429dc215c1705eb27b98.png"
            .to_owned(),
    );

    let scrobbling_since = scrobble_user
        .registered_date
        .map(|x| "\n\nSince ".to_owned() + &utils::format_epoch_secs(x, false))
        .unwrap_or_default();

    let text = format!(
        "{}\n\n{} artists\n{} albums\n{} tracks\n{} plays{}",
        utils::name_with_link(&from, &user),
        scrobble_user.artist_count.to_formatted_string(&Locale::en),
        scrobble_user.album_count.to_formatted_string(&Locale::en),
        scrobble_user.track_count.to_formatted_string(&Locale::en),
        scrobble_user.playcount.to_formatted_string(&Locale::en),
        scrobbling_since,
    );

    let media = InputMediaPhoto::new(InputFile::url(Url::parse(&profile_pic_url).unwrap()))
        .caption(text)
        .parse_mode(ParseMode::Html);

    utils::send_or_edit_photo(
        bot,
        media,
        msg,
        inline_message_id.as_ref(),
        edit,
        None,
        false,
    )
    .await?;
    Ok(())
}

async fn compat_command(
    bot: Bot,
    msg: Message,
    arg: &str,
    db_user1_u: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let user1 = msg.from().unwrap();
    let reply_to_msg = msg.reply_to_message();

    if reply_to_msg.is_none() || reply_to_msg.unwrap().from().is_none() {
        utils::send_or_edit_message(
            bot,
            consts::COMPAT_CLICK,
            msg.into(),
            None,
            false,
            None,
            true,
        )
        .await?;

        return Ok(());
    }

    let user2 = reply_to_msg.unwrap().from().unwrap();
    let db_user2 = DB.lock().unwrap().fetch_user(user2.id.0);

    let text: String = if user1.id.0 == user2.id.0 {
        consts::ITS_ME.to_string()
    } else if user1.is_bot || user2.is_bot {
        consts::BOTS_MUSIC.to_string()
    } else if db_user2.is_none() {
        consts::THEY_NOT_REGISTERED.to_string()
    } else {
        let (_size, period, _, _no_text) = utils::parse_collage_arg(arg);
        let period_text = period.to_string();

        let db_user2_u = db_user2.unwrap();

        let username1 = db_user1_u.account_username.clone();
        let username2 = db_user2_u.account_username.clone();
        let api_type1 = db_user1_u.api_type();
        let api_type2 = db_user2_u.api_type();

        let artists1 =
            api_requester::fetch_artists(&username1, &TimePeriod::OneYear, &api_type1, None)
                .await?;
        let artists2 =
            api_requester::fetch_artists(&username2, &TimePeriod::OneYear, &api_type2, None)
                .await?;

        let mut numerator = 0;
        let mut mutual: Vec<String> = Vec::new();
        let denominator = min(min(artists1.len(), artists2.len()), 40);

        for artist1 in &artists1 {
            for artist2 in &artists2 {
                if artist1.name == artist2.name {
                    numerator += 1;
                    if mutual.len() < 8 {
                        mutual.push(artist1.name.clone());
                    }
                    break;
                }
            }
        }

        log::info!("common artists = {}/{}", numerator, denominator);

        let mut score = 0;
        if denominator > 2 {
            score = numerator * 100 / denominator;
        }
        if score > 100 {
            score = 100;
        }

        if mutual.is_empty() || score == 0 {
            format!("No common artists in {}", period_text)
        } else {
            format!(
                "{} and {} listen to {}\n\nCompatibility score is {}%, based on {}",
                utils::name_with_link(user1, &db_user1_u),
                utils::name_with_link(user2, &db_user2_u),
                mutual
                    .iter()
                    .map(|x| utils::replace_html_symbols(x))
                    .collect::<Vec<_>>()
                    .join(", ")
                    + "...",
                score,
                period_text,
            )
        }
    };

    utils::send_or_edit_message(bot, text.as_str(), msg.into(), None, false, None, true).await?;
    Ok(())
}

async fn unset_command(
    bot: Bot,
    msg: Message,
    user: User,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    DB.lock().unwrap().delete_user(user.tg_user_id).unwrap();

    utils::send_or_edit_message(bot, consts::UNSET, msg.into(), None, false, None, true).await?;

    Ok(())
}

async fn inline_query_handler(
    bot: Bot,
    q: InlineQuery,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let user = DB.lock().unwrap().fetch_user(q.from.id.0);

    let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
        "🎹",
        "0 loading",
    )]]);

    let status = InlineQueryResultArticle::new(
        "status",
        "Status",
        InputMessageContent::Text(InputMessageContentText::new("Staaaaaaatus")),
    )
    .reply_markup(keyboard.clone());

    let status_full = InlineQueryResultArticle::new(
        "status_full",
        "Expanded Status",
        InputMessageContent::Text(InputMessageContentText::new("Expanded Staaaaatus")),
    )
    .reply_markup(keyboard.clone());

    let loved = InlineQueryResultArticle::new(
        "loved",
        "Loved",
        InputMessageContent::Text(InputMessageContentText::new("Loved")),
    )
    .reply_markup(keyboard.clone());

    let collage3 = InlineQueryResultPhoto::new(
        "collage 3",
        Url::parse(consts::URL_3X3_ALBUM)?,
        Url::parse(consts::URL_3X3_ALBUM)?,
    )
    .reply_markup(keyboard.clone());

    let random = InlineQueryResultArticle::new(
        "random",
        "Shuffle your scrobbles",
        InputMessageContent::Text(InputMessageContentText::new("Shuffle your scrobbles")),
    )
    .reply_markup(keyboard.clone());

    let flex = InlineQueryResultArticle::new(
        "flex",
        "Flex your numbers",
        InputMessageContent::Text(InputMessageContentText::new("Flex your numbers")),
    )
    .reply_markup(keyboard.clone());

    let results = vec![
        InlineQueryResult::Article(status),
        InlineQueryResult::Article(status_full),
        InlineQueryResult::Article(loved),
        InlineQueryResult::Article(random),
        InlineQueryResult::Article(flex),
        InlineQueryResult::Photo(collage3),
    ];

    if user.is_none() {
        bot.answer_inline_query(q.id, [])
            .cache_time(0)
            .is_personal(true)
            .switch_pm_text(consts::NOT_REGISTERED_INLINE)
            .switch_pm_parameter("set")
            .await?;
    } else {
        bot.answer_inline_query(q.id, results)
            .is_personal(true)
            .cache_time(86400)
            .await?;
    }

    track("inline_query", Some(&q.from)).await;

    Ok(())
}

async fn inline_result_handler(
    bot: Bot,
    chosen_inline_result: ChosenInlineResult,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let splits = chosen_inline_result
        .result_id
        .splitn(2, ' ')
        .collect::<Vec<_>>();
    let result_id = *splits.first().unwrap_or(&"");
    let arg = *splits.last().unwrap_or(&"");
    let from = Some(&chosen_inline_result.from);
    let user = get_registered_user(
        bot.clone(),
        None,
        chosen_inline_result.inline_message_id.clone(),
        from,
        false,
    )
    .await;
    if user.is_err() {
        return Ok(());
    }

    let user = user.unwrap();

    match result_id {
        "status" => {
            status_command(
                bot,
                None,
                chosen_inline_result.inline_message_id,
                from,
                true,
                StatusType::Compact,
                false,
                user,
            )
            .await?;
            track("inline_status_compact", from).await;
        }
        "status_full" => {
            status_command(
                bot,
                None,
                chosen_inline_result.inline_message_id,
                from,
                true,
                StatusType::Expanded,
                false,
                user,
            )
            .await?;
            track("inline_status_full", from).await;
        }
        "loved" => {
            loved_command(
                bot,
                None,
                chosen_inline_result.inline_message_id,
                from,
                true,
                user,
            )
            .await?;
            track("inline_loved", from).await;
        }
        "collage" => {
            collage_command(
                bot,
                None,
                chosen_inline_result.inline_message_id,
                from,
                true,
                arg,
                user,
            )
            .await?;
            track("inline_collage", from).await;
        }
        "random" => {
            random_chooser_command(
                bot,
                None,
                chosen_inline_result.inline_message_id,
                from,
                true,
                "",
                user,
            )
            .await?;
            track("inline_random", from).await;
        }
        "flex" => {
            flex_command(
                bot,
                None,
                chosen_inline_result.inline_message_id,
                from,
                true,
                user,
            )
            .await?;
            track("inline_flex", from).await;
        }
        _ => {
            log::error!("Unknown result id: {result_id}");
        }
    }

    Ok(())
}

async fn fetch_lastfm_infos(
    username: String,
    artist_p: String,
    title_p: String,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let artist_req = task::spawn(api_requester::fetch_lastfm_artist(
        username.clone(),
        artist_p.clone(),
    ));
    let track_req = task::spawn(api_requester::fetch_lastfm_track(
        username, artist_p, title_p,
    ));

    let artist = artist_req
        .await?
        .map(|e| {
            format!(
                "🎙️ {}:\n{} plays\n{} 🌎 listeners\n{} 🌎 scrobbles",
                e.name,
                e.user_playcount.to_formatted_string(&Locale::en),
                e.listeners.to_formatted_string(&Locale::en),
                e.playcount.to_formatted_string(&Locale::en)
            )
        })
        .unwrap_or_default();
    let track = track_req
        .await?
        .map(|e| {
            format!(
                "🎵 {}{}:\n{} plays\n{} 🌎 listeners\n{} 🌎 scrobbles",
                e.name,
                format!(" ({})", utils::human_readable_duration(e.duration)),
                e.user_playcount.to_formatted_string(&Locale::en),
                e.listeners.to_formatted_string(&Locale::en),
                e.playcount.to_formatted_string(&Locale::en)
            )
        })
        .unwrap_or_default();

    let text = format!("{track}\n\n{artist}");

    Ok(text)
}

async fn callback_handler(bot: Bot, q: CallbackQuery) -> Result<(), Box<dyn Error + Send + Sync>> {
    let callback_data = q.data.unwrap();
    let splits: Vec<&str> = callback_data.splitn(3, ' ').collect();
    let allowed_user_id: u64 = splits[0].parse()?;
    let data = splits[1];
    let arg = if splits.len() == 3 {
        splits[2].to_lowercase()
    } else {
        "".to_owned()
    };
    let from = &q.from;

    // 0 means everyone is allowed to click
    if allowed_user_id != 0 && allowed_user_id != from.id.0 {
        bot.answer_callback_query(q.id).text(consts::NO).await?;
        return Ok(());
    };

    if data == "set" {
        set_command(bot, q.message.unwrap(), Some(from), &arg, true).await?;
        return Ok(());
    }

    let user = DB.lock().unwrap().fetch_user(from.id.0);

    if user.is_none() {
        bot.answer_callback_query(q.id)
            .text(consts::NOT_REGISTERED)
            .show_alert(true)
            .await?;
        return Ok(());
    }

    let user = user.unwrap();

    match data {
        "status" => {
            status_command(
                bot,
                q.message,
                q.inline_message_id,
                Some(from),
                true,
                arg.parse().unwrap_or(StatusType::Compact),
                true,
                user,
            )
            .await?;
        }
        "status_refresh" => {
            let res = status_command(
                bot.clone(),
                q.message,
                q.inline_message_id,
                Some(from),
                true,
                arg.parse().unwrap_or(StatusType::Compact),
                false,
                user,
            )
            .await;
            if res.is_err() {
                bot.answer_callback_query(q.id)
                    .text(consts::MESSAGE_UNMODIFIED)
                    .await?;
            }
        }
        "info" => {
            if user.api_type() == ApiType::Lastfm && q.message.is_some() {
                let msg = q.message.unwrap();
                let msg_text = msg.text().unwrap_or_default().to_string();
                let itatic_entity = utils::find_first_entity(&msg, MessageEntityKind::Italic);
                let bold_entity = utils::find_first_entity(&msg, MessageEntityKind::Bold);

                if itatic_entity.is_none() || bold_entity.is_none() {
                    bot.answer_callback_query(q.id)
                        .text(consts::NOT_FOUND)
                        .await?;
                    return Ok(());
                }

                let ita = itatic_entity.unwrap();
                let bol = bold_entity.unwrap();

                let artist =
                    utils::slice_tg_string(msg_text.clone(), ita.offset, ita.length + ita.offset);
                let title = utils::slice_tg_string(msg_text, bol.offset, bol.length + bol.offset);

                if artist.is_none() || title.is_none() {
                    bot.answer_callback_query(q.id)
                        .text(consts::NOT_FOUND)
                        .await?;
                    return Ok(());
                }

                let lastfm_username = user.account_username;

                let infos = fetch_lastfm_infos(lastfm_username, artist.unwrap(), title.unwrap())
                    .await
                    .unwrap_or(consts::NOT_FOUND.to_owned());
                bot.answer_callback_query(q.id)
                    .text(infos)
                    .show_alert(true)
                    .await?;
            } else {
                bot.answer_callback_query(q.id).text(consts::NO).await?;
            }
        }

        "collage" => {
            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                "🖼️",
                "0 loading",
            )]]);
            let media = InputMediaPhoto::new(InputFile::url(Url::parse(consts::URL_3X3).unwrap()))
                .caption(consts::LOADING);

            utils::send_or_edit_photo(
                bot.clone(),
                media,
                q.message.clone(),
                q.inline_message_id.as_ref(),
                true,
                Some(keyboard),
                false,
            )
            .await?;

            collage_command(
                bot,
                q.message,
                q.inline_message_id,
                from.into(),
                true,
                &arg,
                user,
            )
            .await?;
        }

        "random" => {
            random_command(
                bot,
                q.message,
                q.inline_message_id,
                from.into(),
                true,
                &arg,
                user,
            )
            .await?;
        }

        "user_settings" => {
            user_settings_command(
                bot,
                q.message,
                q.inline_message_id,
                from.into(),
                true,
                &arg,
                user,
            )
            .await?;
        }

        "loading" => {
            bot.answer_callback_query(q.id)
                .text(consts::LOADING)
                .await?;
        }

        _ => {
            bot.answer_callback_query(q.id).text(consts::NO).await?;
            log::error!("{data} unhandled");
        }
    }

    track(&format!("callback_{data}"), from.into()).await;

    Ok(())
}
