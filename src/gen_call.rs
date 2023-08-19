use anyhow::{Context, Error, Result};
use futures_util::Future;
use log::{info, warn};
use serde::{de::DeserializeOwned, Serialize};

use crate::oauth2::Authorizer;
use crate::types::*;

/// This is a callback for gen_call_cb, deserializing the response to JSON.
async fn read_body_to_json<RT: DeserializeOwned + ?Sized>(rp: reqwest::Response) -> Result<RT> {
    let status = rp.status();
    if status.is_success() {
        let body = rp.text().await?;
        info!(target: "hd_api::hidrive", "Received HTTP response 200, body: {}", body);
        Ok(serde_json::from_reader(body.as_bytes())?)
    } else {
        let body = rp.text().await?;
        let e: ApiError = serde_json::from_reader(body.as_bytes())?;
        warn!(target: "hd_api::hidrive", "Received HTTP error {}: {:?}", status, e);
        Err(Error::new(e))
    }
}

pub async fn gen_call<
    U: reqwest::IntoUrl,
    P: Serialize + ?Sized,
    RP: Serialize + ?Sized,
    RT: DeserializeOwned,
    BT: Into<reqwest::Body>,
>(
    client: &mut reqwest::Client,
    authz: &mut Authorizer,
    method: reqwest::Method,
    url: U,
    required: &RP,
    optional: Option<&P>,
    body: Option<BT>,
) -> Result<RT> {
    gen_call_cb(
        client,
        authz,
        method,
        url,
        required,
        optional,
        body,
        read_body_to_json,
    )
    .await
}

/// Generic call to an API endpoint.
pub async fn gen_call_cb<
    U: reqwest::IntoUrl,
    P: Serialize + ?Sized,
    RP: Serialize + ?Sized,
    RT,
    RF: Future<Output = Result<RT>>,
    CB: FnOnce(reqwest::Response) -> RF,
    BT: Into<reqwest::Body>,
>(
    client: &mut reqwest::Client,
    authz: &mut Authorizer,
    method: reqwest::Method,
    url: U,
    required: &RP,
    optional: Option<&P>,
    body: Option<BT>,
    cb: CB,
) -> Result<RT> {
    let rqb = authz
        .authorize(client.request(method, url))
        .await
        .context("HiDrive::new_request: Building authorized RequestBuilder")?;
    let mut rqb = rqb.query(required);
    if let Some(body) = body {
        rqb = rqb.body(body);
    }
    let rqb = if let Some(params) = optional {
        rqb.query(params)
    } else {
        rqb
    };
    info!(target: "hd_api::hidrive", "Sending HTTP request: {:?}", rqb);
    let rp = rqb.send().await?;
    cb(rp).await
}
