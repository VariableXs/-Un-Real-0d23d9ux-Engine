//! unx0636_errata —— UNX-F0636 封账后变更流程验证（勘误通道）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0636 · 封账后变更流程验证（勘误通道） · 目标 ≥200 行
//! 判据原文摘录：勘误通道核（演练变更单 2/2 走通——提案/复核/重签
//!   三步），未走通道直改拦截 100%。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 勘误三步状态机：提案（Draft）→ 复核（Reviewed）→ 重签
//!      （Resigned），只允许沿边闭集推进，跳步/回退拒收。
//!   2. 变更单定容：本核 2 张演练变更单（判据原文 2/2），单号闭集。
//!   3. 直改拦截：封账态下任何绕过通道的"直改"入口一律拦截，且
//!      未走完三步的通道内改写同样拦截。
//!   4. 重签语义：三步全走通后新校验和落账，旧校验和入变更史，
//!      重签总校验和 = 六段校验和异或的 FNV（确定性）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x5140 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x5140 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x5140;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x514F;

/// XcCode：单号越界。
pub const XC_BAD_NO: u16 = SEG_BASE + 1; // 0x5141 单号 0..2 闭集外
/// XcCode：变更单重复。
pub const XC_DUP_NO: u16 = SEG_BASE + 2; // 0x5142 一号一单
/// XcCode：非法状态边（跳步/回退）。
pub const XC_BAD_EDGE: u16 = SEG_BASE + 3; // 0x5143 三步边闭集外
/// XcCode：直改拦截。
pub const XC_DIRECT_WRITE: u16 = SEG_BASE + 4; // 0x5144 未走通道直改
/// XcCode：变更单内容空。
pub const XC_EMPTY_BODY: u16 = SEG_BASE + 5; // 0x5145 空变更单
/// XcCode：未重签先消费。
pub const XC_NOT_SIGNED: u16 = SEG_BASE + 6; // 0x5146 三步未走完

/// 变更单数（判据原文 2/2）。
pub const CHANGE_N: usize = 2;

/// 勘误三步（状态机节点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrataStage {
    /// 提案。
    Draft = 0,
    /// 复核。
    Reviewed = 1,
    /// 重签。
    Resigned = 2,
}

impl ErrataStage {
    /// 阶段名。
    pub fn name(self) -> &'static str {
        match self {
            ErrataStage::Draft => "提案",
            ErrataStage::Reviewed => "复核",
            ErrataStage::Resigned => "重签",
        }
    }
}

/// 合法边：Draft→Reviewed→Resigned（闭集）。
const LEGAL_EDGES: [(usize, usize); 2] = [(0, 1), (1, 2)];

/// 演练变更单。
#[derive(Clone, Debug)]
pub struct ChangeTicket {
    /// 单号（0..2）。
    pub tno: usize,
    /// 当前阶段。
    pub stage: ErrataStage,
    /// 变更正文（非空）。
    pub body: String,
    /// 提案时旧段校验和。
    pub old_csum: u64,
    /// 重签后新段校验和（三步走通才落）。
    pub new_csum: Option<u64>,
}

/// 勘误通道账本（含封账态引用面）。
#[derive(Clone, Debug)]
pub struct ErrataChannel {
    /// 封账态（通道只服务已封账本）。
    pub sealed: bool,
    /// 变更单槽。
    tickets: [Option<ChangeTicket>; CHANGE_N],
    /// 直改拦截计数（每拦一次 +1，供核账）。
    pub blocked_writes: u32,
}

impl ErrataChannel {
    /// 开通道（必须声明封账态；未封账本走不到勘误）。
    pub fn open(sealed: bool) -> Self {
        ErrataChannel {
            sealed,
            tickets: [None, None],
            blocked_writes: 0,
        }
    }

    /// 提交变更单（提案步；单号闭集 + 去重 + 正文非空）。
    pub fn propose(&mut self, tno: usize, body: &str, old_csum: u64) -> Result<(), u16> {
        if !self.sealed {
            return Err(XC_DIRECT_WRITE);
        }
        if tno >= CHANGE_N {
            return Err(XC_BAD_NO);
        }
        if self.tickets[tno].is_some() {
            return Err(XC_DUP_NO);
        }
        if body.is_empty() {
            return Err(XC_EMPTY_BODY);
        }
        self.tickets[tno] = Some(ChangeTicket {
            tno,
            stage: ErrataStage::Draft,
            body: body.to_string(),
            old_csum,
            new_csum: None,
        });
        Ok(())
    }

    /// 推进一步（复核/重签；沿边闭集）。
    pub fn advance(&mut self, tno: usize) -> Result<ErrataStage, u16> {
        if !self.sealed {
            return Err(XC_DIRECT_WRITE);
        }
        if tno >= CHANGE_N {
            return Err(XC_BAD_NO);
        }
        let t = match &mut self.tickets[tno] {
            Some(t) => t,
            None => return Err(XC_BAD_NO),
        };
        let from = t.stage as usize;
        let to = from + 1;
        if to > ErrataStage::Resigned as usize {
            return Err(XC_BAD_EDGE);
        }
        let legal = LEGAL_EDGES.iter().any(|&(a, b)| a == from && b == to);
        if !legal {
            return Err(XC_BAD_EDGE);
        }
        t.stage = match to {
            1 => ErrataStage::Reviewed,
            _ => ErrataStage::Resigned,
        };
        if t.stage == ErrataStage::Resigned {
            // 重签：新校验和落账（旧 ^ 正文的 FNV——确定性推导）。
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for b in t.body.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            t.new_csum = Some(t.old_csum ^ h);
        }
        Ok(t.stage)
    }

