use rusqlite::{params, Connection, Result};

use crate::api_requester::ApiType;

#[derive(Clone, Debug)]
pub struct User {
    pub tg_user_id: u64,
    pub account_username: String,
    api_type: String,
    pub profile_shown: bool,
}

impl User {
    pub fn new(
        tg_user_id: u64,
        account_username: String,
        api_type: &ApiType,
        profile_shown: bool,
    ) -> User {
        User {
            tg_user_id,
            account_username,
            api_type: api_type.to_string(),
            profile_shown,
        }
    }

    pub fn api_type(&self) -> ApiType {
        self.api_type.parse().unwrap_or(ApiType::Lastfm)
    }
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn new() -> Db {
        let conn = Connection::open("users.sqlite").unwrap();
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS users (
            tg_user_id              INTEGER PRIMARY KEY,
            account_username        TEXT NOT NULL,
            api_type                TEXT NOT NULL,
            profile_shown           INTEGER NOT NULL DEFAULT 0
            )",
            (),
        );

        Db { conn }
    }

    pub fn fetch_user(&self, tg_user_id: u64) -> Option<User> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM users WHERE tg_user_id = ?1 LIMIT 1")
            .unwrap();
        let user = stmt
            .query_map([tg_user_id], |row| {
                Ok(User {
                    tg_user_id: row.get(0)?,
                    account_username: row.get(1)?,
                    api_type: row.get(2)?,
                    profile_shown: row.get(3)?,
                })
            })
            .unwrap()
            .next()
            .map(|x| x.unwrap());

        user
    }

    pub fn upsert_user(&self, user: &User) -> Result<usize> {
        self.conn.execute("INSERT INTO users (tg_user_id, account_username, api_type, profile_shown) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (tg_user_id) DO UPDATE SET account_username = ?2, api_type = ?3, profile_shown = ?4",
         params![user.tg_user_id, user.account_username, user.api_type, user.profile_shown])
    }

    pub fn delete_user(&self, tg_user_id: u64) -> Result<usize> {
        self.conn
            .execute("DELETE FROM users WHERE tg_user_id = ?1", [tg_user_id])
    }
}
