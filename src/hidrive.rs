use crate::oauth2;
use crate::types::*;

use std::collections::LinkedList;
use std::fmt::{Display, Formatter};

use anyhow::{self, Context, Error, Result};
use log::{error, info, warn};
use reqwest;
use serde::{ser::SerializeSeq, Deserialize, Serialize};
use serde_json;

pub enum ParamValue {
    String(String),
    Bool(bool),
    Int(isize),
}

impl Display for ParamValue {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        match self {
            &ParamValue::String(ref s) => s.fmt(f),
            &ParamValue::Bool(b) => b.fmt(f),
            &ParamValue::Int(u) => u.fmt(f),
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

pub struct Params {
    p: LinkedList<Param>,
}

impl serde::Serialize for Params {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut ss = s.serialize_seq(Some(self.p.len()))?;
        for p in self.p.iter() {
            ss.serialize_element(&(&p.name, &format!("{}", p.val)))?;
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

    pub fn add(&mut self, k: String, v: ParamValue) {
        self.p.push_back(Param { name: k, val: v })
    }

    pub fn add_str(&mut self, k: String, v: String) {
        self.p.push_back(Param {
            name: k,
            val: ParamValue::String(v),
        })
    }
    pub fn add_bool(&mut self, k: String, v: bool) {
        self.p.push_back(Param {
            name: k,
            val: ParamValue::Bool(v),
        })
    }
    pub fn add_int(&mut self, k: String, v: isize) {
        self.p.push_back(Param {
            name: k,
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

const DEFAULT_API_BASE_URL: &'static str = "https://api.hidrive.strato.com/2.1";

pub struct HiDrive {
    client: reqwest::Client,
    authz: oauth2::Authorizer,
    base_url: String,
}

impl HiDrive {
    pub fn new(c: reqwest::Client, a: oauth2::Authorizer) -> HiDrive {
        HiDrive {
            client: c,
            authz: a,
            base_url: DEFAULT_API_BASE_URL.into(),
        }
    }

    pub fn user<'a>(&'a mut self) -> HiDriveUser<'a> {
        HiDriveUser { hd: self }
    }

    async fn new_request<U: reqwest::IntoUrl>(
        &mut self,
        method: reqwest::Method,
        url: U,
    ) -> Result<reqwest::RequestBuilder> {
        self.authz
            .authorize(self.client.request(method, url))
            .await
            .context("HiDrive::new_request: Building authorized RequestBuilder")
    }
}

pub struct HiDriveUser<'a> {
    hd: &'a mut HiDrive,
}

impl<'a> HiDriveUser<'a> {
    pub async fn me<P: serde::Serialize + ?Sized>(&mut self, params: Option<&P>) -> Result<User> {
        let u = format!("{}/user/me", self.hd.base_url);
        let rqb = self.hd.new_request(reqwest::Method::GET, u).await?;
        let rp = if let Some(params) = params {
            let rqb = rqb.query(params);
            info!(target: "hd_api", "Sending HTTP request: {:?}", rqb);
            rqb.send().await?
        } else {
            rqb.send().await?
        };

        let status = rp.status();
        info!(target: "hd_api", "Received HTTP response: {:?}", rp);
        let body = rp.text().await?;
        info!(target: "hd_api", "Received HTTP response body: {}", body);
        if status.is_success() {
            Ok(serde_json::from_str(body.as_str())?)
        } else {
            let e: ApiError = serde_json::from_str(body.as_str())?;
            Err(Error::msg(format!("Error from API: {:?}", e)))
        }
    }
}
