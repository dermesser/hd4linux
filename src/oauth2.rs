// OAuth2 flow for hidrive installed application.

// TODO:
// Implement revocation

use std::fmt::{self, Display, Formatter};

use anyhow::{self, Context};
use log::{error, info, warn};

use hyper::{server, service};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string_pretty};
use time::ext::NumericalDuration;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// An application's client secret.
#[derive(Deserialize, Default, Clone, Debug)]
pub struct ClientSecret {
    client_secret: String,
    client_id: String,
}

impl ClientSecret {
    /// Returns client_id and client_secret. The file must contain a JSON object
    /// with at least the fields client_id and client_secret of type string.
    pub async fn load(p: impl AsRef<std::path::Path>) -> anyhow::Result<ClientSecret> {
        let mut s = String::new();
        fs::OpenOptions::new()
            .read(true)
            .open(p.as_ref())
            .await?
            .read_to_string(&mut s)
            .await?;
        from_str(&s).context("load_client_secret_from_json: error parsing client secret")
    }
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
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Credentials {
    refresh_token: String,
    expires_in: usize,
    userid: String,
    access_token: String,
    alias: String,
    token_type: String,
    scope: Option<String>,
}

impl Credentials {
    /// Save credentials to file.
    pub async fn save(&self, f: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let s = to_string_pretty(self)?;
        fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(f)
            .await?
            .write_all(s.as_bytes())
            .await
            .context("Credentials::save: error writing to file")
    }

    /// Load credentials from file.
    pub async fn load(f: impl AsRef<std::path::Path>) -> anyhow::Result<Credentials> {
        let mut s = String::new();
        fs::OpenOptions::new()
            .read(true)
            .open(f)
            .await?
            .read_to_string(&mut s)
            .await?;
        from_str(&s).context("Credentials::load: error loading credentials from file")
    }
}

pub struct Authorizer {
    cred: Credentials,
    cs: ClientSecret,

    http_cl: reqwest::Client,

    token_url: String,
    current_token: Option<(String, time::Instant)>,
}

impl Authorizer {
    /// Create a new Authorizer instance.
    pub fn new(c: Credentials, cs: ClientSecret) -> Authorizer {
        Authorizer {
            cred: c,
            cs: cs,
            http_cl: reqwest::Client::new(),
            token_url: DEFAULT_TOKEN_URL.into(),
            current_token: None,
        }
    }

    pub fn new_with_client(c: Credentials, cs: ClientSecret, cl: reqwest::Client) -> Authorizer {
        Authorizer {
            cred: c,
            cs: cs,
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
                if c.elapsed() < ((self.cred.expires_in - 30) as f64).seconds() {
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
        let url = format!(
            "{}?client_id={}&client_secret={}&grant_type=refresh_token&refresh_token={}",
            self.token_url, self.cs.client_id, self.cs.client_secret, self.cred.refresh_token
        );
        let req =
            self.http_cl.post(url).build().map_err(|e| {
                anyhow::Error::new(e).context("Couldn't build token exchange request.")
            })?;
        let resp = match self.http_cl.execute(req).await {
            Err(e) => return Err(anyhow::Error::new(e).context("Couldn't exchange code for token")),
            Ok(resp) => resp,
        };
        let body = String::from_utf8(resp.bytes().await?.into_iter().collect())?;
        self.cred = from_str(&body)?;
        Ok((self.cred.access_token.clone(), t))
    }

    /// Set authorization headers on a request builder.
    pub async fn authorize(
        &mut self,
        rqb: reqwest::RequestBuilder,
    ) -> anyhow::Result<reqwest::RequestBuilder> {
        Ok(rqb.header("Authorization", format!("Bearer {}", self.token().await?)))
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
    cs: ClientSecret,

    authorization_url: String,
    token_url: String,

    lang: Lang,

    ok_body: String,
    err_body: String,

    state: LogInState,
    authz_code: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Role {
    User,
    Admin,
    Owner,
}
#[derive(Debug, Clone)]
pub enum Access {
    Ro,
    Rw,
}
#[derive(Debug, Clone)]
pub struct Scope {
    role: Role,
    access: Access,
}

impl Display for Role {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Role::User => "user",
            Role::Admin => "admin",
            Role::Owner => "owner",
        })
    }
}
impl Display for Access {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Access::Ro => "ro",
            Access::Rw => "rw",
        })
    }
}
impl Display for Scope {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.role.fmt(f)?;
        f.write_str(",")?;
        self.access.fmt(f)
    }
}

#[derive(Debug, Clone)]
pub enum Lang {
    De,
    En,
    Es,
    Fr,
    Nl,
    Pt,
}

impl Default for Lang {
    fn default() -> Lang {
        Lang::En
    }
}

impl Display for Lang {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        f.write_str(match self {
            Lang::De => "de",
            Lang::En => "en",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::Nl => "nl",
            Lang::Pt => "pt",
        })
    }
}

