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
    account: String,
    readable: bool,
    writable: bool,
    path: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Protocols {
    ftp: bool,
    rsync: bool,
    webdav: bool,
    scp: bool,
    cifs: bool,
    git: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    path: String,
    name: Option<String>,
    size: Option<usize>,
    #[serde(rename = "type")]
    typ: Option<String>,

    id: Option<String>,
    parent_id: Option<String>,

    has_dirs: Option<bool>,
    nmembers: Option<usize>,
    members: Vec<Item>,

    #[serde(with = "time::serde::timestamp::option")]
    ctime: Option<OffsetDateTime>,
    #[serde(with = "time::serde::timestamp::option")]
    mtime: Option<OffsetDateTime>,

    chash: Option<Hash>,
    mhash: Option<Hash>,
    nhash: Option<Hash>,
    mohash: Option<Hash>,

    readable: Option<bool>,
    writable: Option<bool>,
    shareable: Option<bool>,
    teamfolder: Option<bool>,

    rshare: Option<Share>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Share {
    name: Option<String>,
    path: Option<String>,
    id: Option<String>,
    pid: Option<String>,
    size: Option<usize>,
    status: Option<String>,

    viewmode: Option<String>,
    share_type: Option<String>,
    file_type: Option<String>,

    #[serde(with = "time::serde::timestamp::option")]
    created: Option<OffsetDateTime>,
    #[serde(with = "time::serde::timestamp::option")]
    last_modified: Option<OffsetDateTime>,
    #[serde(with = "time::serde::timestamp::option")]
    valid_until: Option<OffsetDateTime>,
    ttl: Option<usize>,

    // Only included if set.
    password: Option<bool>,
    has_password: Option<bool>,
    is_encrypted: Option<bool>,

    uri: Option<String>,
    count: Option<usize>,
    // Only included if set.
    maxcount: Option<usize>,
    remaining: Option<usize>,

    readable: Option<bool>,
    writable: Option<bool>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
    account: String,
    encrypted: bool,
    descr: String,
    is_owner: bool,
    email: String,
    email_verified: bool,
    language: String,
    protocols: Protocols,
    is_admin: bool,
    alias: String,
    home: String,
    home_id: String,
    folder: Item,
}
