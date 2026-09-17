//! 任务16 · 块设备抽象——接口冻结层（双域总案·2.3 真驱动·步骤 8）。
//!
//! **接口冻结契约**：NVMe / AHCI / 未来 virtio-blk 全部实现同一
//! [`BlockDevice`]。上层（fs23_journal 真盘后端、镜像装载、swap）只认识
//! 本 trait，不感知任何控制器细节。冻结语义：
//!
//! - `block_size()`：字节/块，实现期恒定（NVMe 512；调用方不得假设 512，
//!   必须先读该值定分片）。
//! - `capacity_blocks()`：LBA 总数（不含），越界访问返回
//!   [`BlockError::InvalidRange`]，绝不越界 DMA。
//! - `read_blocks/write_blocks`：`buf.len()` 必须是 block_size 的整数倍
//!   且非零，否则 [`BlockError::InvalidRange`]。半块 IO 一律上层分片。
//! - `flush()`：落盘屏障（NVMe flush 命令 / AHCI cache flush）。实现
//!   不承诺持久化窗口之外的顺序，只保证 flush 返回后先前写入可读。
//!
//! 回环自检 [`loopback_probe`] 是 trait 冻结的行为学测试：写→读回→
//! FNV-1a 校验和对比→交错 LBA 打乱写读，防"顺序命中缓存假通过"。
//! 宿主用 FakeBlockDevice 全逻辑可测（×1000 与总案验收口径一致），
//! 目标态由 NVMe/AHCI 实盘跑同一函数。

/// 块设备错误口径（全实现统一；ErrNo 映射上层处理）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockError {
    /// 控制器上报/总线错误（不可恢复由调用方决定重试策略）。
    Io,
    /// 命令超时（含恢复路径重试耗尽）。
    Timeout,
    /// 控制器经历复位，先前状态失效（调用方应整段重试）。
    DeviceReset,
    /// 越界 LBA / 非法 buf 长度 / 容量查询失败。
    InvalidRange,
    /// 实现不支持的操作（如只读介质收到 write）。
    Unsupported,
}

impl BlockError {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockError::Io => "IO",
            BlockError::Timeout => "TIMEOUT",
            BlockError::DeviceReset => "DEVICE-RESET",
            BlockError::InvalidRange => "INVALID-RANGE",
            BlockError::Unsupported => "UNSUPPORTED",
        }
    }
}

/// 块设备抽象——接口冻结（NVMe/AHCI/virtio 同接口）。
pub trait BlockDevice {
    /// 字节/块。
    fn block_size(&self) -> u32;
    /// LBA 总数（容量），单位块。
    fn capacity_blocks(&self) -> u64;
    /// 读：dst 长度须为 block_size 整数倍且非零。
    fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError>;
    /// 写：src 长度须为 block_size 整数倍且非零。
    fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError>;
    /// 落盘屏障。
    fn flush(&mut self) -> Result<(), BlockError>;
}

/// FNV-1a 64 位校验和（回环自检与验收证据用；与 fs23_journal 的
/// CRC23 是两回事——这里只要"读写通路数据一致"的强区分度）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 回环探针结论（串口验收证据行直接格式化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoopbackReport {
    pub rounds: u64,
    pub blocks_per_round: u32,
    /// 每轮写校验和。
    pub write_sum: u64,
    /// 每轮读回校验和（×rounds 全部一致才 pass；这里存最后一轮）。
    pub read_sum: u64,
    pub passed: bool,
    pub err: Option<BlockError>,
}

