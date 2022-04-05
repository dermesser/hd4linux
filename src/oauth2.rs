// OAuth2 flow for hidrive installed application.

use log::{error, info, warn};


use hyper::{server, service};
use tokio::sync::mpsc;

// First time login to HiDrive.
#[derive(Debug, Clone, Default)]
pub struct LogInFlow {
    client_id: String,
    client_secret: String,
    authorization_url: String,
    token_url: String,

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

impl LogInFlow {
    pub fn default_instance(client_id: String, client_secret: String) -> LogInFlow {
        LogInFlow {
            client_id: client_id,
            client_secret: client_secret,
            authorization_url: DEFAULT_AUTHORIZATION_URL.into(),
            token_url: DEFAULT_TOKEN_URL.into(),
            authz_code: None,
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
    }

    /// If your application is configured with a redirect-to-localhost scheme, this will
    /// start a web server on port 8087 (TO DO: make this adjustable) and wait for the redirect
    /// request.
    pub async fn wait_for_redirect(&mut self) -> Result<(), String> {
        let rdr = RedirectHandlingServer::new();
        match rdr.start_and_wait_for_code().await {
            LogInResult::Ok { code } => self.authz_code = Some(code),
            LogInResult::Err { err } => return Err(err),
        }
        Ok(())
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
    ) -> Result<hyper::Response<hyper::Body>, hyper::http::Error> {
        shutdown.send(()).await.expect("shutdown: mpsc error");
        info!(target: "hd_api", "Received OAuth callback");
        let response = hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .body(DEFAULT_BODY_RESPONSE.into());
        let uri = rq.uri();
        let q = uri.query();
        let q = match q {
            None => {
                result
                    .send(LogInResult::Err {
                        err: "No query string was supplied by the callback request!".into(),
                    })
                    .await
                    .expect("result: mpsc error");
                return response;
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
        }
        response
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
        return;
        let rdr = oauth2::RedirectHandlingServer::new();
        println!("{:?}", rdr.start_and_wait_for_code().await);
    }
}
