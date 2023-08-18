use crate::types;

use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::path::Path;
use std::time;

#[cfg(target_family = "unix")]
use std::os::unix::ffi::OsStrExt;

use anyhow::{self, Result};
use digest;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha1::{Digest, Sha1};
use tokio::fs;
use tokio::io::{AsyncRead, AsyncReadExt};

// We are using SHA-1 everywhere, thus 20 bytes = 160 bits.
const HASH_BYTES: usize = 20;
const BLOCK_SIZE: usize = 4096;
const LEVEL_GROUP: usize = 256;

/// A SHA1 hash.
#[derive(Clone, Default)]
pub struct Hash([u8; HASH_BYTES]);

impl Hash {
    fn new() -> Hash {
        Hash([0; HASH_BYTES])
    }

    pub fn new_from_sha1(ga: digest::Output<Sha1>) -> Hash {
        let mut h = Hash::new();
        h.0.copy_from_slice(ga.as_slice());
        h
    }

    pub fn parse<S: AsRef<str>>(sha1: S) -> Result<Hash> {
        let sha1 = sha1.as_ref();
        if sha1.len() != 2 * HASH_BYTES {
            return Err(anyhow::Error::msg(
                "Hash::parse: SHA-1 string must have 40 characters",
            ));
        }
        let mut h = Hash::new();
        for i in 0..HASH_BYTES {
            h.0[i] = u8::from_str_radix(&sha1[2 * i..2 * i + 2], 16)?;
        }
        Ok(h)
    }

    pub fn for_string<S: AsRef<[u8]>>(s: S) -> Hash {
        let mut h = Sha1::new();
        h.update(s.as_ref());
        Hash::new_from_sha1(h.finalize())
    }

    fn is_zero_hash(&self) -> bool {
        !self.0.iter().any(|e| *e != 0)
    }
}

impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct HV {}
        impl<'d> de::Visitor<'d> for HV {
            type Value = Hash;
            fn expecting(&self, f: &mut Formatter) -> fmt::Result {
                write!(f, "String containing 20 hexadecimal digits")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Hash, E> {
                Hash::parse(v).map_err(E::custom)
            }
        }
        deserializer.deserialize_str(HV {})
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

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
}

/// A Hash level (see HiDrive documentation). Contains one hash per block.
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
            h.update(self.h[i].0);
            h.update([i as u8]);
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
    /// Return the hash of the entire file's hash tree, which is used as `chash` in the API.
    pub fn top_hash(&self) -> &Hash {
        &self.l[self.l.len() - 1].h[0]
    }

    pub fn from_api_hashes(ah: &[types::HashedBlock]) -> Result<Hashes> {
        let mut by_level: HashMap<usize, Vec<(usize, Hash)>> = HashMap::new();
        let mut max_level = 0;
        for hb in ah.iter() {
            by_level
                .entry(hb.level)
                .and_modify(|e| e.push((hb.block, hb.hash.clone())))
                .or_insert_with(|| vec![(hb.block, hb.hash.clone())]);
            max_level = usize::max(max_level, hb.level);
        }
        let mut hash_levels = vec![];
        for i in 0..max_level + 1 {
            if let Some(mut hashes) = by_level.remove(&i) {
                hashes.sort_by(|(ref k, ref _v), (ref kk, ref _vv)| k.cmp(kk));
                hash_levels.push(HashLevel {
                    h: hashes.into_iter().map(|(_, v)| v).collect(),
                });
            } else {
                return Err(anyhow::Error::msg(
                    "Missing hash level in API response: this is an API error",
                ));
            }
        }
        Ok(Hashes { l: hash_levels })
    }
}

/// Calculate `nhash`, `mhash`, `chash` at once and return them.
pub async fn file_hashes<S: AsRef<Path>>(path: S) -> Result<(Hash, Hash, Hash)> {
    let nh = nhash(&path);
    let mh = mhash_file(&path).await?;
    let ch = chash_file(&path).await?;
    Ok((nh, mh, ch.top_hash().clone()))
}

/// Calculate nhash for file name.
pub fn nhash<S: AsRef<Path>>(filename: S) -> Hash {
    // To do: handle error when parsing file name.
    Hash::for_string(filename.as_ref().file_name().unwrap().as_bytes())
}

