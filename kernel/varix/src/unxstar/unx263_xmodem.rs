//! unx263_xmodem ↔ UNX-F0263 串口 XModem-CRC 接收器（镜像导入只读通道）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0263，UNX-A1-B14）：
//! 「串口 XModem-CRC 接收器（镜像导入只读通道）| 300 | 已深化 |
//!  UNX-F0263-J1 1MB 接收零丢包（CRC 过），数据只入内存审计区零执行（安全边界）」
//!
//! 设计要点（全部围绕串口镜像导入的真实协议状态）：
//! 1. **XModem-CRC 协议闭式**：SOH(0x01) + 块号 + 块号反码 + 128B 载荷 +
//!    CRC16-CCITT（XModem 变体：初值 0x0000、多项式 0x1021、大端收发）；
//!    EOT(0x04) 结束；ACK(0x06)/NAK(0x15) 应答。协议状态机逐态显性。
//! 2. **1MB = 8192 块零丢包**：块号 1..=8192 连续无缺口（缺口即丢包 =
//!    `SequenceGap` 立案）；重复块（发送方超时重发）幂等吸收不重复入账。
//! 3. **安全边界（本单红线）**：接收数据**只入内存审计区**，审计区
//!    `executable` 标志恒为 false 且类型上无法置真（`mark_executable`
//!    返回 `Err(ExecForbidden)`）——镜像内容再合法也不从串口通道执行。
//! 4. **CRC 门**：块 CRC 不过 → NAK 重试（重试计数留痕）；重试超限
//!    显性 `RetryExhausted`，不静默跳块。
//! 5. **纯函数内核**：串口字节流以显式注入逐字节喂入，零 IO 零墙钟。
//!
//! 诊断码：`XModemCode` 十六进制段 0x94（0x9400..=0x940F）独占。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本号（进指纹，改动即显性）。
pub const UNX263_XMODEM_VERSION: &str = "unx263-xmodem-v1";

/// 块载荷长度（XModem 标准定长）。
pub const BLOCK_PAYLOAD: usize = 128;

/// 目标接收量（锚点原文：1MB）。
pub const TARGET_BYTES: u64 = 1024 * 1024;

/// 总块数（1MB / 128B = 8192）。
pub const TARGET_BLOCKS: u32 = (TARGET_BYTES / BLOCK_PAYLOAD as u64) as u32;

/// 单块重试上限（发送方 NAK 重发预算）。
pub const RETRY_LIMIT: u32 = 8;

/// 协议控制字节闭集。
pub const SOH: u8 = 0x01;
pub const EOT: u8 = 0x04;
pub const ACK: u8 = 0x06;
pub const NAK: u8 = 0x15;

