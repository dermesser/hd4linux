// OAuth2 flow for hidrive installed application.

// TODO:
// Implement revocation

use std::fmt::{self, Display, Formatter};
use std::pin::pin;
use std::time::Duration;

use anyhow::{self, Context, Result};
use log::{self, info, error};

use futures_util::future::{select, FutureExt};
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
    /// Returns a client secret. The file must contain a JSON object
    /// with at least the fields `client_id` and `client_secret` of type string.
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

/// Credentials are deserialized from a JSON object looking like this:
/// ```ignore
/// {
///   "refresh_token": "rt-abcdeabcde",
///   "expires_in": 3600,
///   "userid": "12345.12345.12345",
///   "access_token": "ssklnLKwerlnc9sal",
///   "alias": "uvwxyz",
///   "token_type": "Bearer",
///   "scope": "ro,user"
/// }
/// ```
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
        info!(target: "hd_api::oauth2", "Saving credentials to {:?}", f.as_ref());
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
        info!(target: "hd_api::oauth2", "Loading credentials from {:?}", f.as_ref());
        fs::OpenOptions::new()
            .read(true)
            .open(f)
            .await?
            .read_to_string(&mut s)
            .await?;
        from_str(&s).context("Credentials::load: error loading credentials from file")
    }
}

/// Authorizer is responsible for issuing Bearer tokens to HTTP requests, refreshing the access
/// token when necessary.
pub struct Authorizer {
    cred: Credentials,
    cs: ClientSecret,

    http_cl: reqwest::Client,

    token_url: String,
    current_token: Option<(String, time::Instant)>,
}

impl Authorizer {
    /// Create a new Authorizer instance.
    pub fn new(cred: Credentials, cs: ClientSecret) -> Authorizer {
        Authorizer {
            cred,
            cs,
            http_cl: reqwest::Client::new(),
            token_url: DEFAULT_TOKEN_URL.into(),
            current_token: None,
        }
    }

    pub fn new_with_client(
        cred: Credentials,
        cs: ClientSecret,
        http_cl: reqwest::Client,
    ) -> Authorizer {
        Authorizer {
            cred,
            cs,
            http_cl,
            token_url: DEFAULT_TOKEN_URL.into(),
            current_token: None,
        }
    }

