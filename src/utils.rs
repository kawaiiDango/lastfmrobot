use std::{
    error::Error,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use once_cell::sync::Lazy;
use teloxide::{
    adaptors::Throttle,
    payloads::{
        EditMessageMediaInlineSetters, EditMessageMediaSetters, EditMessageTextInlineSetters,
        EditMessageTextSetters, SendMessageSetters, SendPhotoSetters,
    },
    requests::Requester,
    types::{
        InlineKeyboardMarkup, InputMedia, InputMediaPhoto, Message, MessageEntity,
        MessageEntityKind, ParseMode,
    },
};

use crate::{
    api_requester::{ApiType, EntryType, TimePeriod},
    db,
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

pub fn slice_unicode_string(text: String, start_inclusive: usize, end_exclusive: usize) -> String {
    let mut char_indices = text.char_indices();
    let start_byte = match char_indices.nth(start_inclusive) {
        Some((byte_index, _)) => byte_index,
        None => text.len(),
    };

    let end_byte = match char_indices.nth(end_exclusive - start_inclusive - 1) {
        Some((byte_index, _)) => byte_index,
        None => text.len(),
    };

    text[start_byte..end_byte].to_string()
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
            x.await?;
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
            x.await?;
        } else {
            let mut x = bot.edit_message_media(m.chat.id, m.id, InputMedia::Photo(media));
            if keyboard.is_some() {
                x = x.reply_markup(keyboard.unwrap())
            }
            x.await?;
        }
    } else if inline_message_id.is_some() && edit {
        let mut x =
            bot.edit_message_media_inline(inline_message_id.unwrap(), InputMedia::Photo(media));
        if keyboard.is_some() {
            x = x.reply_markup(keyboard.unwrap())
        }
        x.await?;
    };

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

// collage 3 1month
pub fn parse_collage_arg(arg: &str) -> (u32, TimePeriod, EntryType, bool) {
    let splits = arg.splitn(3, ' ').collect::<Vec<&str>>();

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
        }

        let fragment = truncate_str(split, 4);

        if !size_found {
            let parsed = fragment.parse::<u32>();
            if parsed.is_ok() {
                let s = parsed.ok().unwrap_or_default();

                if s > 0 && s <= 7 {
                    size = s;
                    size_found = true;
                }
            }
        }
        if !period_found {
            let is_day = fragment.contains('d');
            let is_week = fragment.contains('w');
            let is_month = fragment.contains('m');
            let is_year = fragment.contains('y');
            let is_all = fragment.contains('o') || fragment.contains("all");

            let first_digit = &split.get(0..1).unwrap_or_default().parse::<i32>();

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