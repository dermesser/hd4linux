// OAuth2 flow for hidrive installed application.

// TODO:
// Make defaults configurable
// Use better error handling (not string-typed)
// Implement refreshing tokens

use anyhow::{self, Context};
use log::{error, info, warn};

use hyper::{server, service};
use json::JsonValue;
use time::ext::NumericalDuration;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// Returns client_id and client_secret.
pub async fn load_client_secret_from_json(
    p: impl AsRef<std::path::Path>,
) -> anyhow::Result<(String, String)> {
    let mut s = String::new();
    fs::OpenOptions::new()
        .read(true)
        .open(p)
        .await?
        .read_to_string(&mut s)
        .await?;
    let j = json::parse(&s)?;
    let (ci, cs): (String, String) = match j {
        JsonValue::Object(o) => (
            o["client_id"].as_str().unwrap_or("").into(),
            o["client_secret"].as_str().unwrap_or("").into(),
        ),
        _ => {
            return Err(anyhow::Error::msg(format!(
                "Expected object; client credential file is {}",
                s
            )))
        }
    };
    if ci.is_empty() || cs.is_empty() {
        Err(anyhow::Error::msg("client_id or client_secret not set!"))
    } else {
        Ok((ci, cs))
    }
}

pub async fn load_credentials_from_file(
    p: impl AsRef<std::path::Path>,
) -> anyhow::Result<Credentials> {
    let mut s = String::new();
    fs::OpenOptions::new()
        .read(true)
        .open(p)
        .await?
        .read_to_string(&mut s)
        .await?;
    let j = json::parse(&s)?;
    if let JsonValue::Object(ref o) = j {
        if o["refresh_token"].as_str().is_some() {
            return Ok(Credentials(j));
        }
    }
    Err(anyhow::Error::msg(
        "load_credentials_from_file: content doesn't look like valid credentials!",
    ))
}

/// A JSON object looking like this:
/// {
///   "refresh_token": "rt-abcdeabcde",
///   "expires_in": 3600,
///   "userid": "12345.12345.12345",
///   "access_token": "ssklnLKwerlnc9sal",
///   "alias": "lebohd0",
///   "token_type": "Bearer",
///   "scope": "ro,user"
/// }
pub struct Credentials(JsonValue);

impl Credentials {
    /// Returns None only if the JSON object is malformed.
    pub fn refresh_token(&self) -> Option<String> {
        self.access_field("refresh_token")
    }

    pub fn access_token(&self) -> Option<String> {
        self.access_field("access_token")
    }

    pub fn name(&self) -> Option<String> {
        self.access_field("alias")
    }

    pub fn expires_in(&self) -> Option<usize> {
        match &self.0 {
            &JsonValue::Object(ref o) => o["expires_in"].as_usize(),
            _ => None,
        }
    }

    fn access_field(&self, f: &str) -> Option<String> {
        match &self.0 {
            &JsonValue::Object(ref o) => Some(o[f].as_str().unwrap_or("").into()),
            _ => None,
        }
    }

    /// Save credentials to file.
    async fn save(&self, f: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let c = self.0.pretty(2);
        fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(f)
            .await?
            .write_all(c.as_bytes())
            .await
            .map_err(|e| e.into())
    }

    /// Load credentials from file.
    async fn load(&self, f: impl AsRef<std::path::Path>) -> anyhow::Result<Credentials> {
        let mut s = String::new();
        fs::OpenOptions::new()
            .read(true)
            .open(f)
            .await?
            .read_to_string(&mut s)
            .await?;
        let j = json::parse(&s)?;
        Ok(Credentials(j))
    }
}

pub struct Authorizer {
    cred: Credentials,
    client_id: String,
    client_secret: String,

    http_cl: reqwest::Client,

    token_url: String,
    current_token: Option<(String, time::Instant)>,
}

impl Authorizer {
    pub fn new(c: Credentials, client_id: String, client_secret: String) -> Authorizer {
        Authorizer {
            cred: c,
            client_id: client_id,
            client_secret: client_secret,
            http_cl: reqwest::Client::new(),
            token_url: DEFAULT_TOKEN_URL.into(),
            current_token: None,
        }
    }