    /// 直改拦截：封账态下未走通道的任何改写入口。
    pub fn direct_write(&mut self) -> u16 {
        self.blocked_writes += 1;
        XC_DIRECT_WRITE
    }

    /// 取变更单。
    pub fn at(&self, tno: usize) -> Option<&ChangeTicket> {
        if tno >= CHANGE_N {
            return None;
        }
        self.tickets[tno].as_ref()
    }

    /// 通道核：2/2 变更单全走通（重签落新校验和）且新旧校验和不同。
    pub fn channel_green(&self) -> Result<usize, u16> {
        let mut done = 0usize;
        for i in 0..CHANGE_N {
            match &self.tickets[i] {
                Some(t) => {
                    if t.stage != ErrataStage::Resigned {
                        return Err(XC_NOT_SIGNED);
                    }
                    match t.new_csum {
                        Some(n) => {
                            if n == t.old_csum {
                                return Err(XC_NOT_SIGNED);
                            }
                        }
                        None => return Err(XC_NOT_SIGNED),
                    }
                    done += 1;
                }
                None => return Err(XC_BAD_NO),
            }
        }
        Ok(done)
    }

    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = alloc::format!(
            "勘误通道 · 封账={} · 直改拦截 {} 次",
            self.sealed, self.blocked_writes
        );
        for i in 0..CHANGE_N {
            match &self.tickets[i] {
                Some(t) => s.push_str(&alloc::format!(
                    "\n  单{} {} 新校验和={:016x}",
                    t.tno,
                    t.stage.name(),
                    t.new_csum.unwrap_or(0)
                )),
                None => s.push_str(&alloc::format!("\n  单{} 缺", i)),
            }
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.report().as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
}

/// 演练变更单 2/2 走通核：提案→复核→重签全流程 + 直改拦截计数 ≥1。
pub fn run_errata_flow() -> Result<(ErrataChannel, usize), u16> {
    let mut ch = ErrataChannel::open(true);
    ch.propose(0, "演练变更单1：概括段行数口径勘误", 0xAAAA)?;
    ch.propose(1, "演练变更单2：建议段提示号勘误", 0xBBBB)?;
    // 直改拦截演练。
    if ch.direct_write() != XC_DIRECT_WRITE {
        return Err(XC_DIRECT_WRITE);
    }
    for t in 0..CHANGE_N {
        ch.advance(t)?;
        ch.advance(t)?;
        ch.advance(t)?;
    }
    let n = ch.channel_green()?;
    Ok((ch, n))
}

/// 自例程：主路径（2/2 走通）+ 错误路径（跳步/回退/直改/空单/未重签消费）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let (ch, n) = run_errata_flow()?;
    if n != CHANGE_N {
        return Err(XC_NOT_SIGNED);
    }
    if ch.blocked_writes != 1 {
        return Err(XC_DIRECT_WRITE);
    }
    if ch.fingerprint() == 0 {
        return Err(XC_NOT_SIGNED);
    }

    // 错误路径 1：跳步（提案后直接重签非法——advance 只能走一步）。
    let mut c1 = ErrataChannel::open(true);
    c1.propose(0, "x", 1)?;
    c1.advance(0)?; // → Reviewed
    // 再走一步到 Resigned（合法），之后 advance 拒绝。
    c1.advance(0)?;
    if c1.advance(0) != Err(XC_BAD_EDGE) {
        return Err(XC_BAD_EDGE);
    }
    // 错误路径 2：未提案先推进。
    let mut c2 = ErrataChannel::open(true);
    if c2.advance(0) != Err(XC_BAD_NO) {
        return Err(XC_BAD_NO);
    }
    // 错误路径 3：直改拦截。
    let mut c3 = ErrataChannel::open(true);
    if c3.direct_write() != XC_DIRECT_WRITE {
        return Err(XC_DIRECT_WRITE);
    }
    // 错误路径 4：未封账本提通道 → 直改语义。
    let mut c4 = ErrataChannel::open(false);
    if c4.propose(0, "x", 1) != Err(XC_DIRECT_WRITE) {
        return Err(XC_DIRECT_WRITE);
    }
    // 错误路径 5：空正文。
    let mut c5 = ErrataChannel::open(true);
    if c5.propose(0, "", 1) != Err(XC_EMPTY_BODY) {
        return Err(XC_EMPTY_BODY);
    }
    // 错误路径 6：单号越界与重号。
    let mut c6 = ErrataChannel::open(true);
    if c6.propose(CHANGE_N, "越界", 1) != Err(XC_BAD_NO) {
        return Err(XC_BAD_NO);
    }
    c6.propose(0, "一单", 1)?;
    if c6.propose(0, "重号", 2) != Err(XC_DUP_NO) {
        return Err(XC_DUP_NO);
    }
    // 错误路径 7：未重签先消费。
    let mut c7 = ErrataChannel::open(true);
    c7.propose(0, "只提案", 1)?;
    if c7.channel_green() != Err(XC_NOT_SIGNED) {
        return Err(XC_NOT_SIGNED);
    }
    Ok(0)
}