/// Calculate mhash for a given filename and access time (in seconds since epoch).
pub fn mhash<S: AsRef<Path>>(filename: S, mtime: i64, size: Option<u64>) -> Hash {
    let mut h = Sha1::new();
    let nh = nhash(filename);
    h.update(nh.0);
    if let Some(s) = size {
        h.update(s.to_le_bytes());
    }
    h.update(mtime.to_le_bytes());
    Hash::new_from_sha1(h.finalize())
}

/// Hashes a file at the given path to obtain the mhash. This hash goes over file name (basename),
/// file size, and mtime.
pub async fn mhash_file<S: AsRef<Path>>(path: S) -> Result<Hash> {
    let md = fs::metadata(&path).await?;
    let mtime = md
        .modified()?
        .duration_since(time::SystemTime::UNIX_EPOCH)?;
    let mtime_s = mtime.as_secs();
    let fsize = md.len();
    Ok(mhash(path, mtime_s as i64, Some(fsize)))
}

/// Calculate content hash for file at path. A shortcut for opening a file and using `chash`.
pub async fn chash_file<S: AsRef<Path>>(path: S) -> Result<Hashes> {
    let f = fs::OpenOptions::new().read(true).open(path).await?;
    chash(f).await
}

/// Hashes a file's content.
pub async fn chash<R: AsyncRead + Unpin>(mut r: R) -> Result<Hashes> {
    let mut l0 = HashLevel { h: vec![] };
    loop {
        let mut buf = [0_u8; BLOCK_SIZE];
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let mut hash_arr = Hash::new();
        // Only hash a block if it has non-zero bytes in it.
        if buf.iter().any(|e| *e != 0) {
            let mut h = Sha1::new();
            h.update(buf);
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

/// Calculate a `chash` for a directory.
pub fn chash_dir(mhashes: &[Hash], chashes: &[Hash]) -> Hash {
    let mut h = Hash::new();
    for mh in mhashes {
        h = add_hashes(h, &mh.0);
    }
    for ch in chashes {
        h = add_hashes(h, &ch.0);
    }
    h
}

pub fn mohash_dir(mhashes: &[Hash]) -> Hash {
    let mut h = Hash::new();
    for mh in mhashes {
        h = add_hashes(h, &mh.0);
    }
    h
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
        assert_eq!("1f8ac10f23c5b5bc1167bda84b833e5c057a77d2", mh.to_string());
    }

    #[test]
    fn test_hash_string() {
        assert_eq!(
            "1f8ac10f23c5b5bc1167bda84b833e5c057a77d2",
            super::Hash::for_string("abcdef").to_string()
        );
    }

    #[tokio::test]
    async fn test_hash_tree_4k() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes.txt")
            .await
            .unwrap();
        let h = super::chash(f).await.unwrap();
        assert_eq!("09f077820a8a41f34a639f2172f1133b1eafe4e6", h.to_string());
    }

    #[tokio::test]
    async fn test_hash_tree_1m() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes_1M.txt")
            .await
            .unwrap();
        let h = super::chash(f).await.unwrap();
        assert_eq!("75a9f88fb219ef1dd31adf41c93e2efaac8d0245", h.to_string());
    }

    #[tokio::test]
    async fn test_hash_tree_2m() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes_2M.txt")
            .await
            .unwrap();
        let h = super::chash(f).await.unwrap();
        assert_eq!("fd0da83a93d57dd4e514c8641088ba1322aa6947", h.to_string());
    }

    #[tokio::test]
    async fn test_top_hash_2m() {
        let f = fs::OpenOptions::new()
            .read(true)
            .open("testdata/test_hashes_2M.txt")
            .await
            .unwrap();
        let h = super::chash(f).await.unwrap();
        let h = h.top_hash();
        assert_eq!("fd0da83a93d57dd4e514c8641088ba1322aa6947", h.to_string());
    }

    #[test]
    fn test_hash_parse() {
        let hs = "4f450fa02257ea368179557f482e73b2fb80b566";
        let h = super::Hash::parse(hs).unwrap();
        assert_eq!(hs, h.to_string());
    }

    #[test]
    fn test_nhash_mhash() {
        let name = "HiDrive ☁";
        let mtime = 1456789012;

        assert_eq!(
            "f72f99f62d1142f67ac32be03043c0c2adb3ab88",
            super::nhash(name).to_string()
        );
        assert_eq!(
            "4f450fa02257ea368179557f482e73b2fb80b566",
            super::mhash(name, mtime, None).to_string()
        );
    }

    #[test]
    fn test_dirchash() {
        let fname = "sample.bin";
        let fmtime = 1234567890;
        let fsize = 2107392;

        let h = super::chash_dir(
            &[super::mhash(fname, fmtime, Some(fsize))],
            &[super::Hash::parse("fd0da83a93d57dd4e514c8641088ba1322aa6947").unwrap()],
        );
        let mohash = super::mohash_dir(&[super::mhash(fname, fmtime, Some(fsize))]);
        // Directory's chash
        assert_eq!("41ad9693fefd464dea4365e646f56fe96165603d", h.to_string());
        assert_eq!(
            "449fee596b27c879052e9d82366cb5d63ebaf6f6",
            mohash.to_string()
        );
        assert_eq!(
            "449fee596b27c879052e9d82366cb5d63ebaf6f6",
            super::mhash(fname, fmtime, Some(fsize)).to_string()
        );
    }

    #[test]
    fn test_hash_serialize() {
        let h = super::Hash::for_string("abcdef");
        assert_eq!(
            "\"1f8ac10f23c5b5bc1167bda84b833e5c057a77d2\"",
            serde_json::to_string(&h).unwrap()
        );
    }

    #[test]
    fn test_hash_deserialize() {
        // Tests entire roundtrip.
        let h = super::Hash::for_string("abcdef");
        assert_eq!(
            h.to_string(),
            serde_json::from_str::<super::Hash>(&serde_json::to_string(&h).unwrap())
                .unwrap()
                .to_string()
        );
    }

    // Only works with correct mtime, i.e. not in CI.
    // Set mtime using `touch -m --date=@1234567890 testdata/sample.bin`
    //#[tokio::test]
    #[allow(dead_code)]
    async fn test_mhash_file() {
        assert_eq!(
            "449fee596b27c879052e9d82366cb5d63ebaf6f6",
            super::mhash_file("testdata/sample.bin")
                .await
                .unwrap()
                .to_string()
        );
    }

    #[tokio::test]
    async fn test_file_hashes() {
        let (nh, _mh, ch) = super::file_hashes("testdata/sample.bin").await.unwrap();
        assert_eq!("7220d977d2db4499f333bfff421158b9815a686f", nh.to_string());
        // Only works with correct mtime, i.e. not in CI.
        //assert_eq!("449fee596b27c879052e9d82366cb5d63ebaf6f6", mh.to_string());
        assert_eq!("fd0da83a93d57dd4e514c8641088ba1322aa6947", ch.to_string());
    }

    #[test]
    fn test_api_hashes_parsing() {
        let json = r#"{
    "level": 1,
    "chash": "126c9798b09a51d069a8f5bcef5174a41ef9e7ea",
    "list": [
                [
                  {
                    "hash": "55752d29f8c8532e7d01b2e747428217262e0bec",
                    "level": 0,
                    "block": 0
                  },
                  {
                    "hash": "a18d31e22d0a4887b8edf6726d5ea51f7203e649",
                    "level": 0,
                    "block": 1
                  },
                  {
                    "hash": "a40a462a40337331c40734b3d999483401adef3c",
                    "level": 0,
                    "block": 3
                  },
                  {
                    "hash": "09f287ce4192aa31286e2445615f8700300dc9bb",
                    "level": 0,
                    "block": 8
                  }
                ]
            ]
}"#;
        let ah: crate::types::FileHash = serde_json::from_str(json).unwrap();
        println!("{:?}", ah);

        let hashes = super::Hashes::from_api_hashes(&ah.list[0]).unwrap();
        assert_eq!(1, hashes.l.len());
        assert_eq!(4, hashes.l[0].h.len());
    }
}
