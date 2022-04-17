use crate::hashing::Hash;

use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiError {
    pub msg: String,
    pub code: usize,
    pub auth: Option<String>,
}

impl Display for ApiError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if self.auth.is_none() {
            f.write_fmt(format_args!("ApiError {}: {}", self.code, self.msg))
        } else {
            f.write_fmt(format_args!(
                "ApiError {}: {} {}",
                self.code,
                self.msg,
                self.auth.as_ref().unwrap()
            ))
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HashedBlock {
    pub hash: Hash,
    pub level: usize,
    pub block: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileHash {
    pub level: usize,
    pub chash: Hash,
    pub list: Vec<Vec<HashedBlock>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Permissions {
    pub account: String,
    pub readable: bool,
    pub writable: bool,
    pub path: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Protocols {
    pub ftp: bool,
    pub rsync: bool,
    pub webdav: bool,
    pub scp: bool,
    pub cifs: bool,
    pub git: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub path: String,
    pub name: Option<String>,
    pub size: Option<usize>,
    #[serde(rename = "type")]
    pub typ: Option<String>,

    pub id: Option<String>,
    pub parent_id: Option<String>,

    pub has_dirs: Option<bool>,
    pub nmembers: Option<usize>,
    pub members: Vec<Item>,

    #[serde(with = "time::serde::timestamp::option")]
    pub ctime: Option<OffsetDateTime>,
    #[serde(with = "time::serde::timestamp::option")]
    pub mtime: Option<OffsetDateTime>,

    pub chash: Option<Hash>,
    pub mhash: Option<Hash>,
    pub nhash: Option<Hash>,
    pub mohash: Option<Hash>,

    pub readable: Option<bool>,
    pub writable: Option<bool>,
    pub shareable: Option<bool>,
    pub teamfolder: Option<bool>,

    pub rshare: Option<Share>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Share {
    pub name: Option<String>,
    pub path: Option<String>,
    pub id: Option<String>,
    pub pid: Option<String>,
    pub size: Option<usize>,
    pub status: Option<String>,

    pub viewmode: Option<String>,
    pub share_type: Option<String>,
    pub file_type: Option<String>,

    #[serde(with = "time::serde::timestamp::option")]
    pub created: Option<OffsetDateTime>,
    #[serde(with = "time::serde::timestamp::option")]
    pub last_modified: Option<OffsetDateTime>,
    #[serde(with = "time::serde::timestamp::option")]
    pub valid_until: Option<OffsetDateTime>,
    pub ttl: Option<usize>,

    // Only included if set.
    pub password: Option<bool>,
    pub has_password: Option<bool>,
    pub is_encrypted: Option<bool>,

    pub uri: Option<String>,
    pub count: Option<usize>,
    // Only included if set.
    pub maxcount: Option<usize>,
    pub remaining: Option<usize>,

    pub readable: Option<bool>,
    pub writable: Option<bool>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
    pub account: String,
    pub encrypted: bool,
    pub descr: String,
    pub is_owner: bool,
    pub email: String,
    pub email_verified: bool,
    pub language: String,
    pub protocols: Protocols,
    pub is_admin: bool,
    pub alias: String,
    pub home: String,
    pub home_id: String,
    pub folder: Item,
}
