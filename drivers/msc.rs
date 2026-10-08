//! S4.2-A · USB 大容量存储（MSC）BOT+SCSI 传输层——AI-5 内核基建长线第二件
//! （2026-09-22，接 xHCI S4.1 之后）。
//!
//! **范围如实声明**：BOT（Bulk-Only Transport，USB MSC 规范 v1.0 §3）+
//! SCSI 透明指令集最小集：TEST_UNIT_READY / INQUIRY / READ_CAPACITY(10) /
//! READ(10) / WRITE(10) / REQUEST_SENSE（仅 FAILED 时取证一次）。不做的：
//! 多 LUN（恒 LUN0）、UAS、6 字节 READ(6)/WRITE(6)、可移除介质轮询、
//! 预取/缓存语义（flush 为受控 no-op，见 [`MscDev::flush`]）。
//!
//! **分层契约**：本文件是纯协议层——只认识「双向 bulk 管道」
//! ([`BulkPipe`])，不感知 xHCI 任何寄存器细节；xHCI 侧 ([`super::xhci`])
//! 提供管道实现并负责 DMA/环/事件。这样 BOT 状态机可以在宿主上用
//! 设备模拟器全链测试（CBW/CSW 逐字节、tag 配对、residue 记账、相位
//! 错误、短包），目标态换真管道即用，逻辑零改动。
//!
//! **BOT 帧序（规范 §3.2，串行）**：CBW(31B, OUT) → 数据阶段（方向由
//! CBW dCBWDataTransferLength 与标志决定，可无数据）→ CSW(13B, IN)。
//! tag 逐命令递增，CSW.bCSWTag 必须与 CBW 相等；residue =
//! dCBWDataTransferLength − 实际传输量。CSW 状态：0=PASS 1=FAIL
//! 2=PHASE_ERROR。任何一步不匹配 = [`BlockError::Io`]，绝不静默降级。
//!
//! **块语义**：块大小来自 READ CAPACITY(10)（本层不假设 512——写入
//! `block_size` 并在 [`BlockDevice`] 实现中如实上报）；单条 BOT 命令的
//! 数据段由 [`BulkPipe`] 分块（xHCI 侧 ≤4KiB DMA 缓冲），分块循环在
//! 本层内完成，对上层呈现整段读写的 [`BlockDevice`] 契约。

use super::blk::{BlockDevice, BlockError};

// ---- BOT 常量（USB MSC BOT 规范逐值同构）----------------------------------
/// CBW 签名 'USBC'（小端 0x43425355）。
pub const CBW_SIG: u32 = 0x4342_5355;
/// CSW 签名 'USBS'（小端 0x53425355）。
pub const CSW_SIG: u32 = 0x5342_5355;
pub const CBW_LEN: usize = 31;
pub const CSW_LEN: usize = 13;
/// CSW 状态：命令通过。
pub const CSW_PASSED: u8 = 0;
/// CSW 状态：命令失败（设备端报错，可 REQUEST_SENSE 取证）。
pub const CSW_FAILED: u8 = 1;
/// CSW 状态：相位错误（主机与设备对数据阶段认知不一致，唯一恢复=复位）。
pub const CSW_PHASE_ERROR: u8 = 2;
/// CBW bmCBWFlags：Data-In。
pub const CBW_FLAG_IN: u8 = 0x80;
/// CBW 最大 CDB 长度（16B；本层词汇表内 ≤10B）。
pub const CBW_CDB_MAX: usize = 16;

// ---- SCSI 操作码（SPC/SBC 最小集）----------------------------------------
pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
pub const SCSI_REQUEST_SENSE: u8 = 0x03;
pub const SCSI_INQUIRY: u8 = 0x12;
pub const SCSI_READ_CAPACITY10: u8 = 0x25;
pub const SCSI_READ10: u8 = 0x28;
pub const SCSI_WRITE10: u8 = 0x2A;

/// INQUIRY 标准返回长度。
pub const INQUIRY_LEN: u32 = 36;
/// REQUEST_SENSE 取证长度。
pub const SENSE_LEN: u32 = 18;

// ---- CDB 词汇表 -----------------------------------------------------------

/// 本层支持的 SCSI 命令（词汇表外一律在构造期拒绝，绝不发出未知 CDB）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cdb {
    TestUnitReady,
    Inquiry,
    ReadCapacity10,
    Read10 { lba: u32, blocks: u16 },
    Write10 { lba: u32, blocks: u16 },
    RequestSense,
}

