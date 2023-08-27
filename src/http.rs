use anyhow::{Context, Error, Result};
use futures_util::StreamExt;
use log::{info, warn};
use reqwest::header::{HeaderName, HeaderValue};
use reqwest::RequestBuilder;
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::oauth2::Authorizer;
use crate::types::*;

/// This is a callback for gen_call_cb, deserializing the response to JSON.
async fn read_body_to_json<RT: DeserializeOwned + ?Sized>(rp: reqwest::Response) -> Result<RT> {
    let status = rp.status();
    if status.is_success() {
        let body = rp.text().await?;
        info!(target: "hd_api::http", "Received HTTP response 200, body: {}", body);
        Ok(serde_json::from_reader(body.as_bytes())?)
    } else {
        let body = rp.text().await?;
        let e: ApiError = serde_json::from_reader(body.as_bytes())?;
        warn!(target: "hd_api::http", "Received HTTP error {}: {:?}", status, e);
        Err(Error::new(e))
    }
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

pub struct Client {
    cl: reqwest::Client,
    authz: Authorizer,
}

pub struct Request {
    rqb: RequestBuilder,
}

impl Client {
    pub fn new(cl: reqwest::Client, authz: Authorizer) -> Client {
        Client { cl, authz }
    }

    /// Generic call to an API endpoint.
    pub async fn request<U: reqwest::IntoUrl, P: Serialize + ?Sized, RP: Serialize + ?Sized>(
        &mut self,
        method: reqwest::Method,
        url: U,
        required: &RP,
        optional: Option<&P>,
    ) -> Result<Request> {
        let rqb = self
            .authz
            .authorize(self.cl.request(method, url))
            .await
            .context("HiDrive::new_request: Building authorized RequestBuilder")?;
        let rqb = rqb.query(required);
        let rqb = if let Some(params) = optional {
            rqb.query(params)
        } else {
            rqb
        };
        Ok(Request { rqb })
    }
}

impl Request {
    pub async fn go<RT: DeserializeOwned + ?Sized>(self) -> Result<RT> {
        info!(target: "hd_api::http", "sending http request: {:?}", self.rqb);
        let resp = self.rqb.send().await?;
        read_body_to_json(resp).await
    }

    pub async fn download_file<W: AsyncWrite + Unpin>(self, dst: W) -> Result<usize> {
        info!(target: "hd_api::http", "sending http request for download: {:?}", self.rqb);
        write_response_to_file(self.rqb.send().await?, dst).await
    }

    pub fn set_body<B: Into<reqwest::Body>>(self, b: B) -> Self {
        Self {
            rqb: self.rqb.body(b),
        }
    }

    pub fn set_header<K: Into<HeaderName>, V: AsRef<str>>(self, k: K, v: V) -> Self {
        Self {
            rqb: self
                .rqb
                .header(k, HeaderValue::from_str(v.as_ref()).unwrap()),
        }
    }

    pub fn set_attachment<B: Into<reqwest::Body>>(self, b: B) -> Self {
        self.set_header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .set_body(b)
    }
}
