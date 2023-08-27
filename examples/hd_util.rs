use clap::{Parser, Subcommand};
use log::info;
use serde_json::to_string_pretty;

use std::path::Path;

use hd_api::Params;
use hd_api::{hidrive, oauth2};

#[derive(Subcommand)]
enum Commands {
    List { folder: String },
    Get { file: String },
    Put { file: String, folder: String },
}

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

struct Home {
    path: String,
    id: String,
}

async fn list_me(mut u: hidrive::HiDriveUser<'_>) -> anyhow::Result<Home> {
    let mut p = Params::new();
    p.add_str("fields", "home,home_id");
    let me = u.me(Some(&p)).await?;
    Ok(Home {
        path: me.home,
        id: me.home_id,
    })
}

async fn list_files(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    folder: impl AsRef<str>,
) -> anyhow::Result<()> {
    let mut p = Params::new();
    // To do: first obtain user name.
    let path = folder.as_ref().to_string();
    p.add_str("path", path)
        .add_str(
            "fields",
            "name,id,parent_id,nmembers,type,members,readable,writable",
        )
        .add_str("pid", home.id);
    info!(target: "get_file", "Checking directory...");
    let dir = u.get_dir(Some(&p)).await?;
    println!(
        "{}",
        to_string_pretty(&dir).expect("json: to_string_pretty")
    );

    Ok(())
}

async fn get_file(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    file: impl AsRef<str>,
) -> anyhow::Result<()> {
    let path = file.as_ref();
    let mut p = Params::new();
    p.add_str("path", path).add_str("pid", home.id);

    let basename = Path::new(&path)
        .file_name()
        .expect("file name to string")
        .to_str()
        .expect("file path to_str");
    let dst_file = tokio::fs::File::create(basename)
        .await
        .expect("open output file");
    let n = u.get(dst_file, Some(&p)).await?;
    println!("Downloaded {} bytes.", n);

    Ok(())
}

async fn put_file(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    file: impl AsRef<str>,
    path: impl AsRef<str>,
) -> anyhow::Result<()> {
    let filename = file.as_ref();
    let path = path.as_ref();

    let file = tokio::fs::File::open(filename)
        .await
        .expect("open local file for reading");

    let mut p = Params::new();
    p.add_str("dir_id", home.id).add_str("dir", path).add_str(
        "name",
        Path::new(filename).file_name().unwrap().to_str().unwrap(),
    );
    u.upload(file, Some(&p)).await.expect("upload failed");

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();

    let cli = Args::parse();

    let client = reqwest::Client::new();

    // We assume that credentials already exist.
    let cred = oauth2::Credentials::load("credentials.json").await.expect("Credentials couldn't be read: make sure they are there and/or authorize using the `user_me` example.");
    let cid = oauth2::ClientSecret::load("clientsecret.json")
        .await
        .unwrap();
    let authz = oauth2::Authorizer::new_with_client(cred, cid, client.clone());

    let mut hd = hidrive::HiDrive::new(client, authz);

    let home = list_me(hd.user()).await.expect("query user info");

    match &cli.command {
        Commands::List { folder } => list_files(hd.files(), home, folder)
            .await
            .expect("list_files"),
        Commands::Get { file } => get_file(hd.files(), home, file).await.expect("get_file"),
        Commands::Put { file, folder } => put_file(hd.files(), home, file, folder)
            .await
            .expect("put_file"),
    }
}
