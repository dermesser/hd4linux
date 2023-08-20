//! HiDrive access is mediated through the structs in this module.
//!
//! Everywhere you see a `P` type parameter, URL parameters are expected. An easy way to supply
//! them is the `Params` type. You can use other types, though, as long as they serialize to a list
//! of pairs, such as `&[(T0, T1)]` or `BTreeMap<T0, T1>`.
//!

use crate::http::Client;
use crate::oauth2;
use crate::types::*;

use std::collections::LinkedList;
use std::fmt::{Display, Formatter};

use anyhow::{self, Error, Result};
use futures_util::StreamExt;
use reqwest;
use serde::ser::SerializeSeq;
use serde_json;
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub const NO_BODY: Option<reqwest::Body> = None;
/// Use this if you don't want to supply options to a method. This prevents type errors due to
/// unknown inner type of Option.
pub const NO_PARAMS: Option<&Params> = None;

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
#[derive(Default)]
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

    pub fn add<S: AsRef<str>>(&mut self, k: String, v: ParamValue) {
        self.p.push_back(Param { name: k, val: v })
    }

    pub fn add_str<S1: AsRef<str>, S2: AsRef<str>>(&mut self, k: S1, v: S2) {
        self.p.push_back(Param {
            name: k.as_ref().into(),
            val: ParamValue::String(v.as_ref().into()),
        })
    }
    pub fn add_bool<S: AsRef<str>>(&mut self, k: S, v: bool) {
        self.p.push_back(Param {
            name: k.as_ref().into(),
            val: ParamValue::Bool(v),
        })
    }
    pub fn add_int<S: AsRef<str>>(&mut self, k: S, v: isize) {
        self.p.push_back(Param {
            name: k.as_ref().into(),
            val: ParamValue::Int(v),
        })
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

const DEFAULT_API_BASE_URL: &str = "https://api.hidrive.strato.com/2.1";

pub struct HiDrive {
    client: Client,
    base_url: String,
}

impl HiDrive {
    pub fn new(c: reqwest::Client, a: oauth2::Authorizer) -> HiDrive {
        HiDrive {
            client: Client::new(c, a),
            base_url: DEFAULT_API_BASE_URL.into(),
        }
    }

    pub fn user(&mut self) -> HiDriveUser<'_> {
        HiDriveUser { hd: self }
    }

    pub fn permissions(&mut self) -> HiDrivePermission<'_> {
        HiDrivePermission { hd: self }
    }

    pub fn files(&mut self) -> HiDriveFiles<'_> {
        HiDriveFiles { hd: self }
    }
}

/// Interact with user information.
pub struct HiDriveUser<'a> {
    hd: &'a mut HiDrive,
}

/// The /user/ API.
///
/// This will be extended in future to allow for administration. For now, it only contains
/// bare-bones features.
impl<'a> HiDriveUser<'a> {
    pub async fn me<P: serde::Serialize + ?Sized>(&mut self, params: Option<&P>) -> Result<User> {
        let u = format!("{}/user/me", self.hd.base_url);
        self.hd
            .client
            .gen_call(reqwest::Method::GET, u, &Params::new(), params, NO_BODY)
            .await
    }
}

/// Interact with object permissions.
pub struct HiDrivePermission<'a> {
    hd: &'a mut HiDrive,
}

impl<'a> HiDrivePermission<'a> {
    /// GET /2.1/permission
    ///
    /// Optional parameters: `pid, account, fields`.
    pub async fn get_permission<S: AsRef<str>, P: serde::Serialize + ?Sized>(
        &mut self,
        path: &S,
        p: Option<&P>,
    ) -> Result<Permissions> {
        let u = format!("{}/permission", self.hd.base_url);
        let rqp = &[("path", path.as_ref().to_string())];
        self.hd
            .client
            .gen_call(reqwest::Method::GET, u, &rqp, p, NO_BODY)
            .await
    }

    /// PUT /2.1/permission
    ///
    /// Optional parameters: `pid, account, invite_id, readable, writable` for P.
    pub async fn set_permission<S: AsRef<str>, P: serde::Serialize + ?Sized>(
        &mut self,
        path: &S,
        p: Option<&P>,
    ) -> Result<Permissions> {
        let u = format!("{}/permission", self.hd.base_url);
        let rqp = &[("path", path.as_ref().to_string())];
        self.hd
            .client
            .gen_call(reqwest::Method::PUT, u, &rqp, p, NO_BODY)
            .await
    }
}

/// Interact with files.
pub struct HiDriveFiles<'a> {
    hd: &'a mut HiDrive,
}

/// A wrapped callback for writing an HTTP response body to a file.
async fn write_response_to_file<D: AsyncWrite + Unpin>(
    rp: reqwest::Response,
    mut d: D,
) -> Result<usize> {
    if rp.status().is_success() {
        let mut stream = rp.bytes_stream();
        let mut i = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            d.write_all(chunk.as_ref()).await?;
            i += chunk.len();
        }
        Ok(i)
    } else {
        let body = rp.text().await?;
        let e: ApiError = serde_json::from_reader(body.as_bytes())?;
        Err(Error::new(e))
    }
}

impl<'a> HiDriveFiles<'a> {
    /// Download file.
    pub async fn get<P: serde::Serialize + ?Sized, D: AsyncWrite + Unpin>(
        &mut self,
        out: D,
        p: Option<&P>,
    ) -> Result<usize> {
        let cb = move |rp: reqwest::Response| write_response_to_file(rp, out);
        let u = format!("{}/file", self.hd.base_url);
        self.hd
            .client
            .gen_call_cb(reqwest::Method::GET, u, &Params::new(), p, NO_BODY, cb)
            .await
    }

