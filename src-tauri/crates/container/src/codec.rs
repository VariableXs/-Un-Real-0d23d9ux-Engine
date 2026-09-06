//! chunk 压缩分级（B-14）——LZ4（热/快）与 Zstd-19（冷/高压缩比）自动选择。
//!
//! 策略：先 LZ4；若压缩比 < 1.25 且原长 ≥ 256KB（大块更在意空间），改试 Zstd-19，
//! 取更小者。不可压缩（压缩后反而变大）一律回存原样 codec=RAW。

use crate::uxv::{CODEC_LZ4, CODEC_RAW, CODEC_ZSTD};
use crate::{CmdResult, ContainerError};

/// LZ4 与 Zstd 的分界：≥ 256KB 且 LZ4 收益不佳时才值得付出 Zstd-19 的高压缩耗时。
const ZSTD_MIN_SIZE: usize = 256 * 1024;

pub fn compress(data: &[u8]) -> (u8, Vec<u8>) {
    let lz4 = lz4_flex::compress_prepend_size(data);
    if lz4.len() >= data.len() {
        return (CODEC_RAW, data.to_vec());
    }
    let pick_zstd = data.len() >= ZSTD_MIN_SIZE && (lz4.len() * 4) > data.len() * 3; // 比率 < 1.25
    if pick_zstd {
        if let Ok(z) = zstd::bulk::compress(data, 19) {
            if z.len() < lz4.len() {
                return (CODEC_ZSTD, z);
            }
        }
    }
    (CODEC_LZ4, lz4)
}

pub fn decompress(codec: u8, stored: &[u8]) -> CmdResult<Vec<u8>> {
    match codec {
        CODEC_RAW => Ok(stored.to_vec()),
        CODEC_LZ4 => lz4_flex::decompress_size_prepended(stored)
            .map_err(|e| ContainerError::Corrupted(format!("LZ4 解压失败: {e}"))),
        CODEC_ZSTD => zstd::bulk::decompress(stored, 64 * 1024 * 1024)
            .map_err(|e| ContainerError::Corrupted(format!("Zstd 解压失败: {e}"))),
        other => Err(ContainerError::Corrupted(format!("未知 codec {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| (i as u8).wrapping_mul(seed).wrapping_add(seed)).collect()
    }

    #[test]
    fn compressible_data_uses_lz4_or_zstd() {
        let text = b"the quick brown fox jumps over the lazy dog. ".repeat(20000); // ~900KB 可压缩
        let (codec, stored) = compress(&text);
        assert!(codec == CODEC_LZ4 || codec == CODEC_ZSTD, "可压缩数据应被压缩");
        assert!(stored.len() < text.len() / 4);
        assert_eq!(decompress(codec, &stored).unwrap(), text);
    }

    #[test]
    fn incompressible_data_stays_raw() {
        use rand::RngCore;
        let mut noise = vec![0u8; 64 * 1024];
        rand::thread_rng().fill_bytes(&mut noise);
        let (codec, stored) = compress(&noise);
        assert_eq!(codec, CODEC_RAW, "随机数据应回存原样");
        assert_eq!(decompress(codec, &stored).unwrap(), noise);
    }

    #[test]
    fn small_hot_blocks_prefer_lz4() {
        let text = vec![b'a'; 200 * 1024]; // < 256KB：只走 LZ4
        let (codec, _) = compress(&text);
        assert_eq!(codec, CODEC_LZ4);
    }

    #[test]
    fn roundtrip_all_sizes() {
        for n in [0usize, 1, 37, 4096, 4 * 1024 * 1024] {
            let d = blob(n, 7);
            let (c, s) = compress(&d);
            assert_eq!(decompress(c, &s).unwrap(), d, "n={n}");
        }
    }
}