// TODO: These could be read from the client secret file.
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
    pub fn default_instance(cs: ClientSecret) -> LogInFlow {
        Self::new(
            cs,
            DEFAULT_AUTHORIZATION_URL.into(),
            DEFAULT_TOKEN_URL.into(),
        )
    }

    pub fn new(cs: ClientSecret, auth_url: String, token_url: String) -> LogInFlow {
        LogInFlow {
            cs: cs,
            authorization_url: auth_url,
            token_url: token_url,
            ok_body: DEFAULT_BODY_RESPONSE.into(),
            err_body: DEFAULT_ERROR_RESPONSE.into(),

            ..Default::default()
        }
    }

    /// Set language for OAuth screens presented to the user.
    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    /// Set the content displayed to a user upon encountering the redirect server operated by the
    /// LogInFlow.
    pub fn set_redirect_screen_body(&mut self, ok_body: String, err_body: String) {
        self.ok_body = ok_body;
        self.err_body = err_body;
    }

    /// Obtain URL for user to navigate to in order to authorize us.
    pub fn get_authorization_url(&self, scope: Scope) -> String {
        format!(
            "{}?client_id={}&response_type=code&scope={}",
            self.authorization_url, self.cs.client_id, scope
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
        let rdr = RedirectHandlingServer::new(self.ok_body.clone(), self.err_body.clone());
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

    /// Call this to exchange the received code for access tokens.
    /// Save the returned credentials somewhere for use in `Authorizer`.
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
            self.token_url, self.cs.client_id, self.cs.client_secret, code
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
        let token = from_str(&body)?;
        self.state = LogInState::Complete;
        Ok(token)
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
    ok_body: String,
    err_body: String,
    port: u16,
}

impl RedirectHandlingServer {
    fn new(ok_body: String, err_body: String) -> RedirectHandlingServer {
        RedirectHandlingServer {
            port: 8087,
            ok_body: ok_body,
            err_body: err_body,
        }
    }

    async fn start_and_wait_for_code(&self) -> LogInResult {
        let (s, mut r) = mpsc::channel::<LogInResult>(1);
        let (sds, mut sdr) = mpsc::channel::<()>(1);
        // Wow, this is quite complex for something so simple...
        let mkservice = service::make_service_fn(|_c: &server::conn::AddrStream| {
            let s = s.clone();
            let sd = sds.clone();
            let (ok_body, err_body) = (self.ok_body.clone(), self.err_body.clone());
            async move {
                Ok::<_, std::convert::Infallible>(service::service_fn(move |rq| {
                    RedirectHandlingServer::handle(
                        rq,
                        s.clone(),
                        sd.clone(),
                        ok_body.clone(),
                        err_body.clone(),
                    )
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
        ok_body: String,
        err_body: String,
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
                    .body(err_body.into())
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
                .body(err_body.into())
                .map_err(anyhow::Error::new)
                .context("couldn't create response to callback request");
        }
        response_builder
            .body(ok_body.into())
            .map_err(anyhow::Error::new)
            .context("couldn't create response to callback request")
    }
}

#[cfg(test)]
mod tests {
    use crate::oauth2;

    #[tokio::test]
    async fn test_code_flow() {
        let rdr = oauth2::RedirectHandlingServer::new(
            oauth2::DEFAULT_BODY_RESPONSE.into(),
            oauth2::DEFAULT_ERROR_RESPONSE.into(),
        );

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
        let rdr = oauth2::RedirectHandlingServer::new(
            oauth2::DEFAULT_BODY_RESPONSE.into(),
            oauth2::DEFAULT_ERROR_RESPONSE.into(),
        );
        println!("{:?}", rdr.start_and_wait_for_code().await);
    }

    #[tokio::test]
    async fn manual_exchange_test() {
        return;
        let cs = oauth2::ClientSecret::load("clientsecret.json")
            .await
            .unwrap();
        let mut lif = oauth2::LogInFlow::default_instance(cs.client_id, cs.client_secret);
        println!(
            "Go to {}",
            lif.get_authorization_url(oauth2::Scope {
                role: oauth2::Role::User,
                access: oauth2::Access::Ro
            })
        );
        lif.wait_for_redirect().await.unwrap();
        println!("Received code! Exchanging...");
        let tok = lif.exchange_code().await.unwrap();
        println!("Got code: {}", serde_json::to_string_pretty(&tok).unwrap());
    }

    #[tokio::test]
    async fn manual_refresh_test() {
        return;
        let cs = oauth2::ClientSecret::load("clientsecret.json")
            .await
            .unwrap();
        let cred = oauth2::Credentials::load("credentials.json").await.unwrap();

        let mut authz = oauth2::Authorizer::new(cred, cs.client_id, cs.client_secret);
        println!("first: {:?}", authz.token().await);
        println!("repeat: {:?}", authz.token().await);
    }
}