    /// Returns a Bearer token for subsequent use.
    pub async fn token(&mut self) -> anyhow::Result<String> {
        // TODO: cache current token on disk and use it if not elapsed yet. This saves one oauth
        // roundtrip.
        match self.current_token {
            None => (),
            Some((ref t, ref c)) => {
                // Token available and not expired
                if c.elapsed() < ((self.cred.expires_in - 30) as f64).seconds() {
                    return Ok(t.clone());
                }
            }
        };

        info!(target: "hd_api::oauth2", "no current token available: refreshing from OAuth2 provider");
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
        info!(target: "hd_api::oauth2", "Refreshing OAuth2 access: {:?}", req);
        let resp = match self.http_cl.execute(req).await {
            Err(e) => return Err(anyhow::Error::new(e).context("Couldn't exchange code for token")),
            Ok(resp) => resp,
        };
        info!(target: "hd_api::oauth2", "Refresh request got response: {:?}", resp);
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

#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub enum LogInState {
    #[default]
    Start, // Next: WaitingForCode or ReceivedCode
    WaitingForCode, // Next: ReceivedCode
    ReceivedCode,   // Next: ExchangingCode
    ExchangingCode, // Next: Complete
    Complete,

    Error,
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

/// Application role
#[derive(Debug, Clone)]
pub enum Role {
    User,
    Admin,
    Owner,
}

/// (Im)mutable access level
#[derive(Debug, Clone)]
pub enum Access {
    Ro,
    Rw,
}

/// Access scope requested by an application.
#[derive(Debug, Clone)]
pub struct Scope {
    pub role: Role,
    pub access: Access,
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

#[derive(Debug, Default, Clone)]
pub enum Lang {
    De,
    #[default]
    En,
    Es,
    Fr,
    Nl,
    Pt,
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
const DEFAULT_AUTHORIZATION_URL: &str = "https://my.hidrive.com/oauth2/authorize";
const DEFAULT_TOKEN_URL: &str = "https://my.hidrive.com/oauth2/token";
const DEFAULT_BODY_RESPONSE: &str = r"
<html>
<head><title>Authorization complete</title></head>
<body>Authorization is complete; you may close this window now
<hr />
hd_api::oauth2 0.1
</body>
</html>";
const DEFAULT_ERROR_RESPONSE: &str = r"
<html>
<head><title>Authorization failed</title></head>
<body>Something went wrong; please return to the application
<hr />
hd_api::oauth2 0.1
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

    pub fn new(cs: ClientSecret, authorization_url: String, token_url: String) -> LogInFlow {
        LogInFlow {
            cs,
            authorization_url,
            token_url,
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
        info!(target: "hd_api::oauth2", "LogInFlow: ReceivedCode");
    }

    /// If your application is configured with a redirect-to-localhost scheme, this will
    /// start a web server on port 8087 (TO DO: make this adjustable) and wait for the redirect
    /// request.
    pub async fn wait_for_redirect(&mut self, abort_p: impl Fn() -> bool) -> anyhow::Result<()> {
        let rdr = RedirectHandlingServer::new(self.ok_body.clone(), self.err_body.clone());
        match rdr.start_and_wait_for_code(abort_p).await {
            LogInResult::Ok { code } => {
                self.authz_code = Some(code);
                self.state = LogInState::ReceivedCode;
                info!(target: "hd_api::oauth2", "LogInFlow: ReceivedCode");
            }
            LogInResult::Err { err } => {
                self.state = LogInState::Error;
                info!(target: "hd_api::oauth2", "LogInFlow: Error (failed to receive code from internal server)");
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
        info!(target: "hd_api::oauth2", "oauth2: Exchanging code");
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
        info!(target: "hd_api::oauth2", "LogInFlow: ExchangingCode");
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
        info!(target: "hd_api::oauth2", "LogInFlow: Complete");
        Ok(token)
    }
}

// High-level authorization logic.

/// An `AuthorizationHandler` is used by `authorize_user()` to perform some custom functionality,
/// and give control to the calling application.
#[async_trait::async_trait]
pub trait AuthorizationHandler: Send {
    /// Display the URL to the user, in order to start the authorization flow.
    async fn display_authorization_url(&mut self, url: String) -> Result<()> {
        println!(
            "Please navigate to {} - the rest will happen automatically.",
            url
        );
        Ok(())
    }
    /// Polled at sub-second frequency while waiting for the user to complete the flow.
    /// If this method returns true, the wait is aborted.
    fn abort_wait_for_redirect(&self) -> bool {
        false
    }
    /// Called after user has successfully completed the authorization flow, and
    /// the code-for-credentials exchange can occur.
    async fn on_received_code(&mut self) {}
}

/// Authorization handler implementing the bare default functionality.
pub struct DefaultAuthorizationHandler;

#[async_trait::async_trait]
impl AuthorizationHandler for DefaultAuthorizationHandler {}

/// High level authorization function: Documents the typical OAuth flow, and can be used for most
/// purposes. The `handler` is used to delegate some tasks and inform the application about the
/// flow's progress.
pub async fn authorize_user(
    handler: &mut dyn AuthorizationHandler,
    client_secret: ClientSecret,
    scope: Scope,
) -> Result<Credentials> {
    let mut flow = LogInFlow::default_instance(client_secret);
    let auth_url = flow.get_authorization_url(scope);
    handler.display_authorization_url(auth_url).await?;
    let abort_wait = || handler.abort_wait_for_redirect();
    flow.wait_for_redirect(abort_wait).await?;
    handler.on_received_code().await;
    let credentials = flow.exchange_code().await?;
    Ok(credentials)
}

// The following ones are only pub for debugging.

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct OAuthError {
    error: String,
    error_description: String,
}

impl std::error::Error for OAuthError {}

impl Display for OAuthError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_fmt(format_args!(
            "OAuth2 error {}: {}",
            self.error, self.error_description
        ))
    }
}

// So far only a normal Result, but can be extended.
#[derive(Debug, Clone)]
enum LogInResult {
    Ok { code: String },
    Err { err: OAuthError },
}

impl Display for LogInResult {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            LogInResult::Ok { code } => f.write_fmt(format_args!("Login OK, code = {}", code)),
            LogInResult::Err { err } => f.write_fmt(format_args!("{}", err)),
        }
    }
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
            ok_body,
            err_body,
        }
    }

