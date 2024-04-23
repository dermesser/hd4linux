# `hd4linux`

Access Strato's HiDrive via the REST API.

This is an API client for Strato's HiDrive. It allows you to access your HiDrive account via the
REST API, and includes a custom OAuth client allowing for user-friendly authentication.

Currently this package is not yet published on crates.io, because important functionality -
notification of changes in the HiDrive account - is still missing. The reason is that this part of
the API appears to be undocumented, and I haven't managed to find a way to access it (but I also
haven't tried very hard).

hd4linux is built on tokio and reqwest, and uses the `serde` and `serde_json` crates for JSON
(de)serialization. It should integrate well with a client application that uses tokio, for example
if you'd like to write a backup or synchronization tool.

## Get Started

There are a few examples. To test authorization, check out the `user_me` example, which will
authorize the user and print their user information. For this, you need a client secret which you
can obtain from the HiDrive developer support (you need to submit their web form). The client secret
is expected to reside in the `clientsecret.json` file in the current directory.

```shell
$ cargo run --example user_me
    Finished dev [unoptimized + debuginfo] target(s) in 0.09s
     Running `target/debug/examples/user_me`
2024-04-23T19:08:19.771Z INFO [hd_api::oauth2] Loading credentials from "credentials.json"
2024-04-23T19:08:19.772Z INFO [hd_api::oauth2] no current token available: refreshing from OAuth2 provider
2024-04-23T19:08:19.772Z INFO [hd_api::oauth2] Refreshing OAuth2 access: Request { method: POST, url: Url { scheme: "https", cannot_be_a_base: false, username: "", password: None, host: Some(Domain("my.hidrive.com")), port: None, path: "/oauth2/token", query: Some("client_id=3b25bdd22eddac82e1d53b2d00aa6446&client_secret=b76b4b862d07a1d1d12fab170ef60fac&grant_type=refresh_token&refresh_token=rt-8cgyqdotsernjlcgghce0zob8t19"), fragment: None }, headers: {} }
2024-04-23T19:08:19.934Z INFO [hd_api::oauth2] Refresh request got response: Response { url: Url { scheme: "https", cannot_be_a_base: false, username: "", password: None, host: Some(Domain("my.hidrive.com")), port: None, path: "/oauth2/token", query: Some("client_id=3b25bdd22eddac82e1d53b2d00aa6446&client_secret=b76b4b862d07a1d1d12fab170ef60fac&grant_type=refresh_token&refresh_token=rt-8cgyqdotsernjlcgghce0zob8t19"), fragment: None }, status: 200, headers: {"connection": "close", "content-type": "application/json", "cache-control": "max-age=0, no-store", "date": "Tue, 23 Apr 2024 19:08:19 GMT", "x-stg-rev": "ed9f30ded391 10.4.1.68:4499", "content-length": "178", "server": "Mojolicious (Perl)", "x-stg-fe": "10.4.1.68:4499", "strict-transport-security": "max-age=31536000; includeSubDomains"} }
2024-04-23T19:08:19.934Z INFO [hd_api::http] sending http request: RequestBuilder { method: GET, url: Url { scheme: "https", cannot_be_a_base: false, username: "", password: None, host: Some(Domain("api.hidrive.strato.com")), port: None, path: "/2.1/user/me", query: Some("fields=account%2Calias%2Cdescr%2Cemail%2Cemail_pending%2Cemail_verified%2Cencrypted%2Cfolder.id%2Cfolder.path%2Cfolder.size%2Chome%2Chome_id%2Cis_admin%2Cis_owner%2Clanguage%2Cprotocols%2Chas_password"), fragment: None }, headers: {"authorization": "Bearer xxxxxxxxxxxxxxxxxxxx"} }
2024-04-23T19:08:20.169Z INFO [hd_api::http] Received HTTP response 200, body: {"home_id":"xxxxxxxxxxx.4","language":"en","account":"xxxxxxxxx.xxxx.xxxx","email_pending":null,"email_verified":true,"folder":{"path":"root/users/xxxxxxx","id":"bxxxxxxxxxx.4","size":208931743225},"descr":"Lewin","is_owner":true,"encrypted":true,"email":"info@xxxxxx.net","protocols":{"git":true,"ftp":true,"rsync":true,"webdav":true,"scp":true,"cifs":false},"is_admin":true,"alias":"xxxxxxx","home":"root/users/xxxxxxx","has_password":true}
{
  "account": "<redacted>",
  "encrypted": true,
  "descr": "Lewin",

```

This results in a credential being stored in the `credentials.json` file, and ensures that your
credentials work.

You can check the example's source code to familiarize yourself with the OAuth flow
(`get_credentials()` function) and the basic API client usage.

### `hd_util`

The `hd_util` example is a bit more useful. It allows listing, deleting, uploading, downloading, and
more operations on files and directories in your HiDrive account. You can run it like this:

```shell
$ cargo run --example hd_util -- help
$ cargo run --example hd_util -- list <folder name>
$ cargo run --example hd_util -- get <remote file>
$ cargo run --example hd_util -- url <remote file>
```

The tool is a bit sensitive with regards to file names. For example, `list` doesn't like leading or
trailing slashes, and you can only list directories.

## License

This code is licensed under the MIT license. See the LICENSE file for details.

This project is not affiliated with Strato AG, and the author is not responsible for any damage
caused by the use of this software. Use at your own risk.

