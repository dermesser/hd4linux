use std::fmt::{self, Display, Formatter};

use anyhow::Result;
use digest;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt};

// We are using SHA-1 everywhere, thus 20 bytes = 160 bits.
const HASH_BYTES: usize = 20;
const BLOCK_SIZE: usize = 4096;
const LEVEL_GROUP: usize = 256;

fn hash_string<S: AsRef<str>>(s: S) -> Hash {
    let mut h = Sha1::new();
    h.update(s.as_ref().as_bytes());
    Hash::new_from_sha1(h.finalize())
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct HashedName {
    name: String,
    nhash: String,
    mtime: Option<i64>,
    mhash: Option<String>,
}

impl HashedName {
    pub fn new_for_name<S: AsRef<str>>(s: S) -> HashedName {
        HashedName {
            name: s.as_ref().into(),
            nhash: format!("{}", hash_string(s)),
            ..Default::default()
        }
    }

    pub fn new_for_dir<S: AsRef<str>>(s: S, mtime: i64) -> HashedName {
        let nh = hash_string(&s);

        let mut h = Sha1::new();
        h.update(nh.0);
        h.update(&mtime.to_le_bytes());
        let h = Hash::new_from_sha1(h.finalize());

        HashedName {
            name: s.as_ref().into(),
            nhash: format!("{}", nh),
            mtime: Some(mtime),
            mhash: Some(format!("{}", h)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hash([u8; HASH_BYTES]);

impl Hash {
    fn new() -> Hash {
        Hash([0; HASH_BYTES])
    }

    fn new_from_sha1(ga: digest::Output<Sha1>) -> Hash {
        let mut h = Hash::new();
        h.0.copy_from_slice(ga.as_slice());
        h
    }

    fn is_zero_hash(&self) -> bool {
        !self.0.iter().any(|e| *e != 0)
    }
}

// Format an SHA-1 hash.
impl Display for Hash {
    fn fmt(&self, m: &mut Formatter) -> Result<(), fmt::Error> {
        m.write_fmt(format_args!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                self.0[0],
                self.0[1],
                self.0[2],
                self.0[3],
                self.0[4],
                self.0[5],
                self.0[6],
                self.0[7],
                self.0[8],
                self.0[9],
                self.0[10],
                self.0[11],
                self.0[12],
                self.0[13],
                self.0[14],
                self.0[15],
                self.0[16],
                self.0[17],
                self.0[18],
                self.0[19]))
    }
}

#[derive(Debug)]
pub struct HashLevel {
    h: Vec<Hash>,
}

// See uint_macros module in std.
fn carrying_add_u8(a: u8, b: u8, carry: bool) -> (u8, bool) {
    let (s, c) = a.overflowing_add(b);
    let (d, e) = s.overflowing_add(carry as u8);
    (d, c || e)
}

fn add_hashes(h1: Hash, h2: &[u8]) -> Hash {
    assert_eq!(h1.0.len(), h2.len());
    let h1 = &h1.0;
    let mut r = Hash::new();
    let mut carry = false;
    for i in (0..h1.len()).rev() {
        let (s, c) = carrying_add_u8(h1[i], h2[i], carry);
        r.0[i] = s;
        carry = c;
    }

    r
}

impl HashLevel {
    fn new(cap: usize) -> HashLevel {
        HashLevel {
            h: Vec::with_capacity(cap),
        }
    }

    fn collapse(&self) -> HashLevel {
        let mut nhl = HashLevel::new(self.h.len() / LEVEL_GROUP + 1);
        let mut current_sum = Hash::new();
        for i in 0..self.h.len() {
            if i % LEVEL_GROUP == 0 && i > 0 {
                nhl.h.push(current_sum);
                current_sum = Hash::new();
            }
            if self.h[i].is_zero_hash() {
                continue;
            }
            let mut h = Sha1::new();
            h.update(&self.h[i].0);
            h.update(&[i as u8]);
            let hash = h.finalize();
            current_sum = add_hashes(current_sum, hash.as_slice());
        }
        nhl.h.push(current_sum);
        nhl
    }
}

/// A HiDrive hashing tree. See "HiDrive_Synchronization-v3.3-rev28.pdf".
#[derive(Debug)]
pub struct Hashes {
    l: Vec<HashLevel>,
}

impl Display for Hashes {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.l[self.l.len() - 1].h[0].fmt(f)
    }
}

impl Hashes {
    async fn calculate<R: AsyncRead + Unpin>(mut r: R) -> Result<Hashes> {
        let mut l0 = HashLevel { h: vec![] };
        loop {
            let mut buf = [0 as u8; BLOCK_SIZE];
            let n = r.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            let mut hash_arr = Hash::new();
            // Only hash a block if it has non-zero bytes in it.
            if buf.iter().any(|e| *e != 0) {
                let mut h = Sha1::new();
                h.update(&buf);
                let hash = h.finalize();
                hash_arr.0.copy_from_slice(hash.as_slice());
            }
            l0.h.push(hash_arr);
        }

        let mut hashes = Hashes { l: vec![l0] };
        loop {
            if hashes.l[hashes.l.len() - 1].h.len() == 1 {
                break;
            }
            let level = hashes.l[hashes.l.len() - 1].collapse();
            hashes.l.push(level);
        }
        Ok(hashes)
    }

    /// Return the hash of the entire file's hash tree.
    fn top_hash<'a>(&'a self) -> &'a Hash {
        &self.l[self.l.len()-1].h[0]
    }
}

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};
    use tokio::fs;

    #[test]
    fn test_hash_to_string() {
        let data = "abcdef";
        let mut hasher = Sha1::new();
        hasher.update(data.as_bytes());
        let hash = hasher.finalize();
        let mut mh = super::Hash::new();
        mh.0.copy_from_slice(&hash[..]);
        assert_eq!(
            "1f8ac10f23c5b5bc1167bda84b833e5c057a77d2",
            format!("{}", mh)
        );
    }

    #[test]
    fn test_hash_string() {
        assert_eq!(
            "1f8ac10f23c5b5bc1167bda84b833e5c057a77d2",
            format!("{}", super::hash_string("abcdef"))
        );
    }

    #[tokio::test]
    async fn test_hash_tree_4k() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes.txt")
            .await
            .unwrap();
        let h = super::Hashes::calculate(f).await.unwrap();
        assert_eq!("09f077820a8a41f34a639f2172f1133b1eafe4e6", format!("{}", h));
    }

    #[tokio::test]
    async fn test_hash_tree_1m() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes_1M.txt")
            .await
            .unwrap();
        let h = super::Hashes::calculate(f).await.unwrap();
        assert_eq!("75a9f88fb219ef1dd31adf41c93e2efaac8d0245", format!("{}", h));
    }

    #[tokio::test]
    async fn test_hash_tree_2m() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes_2M.txt")
            .await
            .unwrap();
        let h = super::Hashes::calculate(f).await.unwrap();
        assert_eq!("fd0da83a93d57dd4e514c8641088ba1322aa6947", format!("{}", h));
    }

    #[tokio::test]
    async fn test_top_hash_2m() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes_2M.txt")
            .await
            .unwrap();
        let h = super::Hashes::calculate(f).await.unwrap();
        let h = h.top_hash();
        assert_eq!("fd0da83a93d57dd4e514c8641088ba1322aa6947", format!("{}", h));
    }

    #[test]
    fn test_nhash() {
        let name = "HiDrive ☁";
        let mtime = 1456789012;

        let h = super::HashedName::new_for_dir(name, mtime);
        assert_eq!(h.name, name);
        assert_eq!(h.nhash, "f72f99f62d1142f67ac32be03043c0c2adb3ab88");
        assert_eq!(h.mtime.unwrap(), mtime);
        assert_eq!(h.mhash.unwrap(), "4f450fa02257ea368179557f482e73b2fb80b566");
    }
}
