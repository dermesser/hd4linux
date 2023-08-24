use hd_api::oauth2::{self, ClientSecret, Credentials};
use hd_api::{self, hidrive, Params};

use serde_json::to_string_pretty;

async fn list_me(mut u: hidrive::HiDriveUser<'_>) -> anyhow::Result<()> {
    let mut p = Params::new();
    p.add_str("fields", "account,alias,descr,email,email_pending,email_verified,encrypted,folder.id,folder.path,folder.size,home,home_id,is_admin,is_owner,language,protocols,has_password");
    let me = u.me(Some(&p)).await?;
    println!("{}", to_string_pretty(&me)?);
    Ok(())
}

const CLIENT_SECRET_PATH: &str = "clientsecret.json";
const CREDENTIALS_PATH: &str = "credentials.json";

/// Load or obtain credentials by reading from the local credentials cache or doing a new
/// authorization flow. The main work is done by `oauth2::authorize_user()`, here we are mostly
/// concerned with reading and caching the credentials from/to a local file.
async fn get_credentials() -> anyhow::Result<(ClientSecret, Credentials)> {
    let client_secret = oauth2::ClientSecret::load(CLIENT_SECRET_PATH).await?;
    if let Ok(cred) = oauth2::Credentials::load(CREDENTIALS_PATH).await {
        Ok((client_secret, cred))
    } else {
        let mut handler = oauth2::DefaultAuthorizationHandler;
        let scope = oauth2::Scope {
            role: oauth2::Role::User,
            access: oauth2::Access::Rw,
        };
        let credentials =
            oauth2::authorize_user(&mut handler, client_secret.clone(), scope).await?;
        let credentials_contents = to_string_pretty(&credentials)?;
        if let Err(e) = tokio::fs::write(CREDENTIALS_PATH, &credentials_contents).await {
            println!("Warning: could not persist client credentials to {} ({})! You will have to reauthorize next time", CREDENTIALS_PATH, e);
        }
        Ok((client_secret, credentials))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();

    let client = reqwest::Client::new();
    let (client_secret, credentials) = get_credentials().await.unwrap();

    let authz = oauth2::Authorizer::new_with_client(credentials, client_secret, client.clone());

    let mut hd = hidrive::HiDrive::new(client, authz);
    list_me(hd.user()).await.unwrap();
}