    pub fn new_with_client(
        c: Credentials,
        client_id: String,
        client_secret: String,
        cl: reqwest::Client,
    ) -> Authorizer {
        Authorizer {
            cred: c,
            client_id: client_id,
            client_secret: client_secret,
            http_cl: cl,
            token_url: DEFAULT_TOKEN_URL.into(),
            current_token: None,
        }
    }

    /// Returns a Bearer token for subsequent use.
    pub async fn token(&mut self) -> anyhow::Result<String> {
        match self.current_token {
            None => (),
            Some((ref t, ref c)) => {
                // Token available and not expired
                if c.elapsed() < ((self.cred.expires_in().unwrap_or(1800) - 30) as f64).seconds() {
                    return Ok(t.clone());
                }
            }
        };

        // No current token available, need to refresh.
        self.current_token = Some(self.refresh().await?);
        Ok(self.current_token.as_ref().unwrap().0.clone())
    }

    async fn refresh(&mut self) -> anyhow::Result<(String, time::Instant)> {
        let t = time::Instant::now();
        let refresh_token = self.cred.refresh_token();
        if refresh_token.is_none() {
            return Err(anyhow::Error::msg(
                "Authorizer: No refresh token found in credentials!",
            ));
        }
        let url = format!(
            "{}?client_id={}&client_secret={}&grant_type=refresh_token&refresh_token={}",
            self.token_url,
            self.client_id,
            self.client_secret,
            refresh_token.unwrap()
        );
        let cl = reqwest::Client::new();
        let req = cl
            .post(url)
            .build()
            .map_err(|e| anyhow::Error::new(e).context("Couldn't build token exchange request."))?;
        let resp = match cl.execute(req).await {
            Err(e) => return Err(anyhow::Error::new(e).context("Couldn't exchange code for token")),
            Ok(resp) => resp,
        };
        let body = String::from_utf8(resp.bytes().await?.into_iter().collect())?;
        let token = json::parse(&body)?;
        self.cred = Credentials(token);
        if let Some(at) = self.cred.access_token() {
            Ok((at, t))
        } else {
            Err(anyhow::Error::msg(
                "Authorizer: no access token found in API response!",
            ))
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LogInState {
    Start,          // Next: WaitingForCode or ReceivedCode
    WaitingForCode, // Next: ReceivedCode
    ReceivedCode,   // Next: ExchangingCode
    ExchangingCode, // Next: Complete
    Complete,

    Error,
}

impl Default for LogInState {
    fn default() -> LogInState {
        LogInState::Start
    }
}

/// LogInFlow implements the process authorizing us to access a user's HiDrive.
/// Once the credentials have been obtained, they should be saved in a safe place and subsequently
/// given to an `Authorizer` which will produce access tokens from it.
#[derive(Debug, Clone, Default)]
pub struct LogInFlow {
    client_id: String,
    client_secret: String,
    authorization_url: String,
    token_url: String,

    state: LogInState,
    authz_code: Option<String>,
}

const DEFAULT_AUTHORIZATION_URL: &'static str = "https://my.hidrive.com/client/authorize";
const DEFAULT_TOKEN_URL: &'static str = "https://my.hidrive.com/oauth2/token";
const DEFAULT_BODY_RESPONSE: &'static str = r"
<html>
<head><title>Authorization complete</title></head>
<body>Authorization is complete; you may close this window now
<hr />
hd_api 0.1
</body>
</html>";
const DEFAULT_ERROR_RESPONSE: &'static str = r"
<html>
<head><title>Authorization failed</title></head>
<body>Something went wrong; please return to the application
<hr />
hd_api 0.1
</body>
</html>";

impl LogInFlow {
    pub fn default_instance(client_id: String, client_secret: String) -> LogInFlow {
        LogInFlow {
            client_id: client_id,
            client_secret: client_secret,
            authorization_url: DEFAULT_AUTHORIZATION_URL.into(),
            token_url: DEFAULT_TOKEN_URL.into(),

            ..Default::default()
        }
    }

    pub fn new(
        client_id: String,
        client_secret: String,
        auth_url: String,
        token_url: String,
    ) -> LogInFlow {
        LogInFlow {
            client_id: client_id,
            client_secret: client_secret,
            authorization_url: auth_url,
            token_url: token_url,

            ..Default::default()
        }
    }

    /// Obtain URL for user to navigate to in order to authorize us.
    pub fn get_authorization_url(&self, scope: String) -> String {
        format!(
            "{}?client_id={}&response_type=code&scope={}",
            self.authorization_url, self.client_id, scope
        )
    }

    /// If the authorization code was received out-of-band, it can be supplied here.
    pub fn supply_authorization_code(&mut self, code: String) {
        self.authz_code = Some(code);
        self.state = LogInState::ReceivedCode;
    }

    /// If your application is configured with a redirect-to-localhost scheme, this will
    /// start a web server on port 8087 (TO DO: make this adjustable) and wait for the redirect
    /// request.
    pub async fn wait_for_redirect(&mut self) -> anyhow::Result<()> {
        let rdr = RedirectHandlingServer::new();
        match rdr.start_and_wait_for_code().await {
            LogInResult::Ok { code } => {
                self.authz_code = Some(code);
                self.state = LogInState::ReceivedCode;
            }
            LogInResult::Err { err } => {
                self.state = LogInState::Error;
                return Err(
                    anyhow::Error::msg(err).context("Received error from redirect catching server")
                );
            }
        }
        Ok(())
    }

    /// Exchange the received code for access tokens.
    pub async fn exchange_code(&mut self) -> anyhow::Result<Credentials> {
        if self.state != LogInState::ReceivedCode {
            return Err(anyhow::Error::msg(format!(
                "LogInFlow: wrong state {:?}: no code obtained yet!",
                self.state
            )));
        }
        let code = match self.authz_code {
            None => return Err(anyhow::Error::msg("No code obtained yet!")),
            Some(ref c) => c,
        };
        let url = format!(
            "{}?client_id={}&client_secret={}&grant_type=authorization_code&code={}",
            self.token_url, self.client_id, self.client_secret, code
        );
        self.state = LogInState::ExchangingCode;
        let cl = reqwest::Client::new();
        let req = cl
            .post(url)
            .build()
            .map_err(|e| anyhow::Error::new(e).context("Couldn't build token exchange request."))?;
        let resp = match cl.execute(req).await {
            Err(e) => return Err(anyhow::Error::new(e).context("Couldn't exchange code for token")),
            Ok(resp) => resp,
        };
        let body = String::from_utf8(resp.bytes().await?.into_iter().collect())?;
        let token = json::parse(&body)?;
        self.state = LogInState::Complete;
        Ok(Credentials(token))
    }
}

// The following ones are only pub for debugging.

// So far only a normal Result, but can be extended.
#[derive(Debug, Clone, PartialEq)]
enum LogInResult {
    Ok { code: String },
    Err { err: String },
}

struct RedirectHandlingServer {
    port: u16,
}

impl RedirectHandlingServer {
    fn new() -> RedirectHandlingServer {
        RedirectHandlingServer { port: 8087 }
    }

    async fn start_and_wait_for_code(&self) -> LogInResult {
        let (s, mut r) = mpsc::channel::<LogInResult>(1);
        let (sds, mut sdr) = mpsc::channel::<()>(1);
        // Wow, this is quite complex for something so simple...
        let mkservice = service::make_service_fn(|_c: &server::conn::AddrStream| {
            let s = s.clone();
            let sd = sds.clone();
            async move {
                Ok::<_, std::convert::Infallible>(service::service_fn(move |rq| {
                    RedirectHandlingServer::handle(rq, s.clone(), sd.clone())
                }))
            }
        });
        let srv = server::Server::bind(&([127, 0, 0, 1], self.port).into()).serve(mkservice);
        info!(target: "hd_api", "Bound server for code callback...");
        // Wait for handler to signal arrival of request.
        let graceful = srv.with_graceful_shutdown(async move {
            sdr.recv().await;
            ()
        });
        info!(target: "hd_api", "Started server for code callback...");
        graceful.await.expect("server error!");
        info!(target: "hd_api", "OAuth callback succeeded");
        match r.recv().await {
            Some(l) => l,
            None => LogInResult::Err {
                err: "mpsc error: sender closed prematurely!".into(),
            },
        }
    }

    async fn handle(
        rq: hyper::Request<hyper::Body>,
        result: mpsc::Sender<LogInResult>,
        shutdown: mpsc::Sender<()>,
    ) -> anyhow::Result<hyper::Response<hyper::Body>> {
        shutdown.send(()).await.expect("shutdown: mpsc error");
        info!(target: "hd_api", "Received OAuth callback");
        let response_builder = hyper::Response::builder().status(hyper::StatusCode::OK);
        let q = rq.uri().query();
        let q = match q {
            None => {
                result
                    .send(LogInResult::Err {
                        err: "No query string was supplied by the callback request!".into(),
                    })
                    .await
                    .expect("result: mpsc error");
                return response_builder
                    .body(DEFAULT_ERROR_RESPONSE.into())
                    .map_err(anyhow::Error::new)
                    .context("Couldn't create response to callback request");
            }
            Some(q) => q,
        };
        let kvs: Vec<&str> = q.split('&').collect();
        let mut code = None;
        for kv in kvs {
            if let Some((k, v)) = kv.split_once('=') {
                if k == "code" {
                    code = Some(v);
                }
            }
        }
        if let Some(code) = code {
            result
                .send(LogInResult::Ok { code: code.into() })
                .await
                .expect("mpsc send error");
        } else {
            result
                .send(LogInResult::Err {
                    err: "No 'code' parameter found in callback request!".into(),
                })
                .await
                .expect("mpsc send error");
            return response_builder
                .body(DEFAULT_ERROR_RESPONSE.into())
                .map_err(anyhow::Error::new)
                .context("couldn't create response to callback request");
        }
        response_builder
            .body(DEFAULT_BODY_RESPONSE.into())
            .map_err(anyhow::Error::new)
            .context("couldn't create response to callback request")
    }
}

#[cfg(test)]
mod tests {
    use crate::oauth2;

    #[tokio::test]
    async fn test_code_flow() {
        let rdr = oauth2::RedirectHandlingServer::new();

        for (url, resp) in [
            (
                "http://localhost:8087/?code=thisismycode",
                oauth2::LogInResult::Ok {
                    code: "thisismycode".into(),
                },
            ),
            (
                "http://localhost:8087/?",
                oauth2::LogInResult::Err {
                    err: "No 'code' parameter found in callback request!".into(),
                },
            ),
        ] {
            tokio::spawn(async move {
                println!("{:?}", reqwest::get(url).await);
            });

            let lir = rdr.start_and_wait_for_code().await;
            assert_eq!(lir, resp);
        }
    }

    #[tokio::test]
    async fn manual_test() {
        // Enable this to check out the returned page manually.
        return;
        let rdr = oauth2::RedirectHandlingServer::new();
        println!("{:?}", rdr.start_and_wait_for_code().await);
    }

    #[tokio::test]
    async fn manual_exchange_test() {
        return;
        let (ci, cs) = oauth2::load_client_secret_from_json("clientsecret.json")
            .await
            .unwrap();
        let mut lif = oauth2::LogInFlow::default_instance(ci, cs);
        println!("Go to {}", lif.get_authorization_url("ro".into()));
        lif.wait_for_redirect().await.unwrap();
        println!("Received code! Exchanging...");
        let tok = lif.exchange_code().await.unwrap();
        println!("Got code: {}", tok.0.pretty(2));
    }

    #[tokio::test]
    async fn manual_refresh_test() {
        //return;
        let (ci, cs) = oauth2::load_client_secret_from_json("clientsecret.json")
            .await
            .unwrap();
        let cred = oauth2::load_credentials_from_file("credentials.json")
            .await
            .unwrap();

        let mut authz = oauth2::Authorizer::new(cred, ci, cs);
        println!("{:?}", authz.token().await);
    }
}