impl Cdb {
    /// CDB 字节与规范长度（6 或 10 字节，逐字段对照 SPC/SBC）。
    pub fn bytes(&self) -> ([u8; 10], usize) {
        let mut b = [0u8; 10];
        match *self {
            Cdb::TestUnitReady => {
                b[0] = SCSI_TEST_UNIT_READY;
                (b, 6)
            }
            Cdb::RequestSense => {
                b[0] = SCSI_REQUEST_SENSE;
                b[4] = SENSE_LEN as u8;
                (b, 6)
            }
            Cdb::Inquiry => {
                b[0] = SCSI_INQUIRY;
                b[4] = INQUIRY_LEN as u8;
                (b, 6)
            }
            Cdb::ReadCapacity10 => {
                b[0] = SCSI_READ_CAPACITY10;
                (b, 10)
            }
            Cdb::Read10 { lba, blocks } => {
                b[0] = SCSI_READ10;
                b[2..6].copy_from_slice(&lba.to_be_bytes());
                b[7..9].copy_from_slice(&blocks.to_be_bytes());
                (b, 10)
            }
            Cdb::Write10 { lba, blocks } => {
                b[0] = SCSI_WRITE10;
                b[2..6].copy_from_slice(&lba.to_be_bytes());
                b[7..9].copy_from_slice(&blocks.to_be_bytes());
                (b, 10)
            }
        }
    }

    /// 数据阶段方向与期望长度；None = 无数据阶段。
    pub fn data_phase(&self, block_size: u32) -> Option<(Dir, u32)> {
        match *self {
            Cdb::TestUnitReady => None,
            Cdb::RequestSense => Some((Dir::In, SENSE_LEN)),
            Cdb::Inquiry => Some((Dir::In, INQUIRY_LEN)),
            Cdb::ReadCapacity10 => Some((Dir::In, 8)),
            Cdb::Read10 { blocks, .. } => Some((Dir::In, blocks as u32 * block_size)),
            Cdb::Write10 { blocks, .. } => Some((Dir::Out, blocks as u32 * block_size)),
        }
    }
}

/// 数据阶段方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    In,
    Out,
}

// ---- CBW/CSW 编解码（纯函数，逐字节锁定）----------------------------------

/// 编码 CBW：tag / 数据段长度 / 方向 / CDB（≤16B，本层词汇表 ≤10B）。
pub fn cbw_bytes(tag: u32, data_len: u32, dir_in: bool, cdb: &[u8]) -> [u8; CBW_LEN] {
    assert!(cdb.len() <= CBW_CDB_MAX, "CDB 超过 CBW 容量（规范 §3.2.1）");
    let mut b = [0u8; CBW_LEN];
    b[0..4].copy_from_slice(&CBW_SIG.to_le_bytes());
    b[4..8].copy_from_slice(&tag.to_le_bytes());
    b[8..12].copy_from_slice(&data_len.to_le_bytes());
    b[12] = if dir_in { CBW_FLAG_IN } else { 0 };
    b[13] = 0; // bCBWLUN：恒 LUN0
    b[14] = cdb.len() as u8;
    b[15..15 + cdb.len()].copy_from_slice(cdb);
    b
}

/// 解码 CSW：签名/tag 配对由调用方校验；这里只做结构提取。
pub struct Csw {
    pub tag: u32,
    pub residue: u32,
    pub status: u8,
}

/// 从 13 字节解析 CSW；签名不符 = None（结构性错误）。
pub fn parse_csw(b: &[u8; CSW_LEN]) -> Option<Csw> {
    let sig = u32::from_le_bytes(b[0..4].try_into().ok()?);
    if sig != CSW_SIG {
        return None;
    }
    Some(Csw {
        tag: u32::from_le_bytes(b[4..8].try_into().ok()?),
        residue: u32::from_le_bytes(b[8..12].try_into().ok()?),
        status: b[12],
    })
}

// ---- 管道抽象 -------------------------------------------------------------

/// 双向 bulk 管道（xHCI 侧实现；宿主模拟器同构实现）。
///
/// 契约：`bulk_out` 全量投递（长度即投递量；设备端短收 = 错误）；
/// `bulk_in` 请求 `out.len()` 字节，返回**实际**传输量（≤ out.len()，
/// 短包合法——BOT 数据段尾部短包是规范行为）。数据落点直接写 `out`。
pub trait BulkPipe {
    fn bulk_out(&mut self, buf: &[u8]) -> Result<(), BlockError>;
    fn bulk_in(&mut self, out: &mut [u8]) -> Result<usize, BlockError>;
}

