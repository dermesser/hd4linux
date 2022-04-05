// OAuth2 flow for hidrive installed application.

use std::io;

use tokio_channel::oneshot;
use hyper::{server, service};
use json::JsonValue;

// First time login to HiDrive.
pub struct LogInFlow {
    client_id: String,
    client_secret: String,
    authorization_url: String,
    token_url: String,
}

const DEFAULT_AUTHORIZATION_URL: &'static str = "https://my.hidrive.com/client/authorize";
const DEFAULT_TOKEN_URL: &'static str = "https://my.hidrive.com/oauth2/token";

#[async_trait::async_trait]
pub trait ClientRedirectHandler {
    async fn redirect_user(&mut self, url: String);
}

// So far only a normal Result, but can be extended.
enum LogInResult {
    Ok { code: String },
    Err { err: String },
}

struct RedirectHandlingServer {
    server: server::Server,
    ch: oneshot::Receiver<io::Result<LogInResult>>,
}

impl RedirectHandlingServer {
    fn start() {
        let mkservice = service::make_service_fn(move |c: &server::conn::AddrStream| async {

        });
        let s = server::Server::bind(&([127,0,0,1], 8087).into()).serve(mkservice);
    }
}