    /*pub async fn upload_no_overwrite<P: serde::Serialize + ?Sized>(&mut self, p: Option<&P>) -> Result<Item> {

    }*/

    /// Return metadata for directory.
    ///
    /// Specify either `pid` or `path`, or the request will fail.
    ///
    /// Further parameters: `members, limit, snapshot, snaptime, fields, sort`.
    pub async fn get_dir<P: serde::Serialize + ?Sized>(&mut self, p: Option<&P>) -> Result<Item> {
        let u = format!("{}/dir", self.hd.base_url);
        self.hd
            .client
            .gen_call(reqwest::Method::GET, u, &Params::new(), p, NO_BODY)
            .await
    }

    /// Return metadata for home directory.
    ///
    /// Further parameters: `members, limit, snapshot, snaptime, fields, sort`.
    pub async fn get_home_dir<P: serde::Serialize + ?Sized>(
        &mut self,
        p: Option<&P>,
    ) -> Result<Item> {
        let u = format!("{}/dir/home", self.hd.base_url);
        self.hd
            .client
            .gen_call(reqwest::Method::GET, u, &Params::new(), p, NO_BODY)
            .await
    }

    /// Create directory.
    ///
    /// Further parameters: `pid, on_exist, mtime, parent_mtime`.
    pub async fn mkdir<P: serde::Serialize + ?Sized, S: AsRef<str>>(
        &mut self,
        path: &S,
        p: Option<&P>,
    ) -> Result<Item> {
        let u = format!("{}/dir", self.hd.base_url);
        let mut rp = Params::new();
        rp.add_str("path", path);
        self.hd
            .client
            .gen_call(reqwest::Method::POST, u, &rp, p, NO_BODY)
            .await
    }

    /// Remove directory.
    ///
    /// Further parameters: `path, pid, recursive, parent_mtime`.
    pub async fn rmdir<P: serde::Serialize + ?Sized>(&mut self, p: Option<&P>) -> Result<Item> {
        let u = format!("{}/dir", self.hd.base_url);
        self.hd
            .client
            .gen_call(reqwest::Method::DELETE, u, &p, NO_PARAMS, NO_BODY)
            .await
    }

    /// Copy directory.
    ///
    /// Further parameters: `src, src_id, dst_id, on_exist, snapshot, snaptime, dst_parent_mtime,
    /// preserve_mtime`.
    pub async fn cpdir<P: serde::Serialize + ?Sized, S: AsRef<str>>(
        &mut self,
        dst: &S,
        p: Option<&P>,
    ) -> Result<Item> {
        let u = format!("{}/dir/copy", self.hd.base_url);
        let mut rp = Params::new();
        rp.add_str("dst", dst);
        self.hd
            .client
            .gen_call(reqwest::Method::POST, u, &rp, p, NO_BODY)
            .await
    }

    /// Move directory.
    ///
    /// Further parameters: `src, src_id, dst_id, on_exist, src_parent_mtime, dst_parent_mtime,
    /// preserve_mtime`.
    pub async fn mvdir<P: serde::Serialize + ?Sized, S: AsRef<str>>(
        &mut self,
        dst: &S,
        p: Option<&P>,
    ) -> Result<Item> {
        let u = format!("{}/dir/move", self.hd.base_url);
        let mut rp = Params::new();
        rp.add_str("dst", dst);
        self.hd
            .client
            .gen_call(reqwest::Method::POST, u, &rp, p, NO_BODY)
            .await
    }

    /// Rename directory.
    ///
    /// Takes the new name as required parameter. Useful parameters: `path, pid, on_exist =
    /// {autoname, overwrite}, parent_mtime (int)'.
    pub async fn renamedir<P: serde::Serialize + ?Sized, S: AsRef<str>>(
        &mut self,
        name: &S,
        p: Option<&P>,
    ) -> Result<Item> {
        let u = format!("{}/dir/rename", self.hd.base_url);
        let mut rp = Params::new();
        rp.add_str("name", name);
        self.hd
            .client
            .gen_call(reqwest::Method::POST, u, &rp, p, NO_BODY)
            .await
    }

    /// Get file or directory hash.
    ///
    /// Parameters: `path, pid` (specifying either is mandatory).
    ///
    /// Get hash for given level and ranges. If ranges is empty, return hashes for entire file (but
    /// at most 256).
    pub async fn hash<P: serde::Serialize + ?Sized>(
        &mut self,
        level: isize,
        ranges: &[(usize, usize)],
        p: Option<&P>,
    ) -> Result<FileHash> {
        let u = format!("{}/file/hash", self.hd.base_url);
        let mut rqp = Params::new();
        rqp.add_int("level", level);
        if ranges.is_empty() {
            rqp.add_str("ranges", "-");
        } else {
            let r = ranges
                .iter()
                .map(|(a, b)| format!("{}-{}", a, b))
                .fold(String::new(), |s, e| (s + ",") + &e);
            rqp.add_str("ranges", &r[1..]);
        }
        self.hd
            .client
            .gen_call(reqwest::Method::GET, u, &rqp, p, NO_BODY)
            .await
    }

    /// Rename operation.
    ///
    /// Takes the new name as required parameter. Useful parameters: `path, pid, on_exist =
    /// {autoname, overwrite}, parent_mtime (int)'.
    pub async fn rename<P: serde::Serialize + ?Sized, S: AsRef<str>>(
        &mut self,
        name: &S,
        p: Option<&P>,
    ) -> Result<Item> {
        let u = format!("{}/file/rename", self.hd.base_url);
        let mut rp = Params::new();
        rp.add_str("name", name);
        self.hd
            .client
            .gen_call(reqwest::Method::GET, u, &rp, p, NO_BODY)
            .await
    }
}
