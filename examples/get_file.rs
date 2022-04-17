use log::info;
use simple_logger;

use hd_api::{hidrive, oauth2};

use anyhow;
use reqwest;
use tokio;

async fn get_file<'a>(mut u: hidrive::HiDriveFiles<'a>) -> anyhow::Result<()> {
    let mut p = hidrive::Params::new();
    p.add_str("path", "/users/lebohd0/hd_api");
    p.add_str(
        "fields",
        "name,id,parent_id,nmembers,type,members,readable,writable",
    );
    info!(target: "get_file", "Checking directory...");
    let dir = u.get_dir(Some(&p)).await?;
    println!("{:?}", dir);

    let mut p = hidrive::Params::new();
    p.add_str("path", "/users/lebohd0/hd_api/test.txt");
    let n = u.get(tokio::io::stdout(), Some(&p)).await?;
    println!("Got {} bytes.", n);

    let h = u.hash(0, &[(0, 100)], Some(&p)).await?;
    println!("Hash: {:?}", h);

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();

    let client = reqwest::Client::new();

    let cred = oauth2::Credentials::load("credentials.json").await.unwrap();
    let cid = oauth2::ClientSecret::load("clientsecret.json")
        .await
        .unwrap();
    let authz = oauth2::Authorizer::new_with_client(cred, cid, client.clone());

    let mut hd = hidrive::HiDrive::new(reqwest::Client::new(), authz);
    get_file(hd.files()).await.unwrap();
}
