//! HiDrive access is mediated through the structs in this module.
//!
//! Everywhere you see a `P` type parameter, URL parameters are expected. An easy way to supply
//! them is the `Params` type. You can use other types, though, as long as they serialize to a list
//! of pairs, such as `&[(T0, T1)]` or `BTreeMap<T0, T1>`.
//!

use crate::http::Client;
use crate::oauth2;
use crate::types::*;

use anyhow::{self, Result};
use reqwest;
use tokio::io::{AsyncRead, AsyncWrite};

pub const NO_BODY: Option<reqwest::Body> = None;
/// Use this if you don't want to supply options to a method. This prevents type errors due to
/// unknown inner type of Option.
pub const NO_PARAMS: Option<&Params> = None;

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
            .request(reqwest::Method::GET, u, &Params::new(), params)
            .await?
            .go()
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
            .request(reqwest::Method::GET, u, &rqp, p)
            .await?
            .go()
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
            .request(reqwest::Method::PUT, u, &rqp, p)
            .await?
            .go()
            .await
    }
}

/// Interact with files.
pub struct HiDriveFiles<'a> {
    hd: &'a mut HiDrive,
}

impl<'a> HiDriveFiles<'a> {
    /// Download file.
    pub async fn get<P: serde::Serialize + ?Sized, D: AsyncWrite + Unpin>(
        &mut self,
        out: D,
        p: Option<&P>,
    ) -> Result<usize> {
        let u = format!("{}/file", self.hd.base_url);
        self.hd
            .client
            .request(reqwest::Method::GET, u, &Params::new(), p)
            .await?
            .download_file(out)
            .await
    }

    /// Upload a file (max. 2 gigabytes). Specify either `dir_id`, `dir`, or both; in the latter
    /// case, `dir` is relative to `dir_id`.
    ///
    /// Parameter `name` specifies the file name to be acted on.
    ///
    /// File will not be overwritten if it exists (in that case, code 409 is returned).
    pub async fn upload_no_overwrite<P: serde::Serialize + ?Sized, R: Into<reqwest::Body>>(
        &mut self,
        src: R,
        p: Option<&P>,
    ) -> Result<Item> {
        let u = format!("{}/file", self.hd.base_url);
        self.hd
            .client
            .request(reqwest::Method::POST, u, &Params::new(), p)
            .await?
            .set_attachment(src)
            .go()
            .await
    }

    /// Return metadata for directory.
    ///
    /// Specify either `pid` or `path`, or the request will fail.
    ///
    /// Further parameters: `members, limit, snapshot, snaptime, fields, sort`.
    pub async fn get_dir<P: serde::Serialize + ?Sized>(&mut self, p: Option<&P>) -> Result<Item> {
        let u = format!("{}/dir", self.hd.base_url);
        self.hd
            .client
            .request(reqwest::Method::GET, u, &Params::new(), p)
            .await?
            .go()
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
            .request(reqwest::Method::GET, u, &Params::new(), p)
            .await?
            .go()
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
            .request(reqwest::Method::POST, u, &rp, p)
            .await?
            .go()
            .await
    }

    /// Remove directory.
    ///
    /// Further parameters: `path, pid, recursive, parent_mtime`.
    pub async fn rmdir<P: serde::Serialize + ?Sized>(&mut self, p: Option<&P>) -> Result<Item> {
        let u = format!("{}/dir", self.hd.base_url);
        self.hd
            .client
            .request(reqwest::Method::DELETE, u, &p, NO_PARAMS)
            .await?
            .go()
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
            .request(reqwest::Method::POST, u, &rp, p)
            .await?
            .go()
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
            .request(reqwest::Method::POST, u, &rp, p)
            .await?
            .go()
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
            .request(reqwest::Method::POST, u, &rp, p)
            .await?
            .go()
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
            .request(reqwest::Method::GET, u, &rqp, p)
            .await?
            .go()
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
            .request(reqwest::Method::GET, u, &rp, p)
            .await?
            .go()
            .await
    }
}
