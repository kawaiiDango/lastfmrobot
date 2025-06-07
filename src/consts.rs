pub const ERR_MSG: &str = "Oopsie doopsie, I did a fucky wucky!";
pub const BOT_STARTED: &str = "I just woke up from the dead.";
pub const NOT_REGISTERED: &str =
    "You are not registered yet. Use /set [username] to link your uwusername.";
pub const NOT_REGISTERED_INLINE: &str = "Link your account";
pub const WELCOME_TEXT: &str = "Welcome to LastFM Robot. Use /set [username] to set your uwusername.\n
Check out <a href=\"https://kawaiidango.github.io/pano-scrobbler\">Pano Scrobbler</a> to set up scrobbling.\n
Some commands work without a /";
pub const LASTFM_STAR_URL: &str =
    "https://lastfm.freetls.fastly.net/i/u/300x300/2a96cbd8b46e442fc41c2b86b821562f.png";
pub const BOTS_MUSIC: &str = "We bots don't listen to music, baaaaaka.";
pub const NOT_FOUND: &str = "Not found.";
pub const USER_NOT_FOUND: &str = "No such uwuser";
pub const PRIVATE_PROFILE: &str = "Your scrobbles are hidden. To use this bot, disable that at https://www.last.fm/settings/privacy";
pub const NO_SCROBBLES: &str = "No scrwobbles fownd!";
pub const UNSET: &str = "Your uwusername has been unlinked from the bot.";
pub const NO: &str = "Nuuuuuuuuuu!";
pub const THEY_NOT_REGISTERED: &str = "They need to /set their uwusername with me.";
pub const COMPAT_CLICK: &str =
    "Usage: compat 1y. Reply to someone's message in a group, with this command.";
pub const COLLAGE_USAGE: &str = "Direct usage: <b>collage 3 1m, /collage clean 4 alltime</b> etc.";
pub const TOP_USAGE: &str = "Direct usage: <b>/topkek artists 1m , /topkek tracks alltime</b>";
pub const RANDOM_USAGE: &str = "Direct usage: <b>/random artists 1m , /random tracks alltime</b>";
pub const COLLAGE_LIBREFM: &str = "Collages aren't available for Librefm.";
pub const SET_CLICK: &str = "usage: <b>/set username</b> to set your username for lastfm\n<b>/set username listenbrainz</b> to set your username for listenbrainz";
pub const ANON_KUN: &str = "Hieee anon kun";
pub const ITS_ME: &str = "Lookie, its me!!!";
pub const LOADING: &str = "lOwOding...";
pub const MESSAGE_UNMODIFIED: &str = "No updates from your profile";
pub const MESSAGE_TOO_OLD: &str = "This message is too old and can't be edited";
pub const PRIVACY_POLICY: &str = r#"The bot, LastFM Robot stores a mapping of the user's Telegram ID, 
to their scrobbling service (Lastfm, Librefm or ListenBrainz) username and the user's bot preferences.

This information is used to fetch and display the user's scrobble information and for overall analytics.

The user may choose to delete this information and unlink themselves from the bot, by clicking on Unlink on the /preferences command."#;