/// 回环自检：`rounds` 轮 ×（写入模式 → flush → 读回 → 校验和对比），
/// 起始 LBA 逐轮偏移（交错防缓存假通过）。任何一步 Err 立即如实上报。
///
/// 缓冲 `buf` 由调用方提供（长度 = blocks_per_round × block_size）。
pub fn loopback_probe(
    dev: &mut dyn BlockDevice,
    rounds: u64,
    blocks_per_round: u32,
    buf: &mut [u8],
) -> LoopbackReport {
    let bs = dev.block_size() as usize;
    let nblocks = blocks_per_round as u64;
    let span = blocks_per_round as usize * bs;
    let mut rep = LoopbackReport {
        rounds,
        blocks_per_round,
        write_sum: 0,
        read_sum: 0,
        passed: false,
        err: None,
    };
    if span == 0 || span > buf.len() {
        rep.err = Some(BlockError::InvalidRange);
        return rep;
    }
    // 起始 LBA 跳过 0 号块：0 区常被日志/引导占用，探针不践踏。
    let base_lba: u64 = 16;
    for r in 0..rounds {
        let lba = base_lba + r * (nblocks + 8);
        // 写入模式：块内位置+轮次签名，读回必须逐字节复现。
        for (i, slot) in buf[..span].iter_mut().enumerate() {
            *slot = ((i as u64).wrapping_mul(0x9E) ^ (r as u64).wrapping_mul(0xC2)) as u8;
        }
        rep.write_sum = fnv1a64(&buf[..span]);
        if let Err(e) = dev.write_blocks(lba, &buf[..span]) {
            rep.err = Some(e);
            return rep;
        }
        if let Err(e) = dev.flush() {
            rep.err = Some(e);
            return rep;
        }
        // 读回前把缓冲踩脏，防"读请求被跳过仍像通过"。
        buf[..span].fill(0xA5);
        if let Err(e) = dev.read_blocks(lba, &mut buf[..span]) {
            rep.err = Some(e);
            return rep;
        }
        rep.read_sum = fnv1a64(&buf[..span]);
        if rep.read_sum != rep.write_sum {
            rep.err = Some(BlockError::Io);
            return rep;
        }
    }
    rep.passed = true;
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 宿主假设备：内存块存储，行为与 trait 契约逐条对齐。
    struct FakeBlock {
        bs: u32,
        data: Vec<u8>,
        /// 注入器：读回时翻转一个字节，模拟数据损坏。
        corrupt: bool,
        fail_write: bool,
    }

    impl FakeBlock {
        fn new(bs: u32, blocks: u64) -> Self {
            FakeBlock {
                bs,
                data: vec![0u8; (blocks as usize) * bs as usize],
                corrupt: false,
                fail_write: false,
            }
        }
        fn lba_span(&self, lba: u64, len: usize) -> Result<(usize, usize), BlockError> {
            let end = lba
                .checked_add(len as u64 / self.bs as u64)
                .ok_or(BlockError::InvalidRange)?;
            let cap = (self.data.len() / self.bs as usize) as u64;
            if len == 0 || len % self.bs as usize != 0 || end > cap {
                return Err(BlockError::InvalidRange);
            }
            Ok((lba as usize * self.bs as usize, end as usize * self.bs as usize))
        }
    }

    impl BlockDevice for FakeBlock {
        fn block_size(&self) -> u32 {
            self.bs
        }
        fn capacity_blocks(&self) -> u64 {
            (self.data.len() / self.bs as usize) as u64
        }
        fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
            let (s, e) = self.lba_span(lba, dst.len())?;
            dst.copy_from_slice(&self.data[s..e]);
            if self.corrupt {
                dst[0] ^= 0xFF;
                self.corrupt = false;
            }
            Ok(())
        }
        fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
            if self.fail_write {
                self.fail_write = false;
                return Err(BlockError::Io);
            }
            let (s, e) = self.lba_span(lba, src.len())?;
            self.data[s..e].copy_from_slice(src);
            Ok(())
        }
        fn flush(&mut self) -> Result<(), BlockError> {
            Ok(())
        }
    }

    #[test]
    fn blk_loopback_1000_rounds_checksum_stable() {
        let mut dev = FakeBlock::new(512, 16 + 1000 * 16 + 8);
        let mut buf = vec![0u8; 8 * 512];
        let rep = loopback_probe(&mut dev, 1000, 8, &mut buf);
        assert!(rep.passed, "1000 轮回环必须全过，err={:?}", rep.err);
        assert_eq!(rep.write_sum, rep.read_sum);
        assert_eq!(rep.rounds, 1000);
    }

    #[test]
    fn blk_loopback_detects_corruption() {
        let mut dev = FakeBlock::new(512, 64);
        dev.corrupt = true;
        let mut buf = vec![0u8; 4 * 512];
        let rep = loopback_probe(&mut dev, 3, 4, &mut buf);
        assert!(!rep.passed);
        assert_eq!(rep.err, Some(BlockError::Io));
    }

    #[test]
    fn blk_loopback_propagates_write_error() {
        let mut dev = FakeBlock::new(512, 64);
        dev.fail_write = true;
        let mut buf = vec![0u8; 2 * 512];
        let rep = loopback_probe(&mut dev, 3, 2, &mut buf);
        assert_eq!(rep.err, Some(BlockError::Io));
        assert!(!rep.passed);
    }

    #[test]
    fn blk_range_rules_enforced() {
        let mut dev = FakeBlock::new(512, 32);
        // 越界。
        let mut buf = vec![0u8; 2 * 512];
        assert_eq!(
            dev.read_blocks(31, &mut buf),
            Err(BlockError::InvalidRange)
        );
        // 非整块。
        assert_eq!(
            dev.read_blocks(0, &mut buf[..100]),
            Err(BlockError::InvalidRange)
        );
        // 空请求。
        assert_eq!(
            dev.read_blocks(0, &mut []),
            Err(BlockError::InvalidRange)
        );
        // 写同理。
        assert_eq!(
            dev.write_blocks(33, &buf),
            Err(BlockError::InvalidRange)
        );
    }

    #[test]
    fn blk_fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }
}
