use log::info;

use hd_api::Params;
use hd_api::{hidrive, oauth2};

async fn get_file(mut u: hidrive::HiDriveFiles<'_>) -> anyhow::Result<()> {
    let mut p = Params::new();
    p.add_str("path", "/users/lebohd0/hd_api");
    p.add_str(
        "fields",
        "name,id,parent_id,nmembers,type,members,readable,writable",
    );
    info!(target: "get_file", "Checking directory...");
    let dir = u.get_dir(Some(&p)).await?;
    println!("{:?}", dir);

    let mut p = Params::new();
    p.add_str("path", "test.txt")
        .add_str("pid", dir.id.unwrap());
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

    // We assume that credentials already exist.
    let cred = oauth2::Credentials::load("credentials.json").await.expect("Credentials couldn't be read: make sure they are there and/or authorize using the `user_me` example.");
    let cid = oauth2::ClientSecret::load("clientsecret.json")
        .await
        .unwrap();
    let authz = oauth2::Authorizer::new_with_client(cred, cid, client.clone());

    let mut hd = hidrive::HiDrive::new(client, authz);
    get_file(hd.files()).await.unwrap();
}
