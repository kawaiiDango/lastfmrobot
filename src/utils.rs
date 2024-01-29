use std::{
    error::Error,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use teloxide::{
    adaptors::Throttle,
    payloads::{
        EditMessageMediaInlineSetters, EditMessageMediaSetters, EditMessageTextInlineSetters,
        EditMessageTextSetters, SendMessageSetters, SendPhotoSetters,
    },
    requests::Requester,
    types::{
        InlineKeyboardMarkup, InputFile, InputMedia, InputMediaPhoto, Message, MessageEntity,
        MessageEntityKind, ParseMode,
    },
};

use crate::{
    api_requester::{ApiType, EntryType, TimePeriod},
    config, db,
};

static TIMEAGO: Lazy<timeago::Formatter> = Lazy::new(timeago::Formatter::new);

pub fn replace_html_symbols(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn find_first_entity(msg: &Message, entity_kind: MessageEntityKind) -> Option<MessageEntity> {
    let entity = msg
        .entities()
        .unwrap_or_default()
        .iter()
        .find(|&e| e.kind == entity_kind);

    entity.cloned()
}

pub fn choose_the_from(
    msg: Option<&Message>,
    inline_from: Option<&teloxide::types::User>,
) -> teloxide::types::User {
    inline_from
        .unwrap_or_else(|| msg.as_ref().unwrap().from().unwrap())
        .clone()
}

pub fn human_readable_duration(ms: u64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let seconds_remaining = seconds % 60;
    format!("{}:{:02}", minutes, seconds_remaining)
}

pub fn name_with_link(tg_user: &teloxide::types::User, db_user: &db::User) -> String {
    let name = replace_html_symbols(&tg_user.first_name);
    if db_user.profile_shown {
        match db_user.api_type() {
            ApiType::Lastfm => format!(
                "<a href=\"https://www.last.fm/user/{}\">{}</a>",
                db_user.account_username, name
            ),
            ApiType::Librefm => format!(
                "<a href=\"https://www.libre.fm/user/{}\">{}</a>",
                db_user.account_username, name
            ),
            ApiType::Listenbrainz => format!(
                "<a href=\"https://listenbrainz.org/user/{}\">{}</a>",
                db_user.account_username, name
            ),
        }
    } else {
        name
    }
}

pub fn slice_tg_string(s: String, start: usize, end: usize) -> Option<String> {
    let mut utf16_len = 0;
    let mut start_byte = None;
    let mut end_byte = None;

    for (i, ch) in s.char_indices() {
        if utf16_len == start {
            start_byte = Some(i);
        }
        if utf16_len == end {
            end_byte = Some(i);
            break;
        }
        utf16_len += ch.len_utf16();
    }

    if start_byte.is_none() || end_byte.is_none() {
        return None;
    }

    Some(s[start_byte.unwrap()..end_byte.unwrap()].to_string())
}

pub async fn send_or_edit_message(
    bot: Throttle<teloxide::Bot>,
    text: &str,
    msg: Option<Message>,
    inline_message_id: Option<String>,
    edit: bool,
    keyboard: Option<InlineKeyboardMarkup>,
    disable_web_page_preview: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if msg.is_some() {
        let m = &msg.unwrap();
        if !edit {
            let mut x = bot
                .send_message(m.chat.id, text)
                .allow_sending_without_reply(true)
                .reply_to_message_id(m.id)
                .parse_mode(ParseMode::Html)
                .disable_web_page_preview(disable_web_page_preview);
            if keyboard.is_some() {
                x = x.reply_markup(keyboard.unwrap())
            }
            match x.await {
                Ok(_) => {
                    return Ok(());
                }
                Err(e) => {
                    if e.to_string().contains(
                        "Bad Request: not enough rights to send text messages to the chat",
                    ) {
                        bot.leave_chat(m.chat.id).await?;
                    } else if e.to_string().contains("Bad Request: can't parse entities:") {
                        log::error!("can't parse: {text}");
                    }
                    return Err(Box::new(e));
                }
            }
        } else {
            let mut x = bot
                .edit_message_text(m.chat.id, m.id, text)
                .parse_mode(ParseMode::Html)
                .disable_web_page_preview(disable_web_page_preview);
            if keyboard.is_some() {
                x = x.reply_markup(keyboard.unwrap())
            }
            x.await?;
        }
    } else if inline_message_id.is_some() && edit {
        let mut x = bot
            .edit_message_text_inline(inline_message_id.unwrap(), text)
            .parse_mode(ParseMode::Html)
            .disable_web_page_preview(disable_web_page_preview);
        if keyboard.is_some() {
            x = x.reply_markup(keyboard.unwrap())
        }
        x.await?;
    };

    Ok(())
}

pub async fn send_or_edit_photo(
    bot: Throttle<teloxide::Bot>,
    media: InputMediaPhoto,
    msg: Option<Message>,
    inline_message_id: Option<&String>,
    edit: bool,
    keyboard: Option<InlineKeyboardMarkup>,
    create_file_id: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if msg.is_some() {
        let m = &msg.unwrap();
        if !edit {
            let mut x = bot
                .send_photo(m.chat.id, media.media)
                .allow_sending_without_reply(true)
                .reply_to_message_id(m.id)
                .parse_mode(ParseMode::Html)
                .caption(media.caption.unwrap_or_default());
            if keyboard.is_some() {
                x = x.reply_markup(keyboard.unwrap())
            }
            match x.await {
                Ok(_) => {
                    return Ok(());
                }
                Err(e) => {
                    if e.to_string()
                        .contains("Bad Request: not enough rights to send photos to the chat")
                    {
                        bot.leave_chat(m.chat.id).await?;
                    }
                    return Err(Box::new(e));
                }
            }
        } else {
            let mut x = bot.edit_message_media(m.chat.id, m.id, InputMedia::Photo(media));
            if keyboard.is_some() {
                x = x.reply_markup(keyboard.unwrap())
            }
            x.await?;
        }
    } else if inline_message_id.is_some() && edit {
        // send the photo to the dump chat to get a file id.
        let new_media = if create_file_id {
            let dump_msg = bot
                .send_photo(config::INLINE_IMAGES_DUMP_CHAT_ID.to_string(), media.media)
                .await?;

            InputMediaPhoto::new(InputFile::file_id(
                dump_msg
                    .photo()
                    .unwrap()
                    .iter()
                    .last() // last is the largest
                    .unwrap()
                    .file
                    .id
                    .clone(),
            ))
            .caption(media.caption.unwrap_or_default())
            .parse_mode(ParseMode::Html)
        } else {
            media
        };

        let mut x =
            bot.edit_message_media_inline(inline_message_id.unwrap(), InputMedia::Photo(new_media));
        if keyboard.is_some() {
            x = x.reply_markup(keyboard.unwrap())
        }
        x.await?;
    }

    Ok(())
}

fn truncate_str(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

pub fn convert_to_timeago(seconds: u64) -> String {
    let current_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let duration = Duration::from_secs(current_time - seconds);

    TIMEAGO.convert(duration)
}

pub fn format_epoch_secs(seconds: u64, with_time: bool) -> String {
    let d = UNIX_EPOCH + Duration::from_secs(seconds);
    let datetime = DateTime::<Utc>::from(d);
    let fmt_str = if with_time {
        "%Y-%m-%d %H:%M:%S"
    } else {
        "%Y-%m-%d"
    };
    datetime.format(fmt_str).to_string()
}

// collage 3 1month
pub fn parse_collage_arg(arg: &str) -> (u32, TimePeriod, EntryType, bool) {
    let splits = arg.splitn(4, ' ').collect::<Vec<&str>>();

    let mut size = 3;
    let mut period = TimePeriod::AllTime;
    let mut no_text = false;
    let mut entry_type = EntryType::Album;

    let mut size_found = false;
    let mut period_found = false;
    let mut entry_type_found = false;

    if splits.contains(&"notext") || splits.contains(&"nonames") || splits.contains(&"clean") {
        no_text = true;
    }

    for split in splits {
        if !entry_type_found {
            entry_type_found = true;
            if split.starts_with("artist") {
                entry_type = EntryType::Artist;
            } else if split.starts_with("album") {
                entry_type = EntryType::Album;
            } else if split.starts_with("track") {
                entry_type = EntryType::Track;
            } else {
                entry_type_found = false;
            }

            if entry_type_found {
                continue;
            }
        }

        let fragment = truncate_str(split, 4);

        if !size_found {
            // parse nxn or just n

            let fragment_splits = fragment.splitn(2, 'x').collect::<Vec<&str>>();

            let parsed = fragment_splits.first().unwrap().parse::<u32>();
            if parsed.is_ok() {
                let s = parsed.ok().unwrap_or_default();

                if s > 0 && s <= 7 {
                    size = s;
                    size_found = true;
                    continue;
                }
            }
        }

        if !period_found {
            let is_day = fragment.contains('d');
            let is_week = fragment.contains('w');
            let is_month = fragment.contains('m');
            let is_year = fragment.contains('y');
            let is_all = fragment.contains('o') || fragment.contains("all");

            let first_digit = &split.get(0..1).unwrap_or_default().parse::<u32>();

            if first_digit.as_ref().is_ok() {
                let first_digit_u = first_digit.clone().ok().unwrap_or_default();
                if is_day && first_digit_u == 7 || is_week && first_digit_u == 1 {
                    period = TimePeriod::OneWeek;
                    period_found = true;
                }

                if is_month && first_digit_u == 1 || first_digit_u == 3 || first_digit_u == 6 {
                    match first_digit_u {
                        1 => period = TimePeriod::OneMonth,
                        3 => period = TimePeriod::ThreeMonths,
                        6 => period = TimePeriod::SixMonths,
                        _ => {}
                    }
                    period_found = true;
                }
                if is_year && first_digit_u == 1 {
                    period = TimePeriod::OneYear;
                    period_found = true;
                }
            } else if is_week || is_month || is_year || is_all {
                period = if is_week {
                    TimePeriod::OneWeek
                } else if is_month {
                    TimePeriod::OneMonth
                } else if is_year {
                    TimePeriod::OneYear
                } else {
                    TimePeriod::AllTime
                };

                period_found = true;
            }
        }
    }

    (size, period, entry_type, no_text)
}
