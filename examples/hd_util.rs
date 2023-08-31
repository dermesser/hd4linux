use clap::{Parser, Subcommand};
use log::info;
use serde_json::to_string_pretty;

use std::path::Path;

use hd_api::{hidrive, oauth2, types};
use hd_api::{Identifier, Params};

#[derive(Subcommand)]
enum Commands {
    List { folder: String },
    Delete { file: String },
    Get { file: String },
    Put { file: String, folder: String },
    Mvfile { from: String, to: String },
    Thumbnail { path: String },
    Url { path: String },
    Metadata { path: String },
    Search { term: String },
}

#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[allow(dead_code)]
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

async fn delete_file(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    file: impl AsRef<str>,
) -> anyhow::Result<()> {
    let id = Identifier::Relative {
        id: home.id,
        path: file.as_ref().to_string(),
    };
    u.delete(id, None).await
}

async fn mv_file(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    from: impl AsRef<str>,
    to: impl AsRef<str>,
) -> anyhow::Result<()> {
    let from = Identifier::Relative {
        id: home.id.clone(),
        path: from.as_ref().to_string(),
    };
    let to = Identifier::Relative {
        id: home.id,
        path: to.as_ref().to_string(),
    };
    u.mv(from, to, None).await?;
    Ok(())
}

async fn list_files(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    folder: impl AsRef<str>,
) -> anyhow::Result<()> {
    let mut p = Params::new();
    p.add_str(
        "fields",
        "name,id,parent_id,nmembers,type,members,readable,writable,size,members.size,members.chash,members.nmembers",
    );
    let id = Identifier::Relative {
        id: home.id,
        path: folder.as_ref().to_string(),
    };
    info!(target: "get_file", "Checking directory...");
    let dir = u.get_dir(id, Some(&p)).await?;
    let mapper = |f: types::Item| {
        (
            if let Some(s) = f.nmembers {
                format!("{:3} sub", s)
            } else {
                format!("{} B", f.size.expect("file size"))
            },
            f.name.expect("missing file name in response"),
        )
    };
    let files = dir.members.into_iter().map(mapper).collect::<Vec<_>>();
    for f in files.iter() {
        println!("{:32} ({})", f.1, f.0);
    }

    Ok(())
}

async fn get_file(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    file: impl AsRef<str>,
) -> anyhow::Result<()> {
    let path = file.as_ref();
    let basename = Path::new(&path)
        .file_name()
        .expect("file name to string")
        .to_str()
        .expect("file path to_str");
    let dst_file = tokio::fs::File::create(basename)
        .await
        .expect("open output file");
    let id = Identifier::Relative {
        id: home.id.clone(),
        path: path.to_string(),
    };
    let n = u.get(id, dst_file, None).await?;
    println!("Downloaded {} bytes.", n);

    Ok(())
}

async fn url(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    file: impl AsRef<str>,
) -> anyhow::Result<()> {
    let url = u
        .url(
            Identifier::Relative {
                id: home.id,
                path: file.as_ref().to_string(),
            },
            None,
        )
        .await?;
    println!("{}", url.url);
    Ok(())
}

async fn metadata(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    file: impl AsRef<str>,
) -> anyhow::Result<()> {
    let it = u
        .metadata(
            Identifier::Relative {
                id: home.id,
                path: file.as_ref().to_string(),
            },
            "path,name,chash,nhash,mhash,mohash,teamfolder,rshare,members,nmembers,id,parent_id,ctime,has_dirs,mtime,readable,size,type,writable",
            None,
        )
        .await?;
    println!("{}", to_string_pretty(&it)?);
    Ok(())
}

async fn search(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    term: impl AsRef<str>,
) -> anyhow::Result<()> {
    let mut p = Params::new();
    p.add_str("pattern", term);
    let it = u.search(Identifier::Id(home.id), "", Some(&p)).await?;
    for i in it.iter() {
        println!("{}", i.path);
    }
    Ok(())
}

async fn thumbnail(
    mut u: hidrive::HiDriveFiles<'_>,
    home: Home,
    file: impl AsRef<str>,
) -> anyhow::Result<()> {
    let basename = Path::new(file.as_ref())
        .file_name()
        .expect("file name to string")
        .to_str()
        .expect("file path to_str");
    let dst = tokio::fs::File::create(basename)
        .await
        .expect("open output file");
    u.thumbnail(
        Identifier::Relative {
            id: home.id,
            path: file.as_ref().to_string(),
        },
        dst,
        None,
    )
    .await?;
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

    u.upload(
        Identifier::Relative {
            id: home.id.to_string(),
            path: path.to_string(),
        },
        filename,
        file,
        None,
    )
    .await
    .expect("upload failed");

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
        Commands::Delete { file } => delete_file(hd.files(), home, file)
            .await
            .expect("delete_file"),
        Commands::Mvfile { from, to } => {
            mv_file(hd.files(), home, from, to).await.expect("mv_file")
        }
        Commands::Thumbnail { path } => thumbnail(hd.files(), home, path).await.expect("thumbnail"),
        Commands::Url { path } => url(hd.files(), home, path).await.expect("url"),
        Commands::Metadata { path } => metadata(hd.files(), home, path).await.expect("metadata"),
        Commands::Search { term } => search(hd.files(), home, term).await.expect("search"),
    }
}
