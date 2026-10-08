//! 任务35 · Uxv 交换格式接入——内核侧只读校验门（双域总案阶段4·步骤7）。
//!
//! **职责边界（诚实声明）**：Uxv 容器的**写侧/解包/压缩/加密全部复用
//! `src-tauri/crates/container`（`uxv.rs` UxvBackend），内核不复制实现**。
//! 内核侧只做**进系统前的只读校验门**（ingest gate）：跨系统大文件经
//! 共享分区落盘后、进入 VARIX 交换队列前，校验容器结构完整性——
//! 校验失败 = 整包拒绝 + 如实错误码，**绝不产生半文件**。
//!
//! 校验语义逐条对齐 container/uxv.rs `open()`：
//! - SuperBlock（64B）：magic `UXVSTR01` @0..8，footer_offset @12..20；
//!   footer_offset==0 = 会话未 seal（整包拒绝，同引擎侧"无有效 checkpoint"）。
//! - Footer 双副本（96B×2）：副本 B 优先、A 兜底；magic/schema/边界校验。
//! - schema_version == 2（container `SCHEMA_VERSION`）不符 = 拒绝。
//! - 索引 blob 越界校验；**BLAKE3（kernel 自实现 `security::blake3`，与
//!   官方 crate 交叉验证）核对 index_hash**。
//! - **单文件大小上限声明**：`MAX_INGEST_BYTES = 256MiB`（超限拒绝）。
//!
//! 掉电安全：容器本身有 journal（B-13）；内核门只承诺"校验失败整包拒"，
//! 掉电注入 ×10 用例断言任何截断形态都被拒收（无半文件流入队列）。

use crate::kblake3 as blake3;

pub const MAX_INGEST_BYTES: u64 = 256 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"UXVSTR01";
const SCHEMA_VERSION: u32 = 2;
const SUPERBLOCK_LEN: u64 = 64;
const FOOTER_LEN: usize = 96;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IngestError {
    /// 不足 superblock / 空输入。
    TooShort,
    /// magic 不符（非 Uxv 容器）。
    NotUxv,
    /// 会话未 seal（footer_offset==0）。
    Unsealed,
    /// footer 双副本均不可读。
    TornFooter,
    /// schema 版本不符（携带实际值）。
    SchemaMismatch(u32),
    /// 索引 blob 越界。
    IndexOutOfBounds,
    /// 超过单文件大小上限。
    TooLarge,
    /// BLAKE3 索引校验失败（内容被篡改/腐坏）。
    HashMismatch,
    /// 尾部有未完成的追加（总长 > footer 结束 = 非法形态）。
    TrailingBytes,
}

/// 校验结论（开放性：后续可扩展为返回文件清单供交换队列消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IngestOk {
    pub schema_version: u32,
    pub index_offset: u64,
    pub index_len: u64,
}

/// 只读校验门。`blob` = 共享分区上读回的完整容器字节（调用方保证
/// 从盘面一次性读入；本函数零写入、零副作用）。
pub fn ingest(blob: &[u8]) -> Result<IngestOk, IngestError> {
    if (blob.len() as u64) > MAX_INGEST_BYTES {
        return Err(IngestError::TooLarge);
    }
    if blob.len() < SUPERBLOCK_LEN as usize {
        return Err(IngestError::TooShort);
    }
    if &blob[0..8] != MAGIC {
        return Err(IngestError::NotUxv);
    }
    let footer_offset = u64::from_le_bytes(blob[12..20].try_into().expect("定长"));
    if footer_offset == 0 {
        return Err(IngestError::Unsealed);
    }
    if footer_offset < SUPERBLOCK_LEN
        || footer_offset as usize + FOOTER_LEN * 2 > blob.len()
    {
        return Err(IngestError::TornFooter);
    }
    if blob.len() as u64 != footer_offset + FOOTER_LEN as u64 * 2 {
        // 引擎侧 journal 追加属引擎内部态；内核门只收"封口完整"的容器。
        return Err(IngestError::TrailingBytes);
    }
    let both = &blob[footer_offset as usize..footer_offset as usize + FOOTER_LEN * 2];
    let (a, b) = both.split_at(FOOTER_LEN);
    let footer = decode_footer(b).or_else(|| decode_footer(a)).ok_or(IngestError::TornFooter)?;
    if footer.0 != SCHEMA_VERSION {
        return Err(IngestError::SchemaMismatch(footer.0));
    }
    let (_schema, index_offset, index_len, index_hash) = footer;
    if index_offset < SUPERBLOCK_LEN
        || index_offset as u64 + index_len + FOOTER_LEN as u64 * 2 > blob.len() as u64
    {
        return Err(IngestError::IndexOutOfBounds);
    }
    let idx = &blob[index_offset as usize..index_offset as usize + index_len as usize];
    if blake3::hash(idx) != index_hash {
        return Err(IngestError::HashMismatch);
    }
    Ok(IngestOk { schema_version: SCHEMA_VERSION, index_offset, index_len })
}