/// CRC16-CCITT（XModem 变体：初值 0x0000，多项式 0x1021，MSB 先行）。
pub fn crc16_xmodem(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0x0000;
    for b in bytes.iter() {
        crc ^= (*b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// 接收状态机闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RxState {
    /// 等待起始（发送方尚未被 NAK 拉起）
    Idle,
    /// 逐块接收中
    Receiving,
    /// 收到 EOT 待确认
    Ending,
    /// 全部完成（终态）
    Done,
    /// 显性失败（终态）
    Aborted,
}

impl RxState {
    /// 是否终态。
    pub const fn is_terminal(self) -> bool {
        matches!(self, RxState::Done | RxState::Aborted)
    }
}

/// 诊断码闭集（0x94 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XModemCode {
    /// 0x9400 块号缺口（丢包）
    SequenceGap,
    /// 0x9401 块号与反码不配对
    ComplementBad,
    /// 0x9402 CRC 不符
    CrcBad,
    /// 0x9403 重试超限
    RetryExhausted,
    /// 0x9404 执行标志置真被拒（安全红线）
    ExecForbidden,
    /// 0x9405 协议字节错位（SOH 外的非法起始）
    ProtocolFraming,
    /// 0x9406 审计区容量不足
    AuditOverflow,
    /// 0x9407 终态后再喂字节
    TerminalFeed,
    /// 0x9408 接收完成（成功码）
    Complete,
}

impl XModemCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            XModemCode::SequenceGap => 0x9400,
            XModemCode::ComplementBad => 0x9401,
            XModemCode::CrcBad => 0x9402,
            XModemCode::RetryExhausted => 0x9403,
            XModemCode::ExecForbidden => 0x9404,
            XModemCode::ProtocolFraming => 0x9405,
            XModemCode::AuditOverflow => 0x9406,
            XModemCode::TerminalFeed => 0x9407,
            XModemCode::Complete => 0x9408,
        }
    }

    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            XModemCode::SequenceGap => "XMODEM-SEQ-GAP",
            XModemCode::ComplementBad => "XMODEM-COMPLEMENT-BAD",
            XModemCode::CrcBad => "XMODEM-CRC-BAD",
            XModemCode::RetryExhausted => "XMODEM-RETRY-EXHAUSTED",
            XModemCode::ExecForbidden => "XMODEM-EXEC-FORBIDDEN",
            XModemCode::ProtocolFraming => "XMODEM-PROTOCOL-FRAMING",
            XModemCode::AuditOverflow => "XMODEM-AUDIT-OVERFLOW",
            XModemCode::TerminalFeed => "XMODEM-TERMINAL-FEED",
            XModemCode::Complete => "XMODEM-COMPLETE",
        }
    }

    /// 段独占双写互斥（0x94 段）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9400 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            XModemCode::SequenceGap => "块号缺口：序列不连续即丢包，镜像不完整禁止进入下一环节",
            XModemCode::ComplementBad => "块号与反码不配对：帧内自校验失败，按坏帧 NAK 处理",
            XModemCode::CrcBad => "CRC 不符：载荷在位但内容已损，NAK 请求重发",
            XModemCode::RetryExhausted => "重试超限：同一块重发预算耗尽，显性中止（不静默跳块）",
            XModemCode::ExecForbidden => "执行标志置真被拒：串口导入数据只入审计区，零执行（安全边界）",
            XModemCode::ProtocolFraming => "协议字节错位：非 SOH/EOT 的起始字节，帧界失步",
            XModemCode::AuditOverflow => "审计区容量不足：接收量超出审计区预算，显性中止",
            XModemCode::TerminalFeed => "终态后再喂字节：状态机已收尾，多余字节按协议噪声丢弃并留痕",
            XModemCode::Complete => "1MB 接收零丢包且 CRC 全过，数据全部入审计区（零执行）",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx263_xmodem.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 接收错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XModemError {
    SequenceGap { expected: u8, got: u8 },
    ComplementBad { block: u8 },
    CrcBad { block: u8 },
    RetryExhausted { block: u8, retries: u32 },
    ExecForbidden,
    ProtocolFraming { byte: u8 },
    AuditOverflow { cap: u64 },
    TerminalFeed,
}

