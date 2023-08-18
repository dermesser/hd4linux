use anyhow::{self, Result};
use rolling_dual_crc::RollingDualCrc;

use tokio::io::{AsyncBufRead, AsyncReadExt};

#[allow(unused)]
pub async fn find_borders<R: AsyncBufRead + Unpin>(
    r: &mut R,
    window_size: usize,
    zerobits: usize,
) -> Result<Vec<usize>> {
    assert!(zerobits <= 32);
    let mask: u32 = 0xffffffff >> (32 - zerobits);
    let mut buf: Vec<u8> = vec![0; window_size];
    r.read_exact(&mut buf).await?;
    let mut rdc = RollingDualCrc::new(&buf);

    let mut i = window_size;
    let mut borders = vec![];

    while let Ok(b) = r.read_u8().await {
        if rdc.get32() & mask == 0 {
            borders.push(i);
        }
        rdc.roll(b);
        i += 1
    }

    Ok(borders)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn find_borders_of_file<P: AsRef<std::path::Path>>(file: P) -> Result<Vec<usize>> {
        let f = tokio::fs::OpenOptions::new()
            .read(true)
            .open(file)
            .await
            .unwrap();
        let mut bf = tokio::io::BufReader::new(f);

        let borders = find_borders(&mut bf, 32, 10).await;
        borders
    }

    #[tokio::test]
    async fn test_find_borders() {
        println!(
            "Borders: {:?}",
            find_borders_of_file("OAuth2-ServerFlow_NativeLocalhostFlow_v1_2a.pdf").await
        );
    }
}