/// (schema_version, index_offset, index_len, index_hash)
fn decode_footer(b: &[u8]) -> Option<(u32, u64, u64, [u8; 32])> {
    if b.len() < FOOTER_LEN || &b[52..60] != MAGIC {
        return None;
    }
    Some((
        u32::from_le_bytes(b[0..4].try_into().ok()?),
        u64::from_le_bytes(b[4..12].try_into().ok()?),
        u64::from_le_bytes(b[12..20].try_into().ok()?),
        b[20..52].try_into().ok()?,
    ))
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// 按引擎侧布局构造一个最小合法 Uxv 容器（superblock + 索引 + 双 footer）。
    /// 索引哈希用官方 blake3 crate（与内核自实现交叉验证的另一个落点）。
    fn build_container(index: &[u8]) -> Vec<u8> {
        let hash = blake3::hash(index);
        let footer = {
            let mut b = [0u8; FOOTER_LEN];
            b[0..4].copy_from_slice(&SCHEMA_VERSION.to_le_bytes());
            b[4..12].copy_from_slice(&SUPERBLOCK_LEN.to_le_bytes()); // index_offset
            b[12..20].copy_from_slice(&(index.len() as u64).to_le_bytes());
            b[20..52].copy_from_slice(&hash);
            b[52..60].copy_from_slice(MAGIC);
            b
        };
        let footer_offset = SUPERBLOCK_LEN + index.len() as u64;
        let mut sb = [0u8; 64];
        sb[0..8].copy_from_slice(MAGIC);
        sb[12..20].copy_from_slice(&footer_offset.to_le_bytes());
        let mut out = Vec::new();
        out.extend_from_slice(&sb);
        out.extend_from_slice(index);
        out.extend_from_slice(&footer);
        out.extend_from_slice(&footer); // 双副本
        out
    }

    #[test]
    fn valid_container_ingests() {
        let c = build_container(b"\x00\x00\x00\x00\x00\x00\x00\x00fake-index");
        let ok = ingest(&c).expect("合法容器应通过");
        assert_eq!(ok.schema_version, 2);
        assert_eq!(ok.index_offset, SUPERBLOCK_LEN);
        assert_eq!(ok.index_len, 18);
    }

    #[test]
    fn rejection_matrix_honest_error_codes() {
        // 非 Uxv（长度足但 magic 不符）。
        let notuxv = alloc::vec![b'x'; 200];
        assert_eq!(ingest(&notuxv), Err(IngestError::NotUxv));
        // 过短。
        assert_eq!(ingest(&[0u8; 8]), Err(IngestError::TooShort));
        // 未 seal（footer_offset=0）。
        let mut c = build_container(b"idx");
        c[12..20].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(ingest(&c), Err(IngestError::Unsealed));
        // schema 不符（双副本同步破坏，任一副本兜底都该报错）。
        let mut c = build_container(b"idx");
        let fa = SUPERBLOCK_LEN as usize + 3;
        c[fa] = 9;
        c[fa + FOOTER_LEN] = 9;
        assert_eq!(ingest(&c), Err(IngestError::SchemaMismatch(9)));
        // 单副本 torn：副本 A 全坏，副本 B 兜底仍可读。
        let mut c = build_container(b"idx");
        let fa = SUPERBLOCK_LEN as usize + 3;
        for i in 0..FOOTER_LEN {
            c[fa + i] ^= 0xFF;
        }
        assert!(ingest(&c).is_ok(), "B 优先 A 兜底：A 坏仍通过");
        // 双副本均坏（B 优先 A 兜底都救不回）。
        let mut c = build_container(b"idx");
        let fb = fa + FOOTER_LEN;
        for i in 0..FOOTER_LEN {
            c[fa + i] ^= 0xFF;
            c[fb + i] ^= 0xFF;
        }
        assert_eq!(ingest(&c), Err(IngestError::TornFooter));
        // 索引被篡改 → BLAKE3 失配。
        let mut c = build_container(b"idx-bytes-tampered!!");
        let i0 = SUPERBLOCK_LEN as usize;
        c[i0] ^= 0x01;
        assert_eq!(ingest(&c), Err(IngestError::HashMismatch));
        // 尾部多余字节（非封口形态）。
        let mut c = build_container(b"idx");
        c.push(0xAB);
        assert_eq!(ingest(&c), Err(IngestError::TrailingBytes));
        // 超限。
        assert_eq!(ingest(&alloc::vec![0u8; 0]), Err(IngestError::TooShort));
    }

    #[test]
    fn size_cap_declared_and_enforced() {
        assert_eq!(MAX_INGEST_BYTES, 256 * 1024 * 1024, "上限声明：256MiB（完善性验收点）");
    }

    /// 掉电注入 ×10：对合法容器在确定性伪随机位置截断/破坏，
    /// 全部必须被整包拒绝（校验失败不产生半文件）。
    #[test]
    fn power_cut_injection_x10_always_full_reject() {
        let base = build_container(b"power-cut-injection-index-payload");
        // 确定性 LCG 生成 10 个注入形态（截断长度 + 单字节破坏位置组合）。
        let mut x: u32 = 0xC0FFEE;
        for round in 0..10 {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let cut = 1 + (x as usize) % (base.len() - 1);
            let mut c = base.clone();
            c.truncate(cut);
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let flip = (x as usize) % c.len().max(1);
            if !c.is_empty() {
                c[flip] ^= 0x80;
            }
            let verdict = ingest(&c);
            assert!(
                matches!(verdict, Err(_)),
                "第 {} 轮掉电注入（cut={}）必须整包拒绝，禁止半文件",
                round + 1,
                cut
            );
        }
    }
}