// ---- 设备层 ---------------------------------------------------------------

/// 单 LUN MSC 设备：BOT 状态机 + 盘几何。`P: BulkPipe` 由承载层注入。
pub struct MscDev<P: BulkPipe> {
    pipe: P,
    tag: u32,
    /// 块大小（READ CAPACITY 读出；<=0 不可能——init 失败即不构造）。
    pub block_size: u32,
    /// 末块 LBA（READ CAPACITY 返回值语义：总块数 = 值 + 1）。
    pub last_lba: u64,
    /// REQUEST_SENSE 取证计数（验收证据）。
    pub sense_count: u32,
    /// INQUIRY 厂商/产品串（8+16B，ASCII 截断；取证用）。
    pub vendor: [u8; 8],
    pub product: [u8; 16],
}

/// 单条 BOT 命令的数据段分块上限（对齐 DMA 缓冲；xHCI 侧 4KiB 帧）。
pub const CHUNK_MAX: usize = 4096;

impl<P: BulkPipe> MscDev<P> {
    /// 初始化序列：TEST_UNIT_READY（失败取证一次后重试）→ INQUIRY →
    /// READ CAPACITY(10)。任何一步硬失败如实上抛，绝不带病构造。
    pub fn init(mut pipe: P) -> Result<MscDev<P>, BlockError> {
        let mut dev = MscDev {
            pipe,
            tag: 0,
            block_size: 512,
            last_lba: 0,
            sense_count: 0,
            vendor: [0x20; 8],
            product: [0x20; 16],
        };
        // TEST_UNIT_READY：介质可能需要短暂就绪；两次 + 一次 SENSE 取证。
        for attempt in 0..2u8 {
            let (csw, _) = dev.command(Cdb::TestUnitReady, None, &mut [])?;
            if csw == CSW_PASSED {
                break;
            }
            if attempt == 1 {
                return Err(BlockError::Io);
            }
            let _ = dev.request_sense();
        }
        // INQUIRY：36B 标准返回。
        let mut inq = [0u8; 36];
        let (csw, got) = dev.command(Cdb::Inquiry, None, &mut inq)?;
        if csw != CSW_PASSED || got < 8 {
            return Err(BlockError::Io);
        }
        dev.vendor.copy_from_slice(&inq[8..16]);
        dev.product.copy_from_slice(&inq[16..32]);
        // READ CAPACITY(10)：8B = [last_lba BE u32][block_size BE u32]。
        let mut cap = [0u8; 8];
        let (csw, got) = dev.command(Cdb::ReadCapacity10, None, &mut cap)?;
        if csw != CSW_PASSED || got != 8 {
            return Err(BlockError::Io);
        }
        let last = u32::from_be_bytes(cap[0..4].try_into().unwrap_or([0; 4]));
        let bs = u32::from_be_bytes(cap[4..8].try_into().unwrap_or([0; 4]));
        if bs == 0 || !bs.is_power_of_two() || bs > CHUNK_MAX as u32 {
            // 块大小非法或超过单块 DMA 缓冲——如实拒绝（不假设 512 兜底）。
            return Err(BlockError::Unsupported);
        }
        dev.block_size = bs;
        dev.last_lba = last as u64;
        Ok(dev)
    }

    /// 已知几何时跳过 init 序列（枚举期已探测过几何；调用方保证参数
    /// 与介质一致——几何失信的后果在 read/write 的越界拒绝里兜住）。
    pub fn from_parts(pipe: P, block_size: u32, last_lba: u64) -> MscDev<P> {
        MscDev {
            pipe,
            tag: 0,
            block_size,
            last_lba,
            sense_count: 0,
            vendor: [0x20; 8],
            product: [0x20; 16],
        }
    }

    /// REQUEST_SENSE 取证（FAILED 后调用；结果只记账不恢复）。
    fn request_sense(&mut self) -> Result<[u8; 18], BlockError> {
        self.sense_count += 1;
        let mut sense = [0u8; 18];
        let (csw, _) = self.command(Cdb::RequestSense, None, &mut sense)?;
        if csw != CSW_PASSED {
            return Err(BlockError::Io);
        }
        Ok(sense)
    }