impl XModemError {
    /// 映射诊断码。
    pub const fn code(self) -> XModemCode {
        match self {
            XModemError::SequenceGap { .. } => XModemCode::SequenceGap,
            XModemError::ComplementBad { .. } => XModemCode::ComplementBad,
            XModemError::CrcBad { .. } => XModemCode::CrcBad,
            XModemError::RetryExhausted { .. } => XModemCode::RetryExhausted,
            XModemError::ExecForbidden => XModemCode::ExecForbidden,
            XModemError::ProtocolFraming { .. } => XModemCode::ProtocolFraming,
            XModemError::AuditOverflow { .. } => XModemCode::AuditOverflow,
            XModemError::TerminalFeed => XModemCode::TerminalFeed,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 内存审计区（只收数据，零执行；executable 标志类型上无法置真）。
#[derive(Clone, Debug)]
pub struct AuditRegion {
    bytes: Vec<u8>,
    /// 恒为 false——本字段是"可执行否"的唯一真源，且不提供置真路径
    executable: bool,
    /// 尝试置真计数（安全审计留痕）
    exec_attempts: u32,
}

impl AuditRegion {
    /// 空审计区。
    pub fn new(cap: u64) -> Self {
        AuditRegion {
            bytes: Vec::new(),
            executable: false,
            exec_attempts: 0,
        }
    }

    /// 追加数据（容量门：超预算显性拒）。
    pub fn append(&mut self, data: &[u8], cap: u64) -> Result<(), XModemError> {
        let new_len = self.bytes.len() as u64 + data.len() as u64;
        if new_len > cap {
            return Err(XModemError::AuditOverflow { cap });
        }
        self.bytes.extend_from_slice(data);
        Ok(())
    }

    /// 可执行否（恒 false——安全边界真源）。
    pub const fn is_executable(&self) -> bool {
        self.executable
    }

    /// 置真尝试（**永远失败**并计数——安全边界在类型上兑现）。
    pub fn mark_executable(&mut self) -> Result<(), XModemError> {
        self.exec_attempts = self.exec_attempts.wrapping_add(1);
        Err(XModemError::ExecForbidden)
    }

    /// 置真尝试计数。
    pub const fn exec_attempts(&self) -> u32 {
        self.exec_attempts
    }

    /// 已收字节数。
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// 全量 CRC32（对拍审计用）。
    pub fn crc32(&self) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for b in self.bytes.iter() {
            crc ^= *b as u32;
            for _ in 0..8 {
                let lsb = crc & 1;
                crc >>= 1;
                if lsb == 1 {
                    crc ^= 0xEDB8_8320;
                }
            }
        }
        !crc
    }

    /// 字节切片读（越界安全）。
    pub fn byte_at(&self, idx: usize) -> Option<u8> {
        self.bytes.get(idx).copied()
    }
}

/// 接收器（协议状态机 + 块账 + 审计区）。
#[derive(Clone, Debug)]
pub struct XModemReceiver {
    pub state: RxState,
    /// 下一期望块号（模 256 语义；XModem 块号 8 位循环，1..=255 后回 0）
    expect: u8,
    /// 已收块计数（连续无缺口口径）
    pub blocks_done: u32,
    /// 当前块重试计数
    retries: u32,
    /// 累计 CRC 失败次数
    pub crc_failures: u32,
    /// 重复块吸收次数（幂等证据）
    pub duplicates_absorbed: u32,
    /// 审计区
    pub audit: AuditRegion,
    /// 审计区容量
    pub cap: u64,
}

impl XModemReceiver {
    /// 新接收器（审计容量按字节给）。
    pub fn new(cap: u64) -> Self {
        XModemReceiver {
            state: RxState::Idle,
            expect: 1,
            blocks_done: 0,
            retries: 0,
            crc_failures: 0,
            duplicates_absorbed: 0,
            audit: AuditRegion::new(cap),
            cap,
        }
    }

    /// 当前期望块号。
    pub const fn expect_block(&self) -> u8 {
        self.expect
    }

    /// 喂一帧（SOH+块号+反码+128B+CRC16；返回应答字节）。
    ///
    /// 状态机只在 Receiving 态接受数据帧；Idle 态收到 SOH 即进入 Receiving。
    pub fn feed_block(&mut self, block_no: u8, complement: u8, payload: &[u8], crc: u16) -> Result<u8, XModemError> {
        if self.state.is_terminal() {
            return Err(XModemError::TerminalFeed);
        }
        // 帧校验一：反码配对
        if complement != !block_no {
            self.state = RxState::Aborted;
            return Err(XModemError::ComplementBad { block: block_no });
        }
        // 重复块：与上一已收块号相同 = 发送方超时重发，幂等吸收
        let prev = if self.blocks_done == 0 { 0 } else { (self.expect.wrapping_sub(1)) };
        if self.blocks_done > 0 && block_no == prev {
            self.duplicates_absorbed = self.duplicates_absorbed.wrapping_add(1);
            return Ok(ACK);
        }
        // 序列门：必须等于期望块号（缺口 = 丢包）
        if block_no != self.expect {
            self.state = RxState::Aborted;
            return Err(XModemError::SequenceGap { expected: self.expect, got: block_no });
        }
        // 帧校验二：载荷定长
        if payload.len() != BLOCK_PAYLOAD {
            self.state = RxState::Aborted;
            return Err(XModemError::ProtocolFraming { byte: SOH });
        }
        // 帧校验三：CRC 门
        if crc16_xmodem(payload) != crc {
            self.crc_failures = self.crc_failures.wrapping_add(1);
            self.retries = self.retries.wrapping_add(1);
            if self.retries > RETRY_LIMIT {
                self.state = RxState::Aborted;
                return Err(XModemError::RetryExhausted { block: block_no, retries: self.retries });
            }
            return Ok(NAK); // 请求重发（重试计数留痕）
        }
        // CRC 过：重试计数归零，入审计区
        self.retries = 0;
        self.audit.append(payload, self.cap)?;
        self.blocks_done = self.blocks_done.wrapping_add(1);
        // 块号 8 位循环：1..=255 → 0 → 1……
        self.expect = self.expect.wrapping_add(1);
        if self.state == RxState::Idle {
            self.state = RxState::Receiving;
        }
        Ok(ACK)
    }

    /// 喂 EOT（收尾确认；块数达 TARGET_BLOCKS 才允许正常收尾）。
    pub fn feed_eot(&mut self) -> Result<u8, XModemError> {
        if self.state.is_terminal() {
            return Err(XModemError::TerminalFeed);
        }
        if self.blocks_done >= TARGET_BLOCKS {
            self.state = RxState::Done;
            Ok(ACK)
        } else {
            // 块数不足：不发最终 ACK（发送方应继续）；仍处 Receiving
            Ok(NAK)
        }
    }

    /// 喂协议噪声字节（非 SOH/EOT 起始）。
    pub fn feed_noise(&mut self, byte: u8) -> Result<(), XModemError> {
        if self.state.is_terminal() {
            return Err(XModemError::TerminalFeed);
        }
        Err(XModemError::ProtocolFraming { byte })
    }

    /// J1 口径一：零丢包（已收块数 == 目标块数）。
    pub const fn zero_loss(&self) -> bool {
        self.blocks_done == TARGET_BLOCKS
    }

    /// J1 口径二：数据只入审计区且零执行。
    pub fn audit_only_no_exec(&self) -> bool {
        !self.audit.is_executable() && self.audit.len() == (self.blocks_done as usize) * BLOCK_PAYLOAD
    }

    /// 接收器快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        b.push(self.state as u8);
        b.push(self.expect);
        b.extend_from_slice(&self.blocks_done.to_le_bytes());
        b.extend_from_slice(&self.crc_failures.to_le_bytes());
        b.extend_from_slice(&self.duplicates_absorbed.to_le_bytes());
        b.extend_from_slice(&self.audit.crc32().to_le_bytes());
        b.extend_from_slice(&self.audit.len().to_le_bytes());
        crc16_xmodem(&b) as u64
    }
}

/// 域自检（`run_unx263_checks`）：J1 判据逐条可失败断言。
pub fn run_unx263_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx263");

