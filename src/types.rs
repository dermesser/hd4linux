use crate::hashing::Hash;

use std::collections::LinkedList;
use std::fmt::{self, Display, Formatter};

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Clone, PartialEq)]
pub enum ParamValue {
    String(String),
    Bool(bool),
    Int(isize),
}

impl Display for ParamValue {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        match self {
            ParamValue::String(ref s) => s.fmt(f),
            ParamValue::Bool(b) => b.fmt(f),
            ParamValue::Int(u) => u.fmt(f),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct Param {
    name: String,
    val: ParamValue,
}

impl Display for Param {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        self.name.fmt(f)?;
        f.write_str("=")?;
        self.val.fmt(f)
    }
}

/// Use Params to supply optional query parameters to API calls. This implements the required trait
/// of `P` parameters in API methods. Alternatively, you can use constructs like `&[("key",
/// "value")]`.
#[derive(Default, Clone)]
pub struct Params {
    p: LinkedList<Param>,
}

impl serde::Serialize for Params {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut ss = s.serialize_seq(Some(self.p.len()))?;
        for p in self.p.iter() {
            ss.serialize_element(&(&p.name, p.val.to_string()))?;
        }
        ss.end()
    }
}

impl Params {
    pub fn new() -> Params {
        Params {
            p: LinkedList::<Param>::new(),
        }
    }

    pub fn add(&mut self, k: String, v: ParamValue) -> &mut Self {
        self.p.push_back(Param { name: k, val: v });
        self
    }

    pub fn add_str<S1: AsRef<str>, S2: AsRef<str>>(&mut self, k: S1, v: S2) -> &mut Self {
        self.p.push_back(Param {
            name: k.as_ref().into(),
            val: ParamValue::String(v.as_ref().into()),
        });
        self
    }
    pub fn add_bool<S: AsRef<str>>(&mut self, k: S, v: bool) -> &mut Self {
        self.p.push_back(Param {
            name: k.as_ref().into(),
            val: ParamValue::Bool(v),
        });
        self
    }
    pub fn add_int<S: AsRef<str>>(&mut self, k: S, v: isize) -> &mut Self {
        self.p.push_back(Param {
            name: k.as_ref().into(),
            val: ParamValue::Int(v),
        });
        self
    }
}

impl Display for Params {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        f.write_str("?")?;
        let mut first = true;
        for p in self.p.iter() {
            if !first {
                f.write_str("&")?;
            }
            first = false;
            p.fmt(f)?;
        }
        Ok(())
    }
}
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

/// An identifier of a file or directory.
#[derive(Debug, Clone)]
pub enum Identifier {
    /// A file or directory ID.
    Id(String),
    /// A path.
    Path(String),
    /// A `path` relative to a directory `id`.
    Relative { id: String, path: String },
}

impl Identifier {
    pub fn to_params<S: AsRef<str>>(&self, p: &mut Params, id_parameter: S, path_parameter: S) {
        match self {
            Identifier::Id(ref s) => p.add_str(id_parameter.as_ref(), s),
            Identifier::Path(ref s) => p.add_str(path_parameter.as_ref(), s),
            Identifier::Relative { ref id, ref path } => p
                .add_str(id_parameter.as_ref(), id)
                .add_str(path_parameter.as_ref(), path),
        };
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

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Url {
    pub url: String,
}