    /// 容量：(块数, 块大小)。
    pub fn capacity(&self) -> (u64, u32) {
        (self.last_lba + 1, self.block_size)
    }

    /// 执行一条 BOT 命令。`data`：
    /// - In 方向：落点缓冲（长度 = 期望；短包返回实际量）；
    /// - Out 方向：数据源（全量投递，分块 ≤CHUNK_MAX）；
    /// - None：数据长度 0。
    /// 返回 (CSW status, 数据阶段实际传输量)。tag/签名/相位失配一律 Io。
    fn command(
        &mut self,
        cdb: Cdb,
        out_src: Option<&[u8]>,
        in_dst: &mut [u8],
    ) -> Result<(u8, usize), BlockError> {
        let (cdb_bytes, cdb_len) = cdb.bytes();
        let phase = cdb.data_phase(self.block_size);
        let (dir, dlen) = match phase {
            Some((Dir::In, n)) => {
                if in_dst.len() < n as usize {
                    // IN 落点必须能装下期望长度（调用方按块对齐分配）。
                    return Err(BlockError::InvalidRange);
                }
                (true, n)
            }
            Some((Dir::Out, n)) => {
                let src_len = out_src.map_or(0, |s| s.len()) as u32;
                if src_len != n {
                    return Err(BlockError::InvalidRange);
                }
                (false, n)
            }
            None => (false, 0),
        };
        self.tag = self.tag.wrapping_add(1);
        let tag = self.tag;
        let cbw = cbw_bytes(tag, dlen, dir, &cdb_bytes[..cdb_len]);
        self.pipe.bulk_out(&cbw)?;
        let mut transferred = 0usize;
        // **提前到达的 CSW**：设备端命令失败时可以不给数据段直接回 CSW
        // （真实硬件表现 = 数据端点 stall 后紧跟 CSW；本最小栈不建 halt
        // 恢复路径，如实声明）。数据段首块若恰为 13B 合法 CSW 且 tag
        // 配对 → 按其状态直接收尾，不再读常规 CSW；FAILED 由调用方走
        // REQUEST_SENSE 取证。非首块不识别（数据段中途不可能合法出现 CSW）。
        let mut early_csw: Option<u8> = None;
        match phase {
            Some((Dir::In, _)) => {
                let mut off = 0usize;
                while off < dlen as usize {
                    let want = (dlen as usize - off).min(CHUNK_MAX);
                    let got = self.pipe.bulk_in(&mut in_dst[off..off + want])?;
                    if got == 0 {
                        return Err(BlockError::Io); // 数据阶段中途零字节=协议破坏
                    }
                    if off == 0 && got == CSW_LEN {
                        if let Some(c) = parse_csw(
                            in_dst[..CSW_LEN].try_into().expect("13B 切片"),
                        ) {
                            if c.tag == tag {
                                early_csw = Some(c.status);
                                break;
                            }
                            return Err(BlockError::Io); // tag 失配
                        }
                    }
                    off += got;
                    transferred = off;
                    if got < want {
                        break; // 短包=数据段提前结束（设备端决定），余量记 residue
                    }
                }
            }
            Some((Dir::Out, _)) => {
                let src = out_src.unwrap_or(&[]);
                let mut off = 0usize;
                while off < src.len() {
                    let end = (off + CHUNK_MAX).min(src.len());
                    self.pipe.bulk_out(&src[off..end])?;
                    transferred = end;
                    off = end;
                }
            }
            None => {}
        }
        // CSW（或已由提前识别路径给出）。
        if let Some(status) = early_csw {
            if status == CSW_PHASE_ERROR {
                return Err(BlockError::Io);
            }
            return Ok((status, 0));
        }
        let mut csw_buf = [0u8; CSW_LEN];
        let got = self.pipe.bulk_in(&mut csw_buf)?;
        if got != CSW_LEN {
            return Err(BlockError::Io);
        }
        let csw = parse_csw(&csw_buf).ok_or(BlockError::Io)?;
        if csw.tag != tag {
            return Err(BlockError::Io); // tag 失配：BOT 串行契约破坏
        }
        if csw.status == CSW_PHASE_ERROR {
            return Err(BlockError::Io);
        }
        Ok((csw.status, transferred))
    }

