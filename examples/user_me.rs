use simple_logger;

use hd_api::{oauth2, hidrive};

use anyhow;
use reqwest;
use serde_json::to_string_pretty;
use tokio;

async fn list_me<'a>(mut u: hidrive::HiDriveUser<'a>) ->  anyhow::Result<()> {
    let mut p = hidrive::Params::new();
    p.add_str("fields".into(), "account,alias,descr,email,email_pending,email_verified,encrypted,folder.id,folder.path,folder.size,home,home_id,is_admin,is_owner,language,protocols,has_password".into());
    let me = u.me(Some(&p)).await?;
    println!("{}", to_string_pretty(&me)?);
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();

    let client = reqwest::Client::new();

    let cred = oauth2::Credentials::load("credentials.json").await.unwrap();
    let cid = oauth2::ClientSecret::load("clientsecret.json").await.unwrap();
    let authz = oauth2::Authorizer::new_with_client(cred, cid, client.clone());

    let mut hd = hidrive::HiDrive::new(reqwest::Client::new(), authz);
    list_me(hd.user()).await.unwrap();
}