    async fn start_and_wait_for_code(&self, abort_wait_p: impl Fn() -> bool) -> LogInResult {
        // Result channel
        let (s, mut r) = mpsc::channel::<LogInResult>(1);
        // Signalling channel: code has been received.
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
        info!(target: "hd_api::oauth2", "Bound server for code callback...");
        // Wait for handler to signal arrival of request.
        let wait_for_abort = async move {
            let mut iv = tokio::time::interval(Duration::from_millis(500));
            while !abort_wait_p() {
                iv.tick().await;
            }
        };
        let (wait_for_abort, r_recv) = (pin!(wait_for_abort), pin!(sdr.recv()));
        let graceful = srv.with_graceful_shutdown(select(wait_for_abort, r_recv).map(|_| {}));
        info!(target: "hd_api::oauth2", "Started server for code callback...");
        if let Err(e) = graceful.await {
            error!(target: "hd_api::oauth2", "RedirectHandlingServer error after shutdown: {}", e);
        }
        match r.recv().now_or_never() {
            Some(Some(l)) => l,
            Some(None) => LogInResult::Err {
                err: OAuthError {
                    error_description: "mpsc error: sender closed prematurely!".into(),
                    error: "clientside".into(),
                },
            },
            None => LogInResult::Err {
                err: OAuthError {
                    error: "timeout".into(),
                    error_description: "OAuth wait for code aborted by app logic".into(),
                },
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
        info!(target: "hd_api::oauth2", "Received OAuth callback");
        let response_builder = hyper::Response::builder().status(hyper::StatusCode::OK);
        let q = rq.uri().query();
        let q = match q {
            None => {
                result
                    .send(LogInResult::Err {
                        err: OAuthError {
                            error_description:
                                "no query string was supplied by the callback request".into(),
                            error: "clientside".into(),
                        },
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
                    err: OAuthError {
                        error_description: "no 'code' parameter supplied in callback request"
                            .into(),
                        error: "clientside".into(),
                    },
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
                    err: super::OAuthError {
                        error_description: "no 'code' parameter supplied in callback request"
                            .into(),
                        error: "clientside".into(),
                    },
                },
            ),
        ] {
            tokio::spawn(async move {
                println!("{:?}", reqwest::get(url).await);
            });

            let lir = rdr.start_and_wait_for_code(|| false).await;
            assert_eq!(format!("{}", lir), format!("{}", resp));
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
        println!("{:?}", rdr.start_and_wait_for_code(|| false).await);
    }

    #[tokio::test]
    async fn manual_exchange_test() {
        return;
        let cs = oauth2::ClientSecret::load("clientsecret.json")
            .await
            .unwrap();
        let mut lif = oauth2::LogInFlow::default_instance(cs);
        println!(
            "Go to {}",
            lif.get_authorization_url(oauth2::Scope {
                role: oauth2::Role::User,
                access: oauth2::Access::Ro
            })
        );
        lif.wait_for_redirect(|| false).await.unwrap();
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

        let mut authz = oauth2::Authorizer::new(cred, cs);
        println!("first: {:?}", authz.token().await);
        println!("repeat: {:?}", authz.token().await);
    }
}