    /// 读块：`dst.len()` 必须是 block_size 整数倍；按 ≤CHUNK_MAX 分段。
    pub fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
        let bs = self.block_size as usize;
        if dst.is_empty() || dst.len() % bs != 0 {
            return Err(BlockError::InvalidRange);
        }
        let nblocks = (dst.len() / bs) as u16;
        if lba + (nblocks as u64) > self.last_lba + 1 {
            return Err(BlockError::InvalidRange);
        }
        let mut off = 0usize;
        let mut remaining = nblocks;
        let mut cur = lba as u32;
        while remaining > 0 {
            let step = remaining.min((CHUNK_MAX / bs) as u16);
            let seg = step as usize * bs;
            let (status, got) = self.command(
                Cdb::Read10 { lba: cur, blocks: step },
                None,
                &mut dst[off..off + seg],
            )?;
            if status != CSW_PASSED {
                let _ = self.request_sense();
                return Err(BlockError::Io);
            }
            if got != seg {
                return Err(BlockError::Io); // 读短于承诺长度=盘几何失信
            }
            off += seg;
            cur += step as u32;
            remaining -= step;
        }
        Ok(())
    }

    /// 写块：`src.len()` 必须是 block_size 整数倍。
    pub fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
        let bs = self.block_size as usize;
        if src.is_empty() || src.len() % bs != 0 {
            return Err(BlockError::InvalidRange);
        }
        let nblocks = (src.len() / bs) as u16;
        if lba + (nblocks as u64) > self.last_lba + 1 {
            return Err(BlockError::InvalidRange);
        }
        let mut off = 0usize;
        let mut remaining = nblocks;
        let mut cur = lba as u32;
        while remaining > 0 {
            let step = remaining.min((CHUNK_MAX / bs) as u16);
            let seg = step as usize * bs;
            let (status, _) = self.command(
                Cdb::Write10 { lba: cur, blocks: step },
                Some(&src[off..off + seg]),
                &mut [],
            )?;
            if status != CSW_PASSED {
                let _ = self.request_sense();
                return Err(BlockError::Io);
            }
            off += seg;
            cur += step as u32;
            remaining -= step;
        }
        Ok(())
    }

    /// 落盘屏障：本层无缓存（写命令直通管道），缓存一致性由介质端
    /// WRITE10 完成语义保证；SCSI SYNCHRONIZE CACHE(10) 词汇表外，
    /// 如实声明为受控 no-op（与 [`BlockDevice`] 的 flush 契约一致：
    /// "flush 返回后先前写入可读"——BOT 无写缓存路径下恒成立）。
    pub fn flush(&mut self) -> Result<(), BlockError> {
        Ok(())
    }
}

impl<P: BulkPipe> BlockDevice for MscDev<P> {
    fn block_size(&self) -> u32 {
        self.block_size
    }
    fn capacity_blocks(&self) -> u64 {
        self.last_lba + 1
    }
    fn read_blocks(&mut self, lba: u64, dst: &mut [u8]) -> Result<(), BlockError> {
        MscDev::read_blocks(self, lba, dst)
    }
    fn write_blocks(&mut self, lba: u64, src: &[u8]) -> Result<(), BlockError> {
        MscDev::write_blocks(self, lba, src)
    }
    fn flush(&mut self) -> Result<(), BlockError> {
        MscDev::flush(self)
    }
}