    // C263-01 CRC16 XModem 标准向量（"123456789" = 0x31C3）
    {
        let got = crc16_xmodem(b"123456789");
        let ok = got == 0x31C3;
        s.add("C263-01-CRC16 标准向量一致", ok, if ok { "" } else { "CRC16 实现偏离标准" });
    }

    // C263-02 正常接收流：块 1、2、3 连续 + EOT NAK（未满 8192）+ 状态保持
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let payload = [0xA5u8; BLOCK_PAYLOAD];
        let crc = crc16_xmodem(&payload);
        let r1 = rx.feed_block(1, !1u8, &payload, crc);
        let r2 = rx.feed_block(2, !2u8, &payload, crc);
        let r3 = rx.feed_block(3, !3u8, &payload, crc);
        let eot = rx.feed_eot();
        let ok = r1 == Ok(ACK) && r2 == Ok(ACK) && r3 == Ok(ACK) && eot == Ok(NAK)
            && rx.blocks_done == 3
            && rx.state == RxState::Receiving
            && rx.audit.len() == 3 * BLOCK_PAYLOAD;
        s.add("C263-02-连续块接收入审计区", ok, if ok { "" } else { "正常接收流失效" });
    }

    // C263-03 块号缺口显性检出（丢包立案）
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let payload = [0u8; BLOCK_PAYLOAD];
        let crc = crc16_xmodem(&payload);
        let _ = rx.feed_block(1, !1u8, &payload, crc);
        let r = rx.feed_block(3, !3u8, &payload, crc); // 跳过 2 → 缺口
        let ok = matches!(r, Err(XModemError::SequenceGap { expected: 2, got: 3 }))
            && rx.state == RxState::Aborted;
        s.add("C263-03-丢包缺口显性立案", ok, if ok { "" } else { "块号缺口未检出" });
    }

    // C263-04 反码不配对显性检出
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let payload = [0u8; BLOCK_PAYLOAD];
        let r = rx.feed_block(1, 0x23, &payload, crc16_xmodem(&payload));
        let ok = matches!(r, Err(XModemError::ComplementBad { block: 1 }));
        s.add("C263-04-反码自校验拦截", ok, if ok { "" } else { "反码不配对未检出" });
    }

    // C263-05 CRC 失败 → NAK 重试 → 重发成功幂等入账（重试计数归零）
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let payload = [0x5Au8; BLOCK_PAYLOAD];
        let good_crc = crc16_xmodem(&payload);
        // 首发坏 CRC
        let r1 = rx.feed_block(1, !1u8, &payload, good_crc ^ 0xFFFF);
        // 重发好 CRC
        let r2 = rx.feed_block(1, !1u8, &payload, good_crc);
        let ok = r1 == Ok(NAK) && r2 == Ok(ACK)
            && rx.blocks_done == 1
            && rx.crc_failures == 1
            && rx.audit.len() == BLOCK_PAYLOAD;
        s.add("C263-05-CRC 失败 NAK 重试闭环", ok, if ok { "" } else { "重试闭环失效" });
    }

    // C263-06 重试超限显性中止（不静默跳块）
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let payload = [0u8; BLOCK_PAYLOAD];
        let good_crc = crc16_xmodem(&payload);
        let mut last: Result<u8, XModemError> = Ok(0);
        for _ in 0..(RETRY_LIMIT + 1) {
            last = rx.feed_block(1, !1u8, &payload, good_crc ^ 0x1234);
        }
        let ok = matches!(last, Err(XModemError::RetryExhausted { block: 1, .. }))
            && rx.state == RxState::Aborted
            && rx.blocks_done == 0;
        s.add("C263-06-重试超限显性中止", ok, if ok { "" } else { "超限未被显性拦截" });
    }

    // C263-07 重复块幂等吸收（发送方超时重发不重复入账）
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let payload = [0x11u8; BLOCK_PAYLOAD];
        let crc = crc16_xmodem(&payload);
        let _ = rx.feed_block(1, !1u8, &payload, crc);
        let dup = rx.feed_block(1, !1u8, &payload, crc);
        let ok = dup == Ok(ACK)
            && rx.blocks_done == 1
            && rx.duplicates_absorbed == 1
            && rx.audit.len() == BLOCK_PAYLOAD;
        s.add("C263-07-重复块幂等吸收", ok, if ok { "" } else { "重复块重复入账或被误拒" });
    }

    // C263-08 安全边界：mark_executable 永远失败且计数（零执行红线）
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let payload = [0x77u8; BLOCK_PAYLOAD];
        let _ = rx.feed_block(1, !1u8, &payload, crc16_xmodem(&payload));
        let mut attempts = 0;
        let mut all_denied = true;
        for _ in 0..5 {
            if rx.audit.mark_executable().is_err() {
                attempts += 1;
            } else {
                all_denied = false;
            }
        }
        let ok = all_denied
            && !rx.audit.is_executable()
            && rx.audit.exec_attempts() == 5
            && attempts == 5
            && rx.audit_only_no_exec();
        s.add("C263-08-审计区零执行红线", ok, if ok { "" } else { "执行边界被突破" });
    }

    // C263-09 审计区容量门（超预算显性拒）
    {
        let mut rx = XModemReceiver::new(BLOCK_PAYLOAD as u64 * 2); // 容量只够 2 块
        let payload = [0u8; BLOCK_PAYLOAD];
        let crc = crc16_xmodem(&payload);
        let _ = rx.feed_block(1, !1u8, &payload, crc);
        let _ = rx.feed_block(2, !2u8, &payload, crc);
        let r3 = rx.feed_block(3, !3u8, &payload, crc);
        let ok = matches!(r3, Err(XModemError::AuditOverflow { .. }));
        s.add("C263-09-审计区容量显性拒", ok, if ok { "" } else { "容量超限被静默吞" });
    }

    // C263-10 终态后喂字节显性拒
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        rx.state = RxState::Done; // 直接收尾态
        let r1 = rx.feed_block(1, !1u8, &[0u8; BLOCK_PAYLOAD], 0);
        let r2 = rx.feed_noise(0x00);
        let ok = matches!(r1, Err(XModemError::TerminalFeed)) && matches!(r2, Err(XModemError::TerminalFeed));
        s.add("C263-10-终态后喂字节拒绝", ok, if ok { "" } else { "终态后仍接受数据" });
    }

    // C263-11 协议噪声字节显性拒
    {
        let mut rx = XModemReceiver::new(TARGET_BYTES * 2);
        let r = rx.feed_noise(0xFF);
        let ok = matches!(r, Err(XModemError::ProtocolFraming { byte: 0xFF }));
        s.add("C263-11-协议噪声显性拒", ok, if ok { "" } else { "噪声字节未被拦" });
    }

    // C263-12 快照指纹确定且内容敏感
    {
        let mut rx1 = XModemReceiver::new(TARGET_BYTES * 2);
        let mut rx2 = XModemReceiver::new(TARGET_BYTES * 2);
        let p1 = [0x33u8; BLOCK_PAYLOAD];
        let p2 = [0x34u8; BLOCK_PAYLOAD];
        let _ = rx1.feed_block(1, !1u8, &p1, crc16_xmodem(&p1));
        let _ = rx2.feed_block(1, !1u8, &p2, crc16_xmodem(&p2));
        let f1a = rx1.fingerprint();
        let f1b = rx1.fingerprint();
        let f2 = rx2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C263-12-快照指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C263-13 诊断码 0x94 段独占
    {
        let codes = [
            XModemCode::SequenceGap,
            XModemCode::ComplementBad,
            XModemCode::CrcBad,
            XModemCode::RetryExhausted,
            XModemCode::ExecForbidden,
            XModemCode::ProtocolFraming,
            XModemCode::AuditOverflow,
            XModemCode::TerminalFeed,
            XModemCode::Complete,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C263-13-诊断码 0x94 段独占", all, if all { "" } else { "码值越出 0x94 段" });
    }

    // C263-14 目标块数守恒（1MB / 128B == 8192，锚点口径编译期钉住）
    {
        let ok = TARGET_BYTES == 1024 * 1024 && TARGET_BLOCKS == 8192 && BLOCK_PAYLOAD == 128;
        s.add("C263-14-1MB=8192 块口径钉死", ok, if ok { "" } else { "块数口径漂移" });
    }

    s
}