// ---------------------------------------------------------------------------
// 宿主模拟器（tests only）：BOT 设备行为学模型——内存盘 + CBW→数据→CSW
// 逐字节执行，注入面：tag 错配 / 相位错误 / 介质错误 / 数据段短包。
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    /// BOT 设备相位（模拟器内部）。
    enum Phase {
        ExpectCbw,
        DataIn { data: Vec<u8>, pos: usize },
        DataOut { total: usize, got: Vec<u8> },
        SendCsw { pending: Option<[u8; CSW_LEN]> },
    }

    struct SimMsc {
        disk: Vec<u8>,
        bs: u32,
        last_lba: u32,
        cur_lba: u32,
        phase: Phase,
        cur_tag: u32,
        tag_delta: u32,
        phase_error: bool,
        in_cap: Option<usize>,
        media_fail_lba: Option<u32>,
    }

    impl SimMsc {
        fn new(blocks: usize, bs: u32) -> SimMsc {
            SimMsc {
                disk: vec![0u8; blocks * bs as usize],
                bs,
                last_lba: blocks as u32 - 1,
                cur_lba: 0,
                phase: Phase::ExpectCbw,
                cur_tag: 0,
                tag_delta: 0,
                phase_error: false,
                in_cap: None,
                media_fail_lba: None,
            }
        }

        fn csw_bytes(&self, tag: u32, residue: u32, status: u8) -> [u8; CSW_LEN] {
            let mut b = [0u8; CSW_LEN];
            b[0..4].copy_from_slice(&CSW_SIG.to_le_bytes());
            b[4..8].copy_from_slice(&(tag.wrapping_add(self.tag_delta)).to_le_bytes());
            b[8..12].copy_from_slice(&residue.to_le_bytes());
            b[12] = status;
            b
        }

        fn exec_cdb(&mut self, cdb: &[u8], tag: u32) {
            if self.phase_error {
                let c = self.csw_bytes(tag, 0, CSW_PHASE_ERROR);
                self.phase = Phase::SendCsw { pending: Some(c) };
                return;
            }
            let fail = self.media_fail_lba.map_or(false, |bad| {
                (cdb[0] == SCSI_READ10 || cdb[0] == SCSI_WRITE10)
                    && u32::from_be_bytes(cdb[2..6].try_into().unwrap()) == bad
            });
            if fail {
                let c = self.csw_bytes(tag, 0, CSW_FAILED);
                self.phase = Phase::SendCsw { pending: Some(c) };
                return;
            }
            match cdb[0] {
                SCSI_TEST_UNIT_READY => {
                    let c = self.csw_bytes(tag, 0, CSW_PASSED);
                    self.phase = Phase::SendCsw { pending: Some(c) };
                }
                SCSI_REQUEST_SENSE => {
                    let mut sense = vec![0x70u8, 0, 0x06, 0, 0, 0, 0, 0x0A];
                    sense.resize(SENSE_LEN as usize, 0);
                    self.phase = Phase::DataIn { data: sense, pos: 0 };
                }
                SCSI_INQUIRY => {
                    let mut d = vec![0u8; 36];
                    d[0] = 0x00;
                    d[4] = 0x21;
                    d[8..16].copy_from_slice(b"VARIXMSD");
                    d[16..32].copy_from_slice(b"SIM-BOT-DISK001 ");
                    self.phase = Phase::DataIn { data: d, pos: 0 };
                }
                SCSI_READ_CAPACITY10 => {
                    let mut d = [0u8; 8];
                    d[0..4].copy_from_slice(&self.last_lba.to_be_bytes());
                    d[4..8].copy_from_slice(&self.bs.to_be_bytes());
                    self.phase = Phase::DataIn { data: d.to_vec(), pos: 0 };
                }
                SCSI_READ10 => {
                    let lba = u32::from_be_bytes(cdb[2..6].try_into().unwrap());
                    let n = u16::from_be_bytes(cdb[7..9].try_into().unwrap());
                    let start = lba as usize * self.bs as usize;
                    let end = start + n as usize * self.bs as usize;
                    let data = self.disk[start..end].to_vec();
                    self.phase = Phase::DataIn { data, pos: 0 };
                }
                SCSI_WRITE10 => {
                    let lba = u32::from_be_bytes(cdb[2..6].try_into().unwrap());
                    let n = u16::from_be_bytes(cdb[7..9].try_into().unwrap());
                    self.cur_lba = lba;
                    self.phase = Phase::DataOut { total: n as usize * self.bs as usize, got: Vec::new() };
                }
                _ => {
                    let c = self.csw_bytes(tag, 0, CSW_FAILED);
                    self.phase = Phase::SendCsw { pending: Some(c) };
                }
            }
        }
    }

    impl BulkPipe for SimMsc {
        fn bulk_out(&mut self, buf: &[u8]) -> Result<(), BlockError> {
            // stall 丢弃语义：命令已失败等待发 CSW 期间到达的 OUT 数据
            // 直接丢弃（相位不动、CSW 保留）——必须先探测再 replace。
            if matches!(self.phase, Phase::SendCsw { .. }) {
                return Ok(());
            }
            match std::mem::replace(&mut self.phase, Phase::ExpectCbw) {
                Phase::ExpectCbw => {
                    assert_eq!(buf.len(), CBW_LEN);
                    let sig = u32::from_le_bytes(buf[0..4].try_into().unwrap());
                    assert_eq!(sig, CBW_SIG, "CBW 签名必须正确");
                    let tag = u32::from_le_bytes(buf[4..8].try_into().unwrap());
                    let cdb_len = buf[14] as usize;
                    let cdb = buf[15..15 + cdb_len].to_vec();
                    self.cur_tag = tag;
                    self.exec_cdb(&cdb, tag);
                    Ok(())
                }
                Phase::DataOut { total, mut got } => {
                    got.extend_from_slice(buf);
                    if got.len() >= total {
                        let start = self.cur_lba as usize * self.bs as usize;
                        let end = start + total;
                        self.disk[start..end].copy_from_slice(&got[..total]);
                        let c = self.csw_bytes(self.cur_tag, 0, CSW_PASSED);
                        self.phase = Phase::SendCsw { pending: Some(c) };
                    } else {
                        self.phase = Phase::DataOut { total, got };
                    }
                    Ok(())
                }
                // （SendCsw 到达的 OUT 已在函数头按 stall 丢弃处理。）
                _ => Err(BlockError::Io),
            }
        }

        fn bulk_in(&mut self, out: &mut [u8]) -> Result<usize, BlockError> {
            let progress: Option<(Vec<u8>, bool, u32)> = match &mut self.phase {
                Phase::DataIn { data, pos } => {
                    let want = out.len().min(data.len() - *pos).min(self.in_cap.unwrap_or(usize::MAX));
                    let seg = data[*pos..*pos + want].to_vec();
                    *pos += want;
                    let residue = (data.len() - *pos) as u32;
                    Some((seg, *pos >= data.len(), residue))
                }
                _ => None,
            };
            if let Some((seg, done, residue)) = progress {
                out[..seg.len()].copy_from_slice(&seg);
                if done {
                    let c = self.csw_bytes(self.cur_tag, residue, CSW_PASSED);
                    self.phase = Phase::SendCsw { pending: Some(c) };
                }
                return Ok(seg.len());
            }
            match &mut self.phase {
                Phase::SendCsw { pending } => {
                    let c = pending.take().unwrap();
                    out[..CSW_LEN].copy_from_slice(&c);
                    self.phase = Phase::ExpectCbw;
                    Ok(CSW_LEN)
                }
                _ => Err(BlockError::Io),
            }
        }
    }

    // ---- CBW/CSW/CDB 编解码 ------------------------------------------------

    #[test]
    fn msc_cbw_read10_golden_bytes() {
        let (cdb, len) = Cdb::Read10 { lba: 0x1234_5678, blocks: 0x0008 }.bytes();
        assert_eq!(len, 10);
        let cbw = cbw_bytes(0xDEAD_BEEF, 0x1000, true, &cdb[..len]);
        assert_eq!(&cbw[0..4], &CBW_SIG.to_le_bytes());
        assert_eq!(&cbw[4..8], &0xDEADBEEFu32.to_le_bytes());
        assert_eq!(&cbw[8..12], &0x1000u32.to_le_bytes());
        assert_eq!(cbw[12], CBW_FLAG_IN);
        assert_eq!(cbw[13], 0, "恒 LUN0");
        assert_eq!(cbw[14], 10);
        assert_eq!(cbw[15], SCSI_READ10);
        assert_eq!(&cbw[17..21], &0x12345678u32.to_be_bytes());
        assert_eq!(&cbw[22..24], &8u16.to_be_bytes());
        let (cdb, len) = Cdb::Write10 { lba: 1, blocks: 1 }.bytes();
        let cbw = cbw_bytes(1, 512, false, &cdb[..len]);
        assert_eq!(cbw[12], 0, "OUT 方向标志位为 0");
    }

    #[test]
    fn msc_csw_parse_and_cdb_fields() {
        let mut c = [0u8; CSW_LEN];
        assert!(parse_csw(&c).is_none(), "签名不符必须拒绝");
        c[0..4].copy_from_slice(&CSW_SIG.to_le_bytes());
        c[4..8].copy_from_slice(&7u32.to_le_bytes());
        c[8..12].copy_from_slice(&3u32.to_le_bytes());
        c[12] = CSW_FAILED;
        let csw = parse_csw(&c).unwrap();
        assert_eq!((csw.tag, csw.residue, csw.status), (7, 3, CSW_FAILED));
        let (b, l) = Cdb::TestUnitReady.bytes();
        assert_eq!((b[0], l), (SCSI_TEST_UNIT_READY, 6));
        let (b, l) = Cdb::ReadCapacity10.bytes();
        assert_eq!((b[0], l), (SCSI_READ_CAPACITY10, 10));
        let (b, l) = Cdb::Write10 { lba: 2, blocks: 3 }.bytes();
        assert_eq!(l, 10);
        assert_eq!(u32::from_be_bytes(b[2..6].try_into().unwrap()), 2);
        assert_eq!(u16::from_be_bytes(b[7..9].try_into().unwrap()), 3);
    }

    // ---- 全链回环（init→写→读→校验）----------------------------------------

    #[test]
    fn msc_init_and_full_loopback() {
        let sim = SimMsc::new(64, 512);
        let mut dev = MscDev::init(sim).expect("BOT 初始化必须成功");
        assert_eq!(dev.capacity(), (64, 512));
        assert!(dev.vendor.starts_with(b"VARIXMSD"));
        let mut pattern = [0u8; 40 * 512];
        for (i, b) in pattern.iter_mut().enumerate() {
            *b = (i * 7 + 3) as u8;
        }
        dev.write_blocks(0, &pattern).expect("整段写必须成功");
        dev.write_blocks(48, &pattern[..4 * 512]).expect("尾段写必须成功");
        let mut back = [0u8; 40 * 512];
        dev.read_blocks(0, &mut back).expect("整段读必须成功");
        assert_eq!(pattern, back, "读回必须逐字节一致");
        let mut tail = [0u8; 4 * 512];
        dev.read_blocks(48, &mut tail).expect("尾段读必须成功");
        assert_eq!(&pattern[..tail.len()], &tail);
        assert_eq!(dev.read_blocks(60, &mut [0u8; 8 * 512]), Err(BlockError::InvalidRange));
        assert_eq!(dev.read_blocks(0, &mut [0u8; 100]), Err(BlockError::InvalidRange));
        assert_eq!(dev.write_blocks(0, &[0u8; 100]), Err(BlockError::InvalidRange));
        assert!(dev.flush().is_ok());
    }

    #[test]
    fn msc_tag_mismatch_rejected() {
        let mut sim = SimMsc::new(16, 512);
        sim.tag_delta = 1;
        assert!(matches!(MscDev::init(sim), Err(BlockError::Io)));
    }

    #[test]
    fn msc_phase_error_rejected() {
        let mut sim = SimMsc::new(16, 512);
        sim.phase_error = true;
        assert!(matches!(MscDev::init(sim), Err(BlockError::Io)));
    }

    #[test]
    fn msc_media_failure_raises_sense_then_io() {
        let mut sim = SimMsc::new(32, 512);
        sim.media_fail_lba = Some(10);
        let mut dev = MscDev::init(sim).unwrap();
        let mut buf = [0u8; 512];
        assert_eq!(dev.read_blocks(10, &mut buf), Err(BlockError::Io));
        assert_eq!(dev.write_blocks(10, &[0u8; 512]), Err(BlockError::Io));
        assert_eq!(dev.sense_count, 2, "FAILED 路径必须各取证一次");
        // 正常 LBA 不受影响。
        assert!(dev.write_blocks(0, &[0xAB; 512]).is_ok());
        let mut chk = [0u8; 512];
        assert!(dev.read_blocks(0, &mut chk).is_ok());
        assert!(chk.iter().all(|&b| b == 0xAB));
        assert!(dev.write_blocks(9, &[0xCD; 512]).is_ok());
    }

    #[test]
    fn msc_non_512_block_size_roundtrip() {
        let sim = SimMsc::new(16, 4096);
        let mut dev = MscDev::init(sim).unwrap();
        assert_eq!(dev.block_size(), 4096);
        assert_eq!(dev.capacity_blocks(), 16);
        let mut buf = [0x5Au8; 4096];
        assert!(dev.write_blocks(3, &buf).is_ok());
        buf.fill(0);
        assert!(dev.read_blocks(3, &mut buf).is_ok());
        assert!(buf.iter().all(|&b| b == 0x5A));
    }

    #[test]
    fn msc_data_in_short_mid_transfer_is_protocol_violation() {
        // 设备在数据段中途回短包但继续发数据（未按承诺给满）→ 主机读 CSW
        // 收到的是数据 → 结构性错误，必须 Io 而非静默。
        let mut sim = SimMsc::new(16, 512);
        sim.in_cap = Some(4);
        let r = MscDev::init(sim);
        assert!(matches!(r, Err(BlockError::Io)));
    }
}

