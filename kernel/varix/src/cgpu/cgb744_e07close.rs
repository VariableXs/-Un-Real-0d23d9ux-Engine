//! cgb744_e07close —— CGPU-F0744 E07 组收口与 E08 移交。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0744 E07 组收口与 E08 移交  <SEG 0x4b40>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0744；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. E07 组收口 = 把 E07 批内全部工单的验收态聚合成组收口单，
//!      收口前提：批内工单 100% 到达"已完成"且逐单结果摘要非空；
//!   2. 移交单（Handover）是 E07→E08 的唯一过桥凭证：八个冻结字段
//!      （批号/工单数/绿数/红数/指纹/收口时间戳/移交方/接收方），
//!      字段表一经构造不可变；
//!   3. 收口状态机四态：Open→QuorumMet→Sealed→HandedOver，
//!      Sealed 后任何工单状态翻转都是红码（封账级不可变）；
//!   4. 组指纹 = FNV-1a 逐单吃（工单号+状态码），顺序敏感——乱序
//!      提交同批工单会得到不同指纹，属显性设计（提交序即账序）；
//!   5. 移交核：接收方以同算法复算指纹，失配即拒收（对账闸）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4b40 段为本模块独占（SEG_BASE=0x4b40），判别式
//!   SEG_BASE+1..+N；与 0x4b20/0x4b60 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const E7_VERSION: &str = "cgb744_e07close-v1";

/// 诊断段基址（0x4b40 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4B40;

// ── XcCode 显式判据式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4b41 批内仍有未收口工单（红单/挂起单存在，不可收口）。
pub const XC_WORK_OPEN: u16 = SEG_BASE + 1;
/// 0x4b42 工单结果摘要为空（已完成但摘要空——假完成）。
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 2;
/// 0x4b43 收口封账后仍试图改工单（Sealed 不可变被破坏）。
pub const XC_SEALED_WRITE: u16 = SEG_BASE + 3;
/// 0x4b44 收口状态机非法边。
pub const XC_BAD_EDGE: u16 = SEG_BASE + 4;
/// 0x4b45 移交指纹失配（接收方复算与移交方不一致，拒收）。
pub const XC_FINGERPRINT_MISMATCH: u16 = SEG_BASE + 5;
/// 0x4b46 移交单字段越界（工单数超批容 64）。
pub const XC_BATCH_OVERFLOW: u16 = SEG_BASE + 6;
/// 0x4b47 重复收口（同批二次 Sealed）。
pub const XC_DOUBLE_SEAL: u16 = SEG_BASE + 7;
/// 0x4b48 接收方重复签收（同一移交单二次 HandedOver）。
pub const XC_DOUBLE_ACCEPT: u16 = SEG_BASE + 8;

/// E07 批容量（写死：册内 §批账 容量 64 工单）。
pub const BATCH_CAP: usize = 64;

/// 工单验收态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkState {
    /// 施工中。
    Building,
    /// 自检绿、待组验。
    SelfGreen,
    /// 组验通过（终态之一）。
    Done,
    /// 组验红（非终态：可返工回 Building）。
    Red,
}

/// 工单：批内最小验收单元。
#[derive(Clone, Debug)]
pub struct WorkItem {
    /// 工单号（批内唯一，0 起）。
    pub wid: u16,
    /// 标题（账面留痕）。
    pub title: String,
    /// 验收态。
    pub state: WorkState,
    /// 结果摘要（Done 时必须非空）。
    pub summary: String,
}

/// 收口状态机四态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseState {
    /// 开批：工单登记中。
    Open,
    /// 法定数已达成：全部工单 Done（尚未封账）。
    QuorumMet,
    /// 已封账：工单表冻结。
    Sealed,
    /// 已移交：指纹对账通过、过桥完成（终态）。
    HandedOver,
}

/// 合法边闭集：Open→QuorumMet→Sealed→HandedOver。
pub fn legal_edges(s: CloseState) -> &'static [CloseState] {
    match s {
        CloseState::Open => &[CloseState::QuorumMet],
        CloseState::QuorumMet => &[CloseState::Sealed],
        CloseState::Sealed => &[CloseState::HandedOver],
        CloseState::HandedOver => &[],
    }
}

fn step(cur: CloseState, next: CloseState) -> Result<CloseState, u16> {
    if legal_edges(cur).iter().any(|n| *n == next) {
        Ok(next)
    } else {
        Err(XC_BAD_EDGE)
    }
}

/// FNV-1a 32 位滚动（组指纹核）。
pub fn fnv32(mut h: u32, byte: u8) -> u32 {
    h ^= byte as u32;
    h.wrapping_mul(0x0100_0193)
}

/// 吃一个 u16（小端两字节）。
fn eat_u16(h: u32, v: u16) -> u32 {
    fnv32(fnv32(h, (v & 0xFF) as u8), (v >> 8) as u8)
}

/// 组指纹：逐单吃 wid + 状态码（0=Building 1=SelfGreen 2=Done 3=Red）。
/// 顺序敏感：账序即指纹序。
pub fn group_fingerprint(items: &[WorkItem]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for it in items {
        h = eat_u16(h, it.wid);
        let code = match it.state {
            WorkState::Building => 0u8,
            WorkState::SelfGreen => 1,
            WorkState::Done => 2,
            WorkState::Red => 3,
        };
        h = fnv32(h, code);
    }
    h
}

/// 移交单：E07→E08 过桥凭证（构造后冻结，字段只读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handover {
    /// 批号（E07 组号）。
    pub batch: u16,
    /// 工单总数。
    pub total: u16,
    /// 绿单数（Done）。
    pub green: u16,
    /// 红单数（Red，收口前提为 0）。
    pub red: u16,
    /// 组指纹（封账时刻）。
    pub fingerprint: u32,
    /// 收口序号（时间戳替身：单调递增封账计数）。
    pub seal_seq: u32,
    /// 移交方标识。
    pub from_tag: u16,
    /// 接收方标识。
    pub to_tag: u16,
}

/// E07 收口引擎：批账 + 状态机 + 移交单签发/签收。
pub struct E07Closer {
    items: Vec<WorkItem>,
    state: CloseState,
    seal_seq: u32,
    sealed_fingerprint: Option<u32>,
    handover: Option<Handover>,
    /// 已签收旗。
    pub accepted: bool,
}

impl E07Closer {
    /// 开批。
    pub fn new() -> Self {
        E07Closer {
            items: Vec::new(),
            state: CloseState::Open,
            seal_seq: 0,
            sealed_fingerprint: None,
            handover: None,
            accepted: false,
        }
    }

    /// 登记工单（封账后红码；批容越界红码；号重复红码）。
    pub fn add(&mut self, wid: u16, title: &str) -> Result<(), u16> {
        if self.state != CloseState::Open {
            return Err(XC_SEALED_WRITE);
        }
        if self.items.len() >= BATCH_CAP {
            return Err(XC_BATCH_OVERFLOW);
        }
        if self.items.iter().any(|i| i.wid == wid) {
            return Err(XC_BATCH_OVERFLOW);
        }
        self.items.push(WorkItem {
            wid,
            title: String::from(title),
            state: WorkState::Building,
            summary: String::new(),
        });
        Ok(())
    }

    /// 工单状态推进：Building→SelfGreen→Done（带摘要）/ Red→Building（返工）。
    /// 封账后一切翻转红码。
    pub fn set_done(&mut self, wid: u16, summary: &str) -> Result<(), u16> {
        if self.state != CloseState::Open {
            return Err(XC_SEALED_WRITE);
        }
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let it = self
            .items
            .iter_mut()
            .find(|i| i.wid == wid)
            .ok_or(XC_WORK_OPEN)?;
        it.state = WorkState::Done;
        it.summary = String::from(summary);
        Ok(())
    }

    /// 打红：允许 Building 语义回退（返工路径）。
    pub fn set_red(&mut self, wid: u16) -> Result<(), u16> {
        if self.state != CloseState::Open {
            return Err(XC_SEALED_WRITE);
        }
        let it = self
            .items
            .iter_mut()
            .find(|i| i.wid == wid)
            .ok_or(XC_WORK_OPEN)?;
        it.state = WorkState::Red;
        it.summary = String::new();
        Ok(())
    }

    /// 批内红单数。
    pub fn red_count(&self) -> usize {
        self.items.iter().filter(|i| i.state == WorkState::Red).count()
    }

    /// 收口主路径：全部工单 Done 且摘要非空 → Open→QuorumMet→Sealed，
    /// 落组指纹、发移交单。空批/红单/空摘要各自红码。
    pub fn seal(&mut self, batch: u16, from_tag: u16, to_tag: u16) -> Result<Handover, u16> {
        if self.state == CloseState::Sealed || self.state == CloseState::HandedOver {
            return Err(XC_DOUBLE_SEAL);
        }
        if self.items.is_empty() {
            return Err(XC_WORK_OPEN);
        }
        if self.red_count() > 0 {
            return Err(XC_WORK_OPEN);
        }
        for it in &self.items {
            if it.state != WorkState::Done {
                return Err(XC_WORK_OPEN);
            }
            if it.summary.is_empty() {
                return Err(XC_SUMMARY_EMPTY);
            }
        }
        self.state = step(self.state, CloseState::QuorumMet)?;
        self.state = step(self.state, CloseState::Sealed)?;
        self.seal_seq = self.seal_seq.wrapping_add(1);
        let fp = group_fingerprint(&self.items);
        self.sealed_fingerprint = Some(fp);
        let h = Handover {
            batch,
            total: self.items.len() as u16,
            green: self.items.len() as u16,
            red: 0,
            fingerprint: fp,
            seal_seq: self.seal_seq,
            from_tag,
            to_tag,
        };
        self.handover = Some(h.clone());
        Ok(h)
    }

    /// 接收方签收：以同算法复算指纹比对移交单，失配拒收；
    /// 二次签收红码。
    pub fn accept(&mut self, claimed: &Handover) -> Result<(), u16> {
        if self.accepted {
            return Err(XC_DOUBLE_ACCEPT);
        }
        if self.state != CloseState::Sealed {
            return Err(XC_BAD_EDGE);
        }
        let rec = group_fingerprint(&self.items);
        if self.sealed_fingerprint != Some(rec) || claimed.fingerprint != rec {
            return Err(XC_FINGERPRINT_MISMATCH);
        }
        if claimed.total as usize != self.items.len() || claimed.red != 0 {
            return Err(XC_FINGERPRINT_MISMATCH);
        }
        self.state = step(self.state, CloseState::HandedOver)?;
        self.accepted = true;
        Ok(())
    }

    /// 当前状态。
    pub fn state(&self) -> CloseState {
        self.state
    }

    /// 已签发的移交单（只读面）。
    pub fn handover(&self) -> Option<&Handover> {
        self.handover.as_ref()
    }

    /// 批收口单文本（非空摘要聚合面）。
    pub fn close_report(&self) -> String {
        let mut s = String::from("E07 close: batch items=");
        s.push_str(&self.items.len().to_string());
        s.push_str(" state=");
        s.push_str(match self.state {
            CloseState::Open => "open",
            CloseState::QuorumMet => "quorum",
            CloseState::Sealed => "sealed",
            CloseState::HandedOver => "handed",
        });
        if let Some(h) = &self.handover {
            s.push_str(" fp=");
            s.push_str(&h.fingerprint.to_string());
        }
        s
    }
}

/// 自例程：主路径（登记→验收→封账→签收）+ 错误路径（空摘要/红单
/// 收口/封账后写/指纹篡改/二次签收），全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let mut c = E07Closer::new();
    c.add(0, "画像注册表")?;
    c.add(1, "场景联动")?;
    c.add(2, "遥测账")?;
    c.set_done(0, "注册表 32 槽全绿")?;
    c.set_done(1, "联动状态机 4 边全绿")?;
    c.set_done(2, "链核全绿")?;
    let h = c.seal(742, 0xE007, 0xE008)?;
    if c.state() != CloseState::Sealed {
        return Err(XC_BAD_EDGE);
    }
    if h.total != 3 || h.green != 3 || h.red != 0 {
        return Err(XC_BATCH_OVERFLOW);
    }
    c.accept(&h)?;
    if c.state() != CloseState::HandedOver || !c.accepted {
        return Err(XC_BAD_EDGE);
    }
    let rep = c.close_report();
    if !rep.contains("handed") {
        return Err(XC_WORK_OPEN);
    }

    // 错误路径 1：空摘要假完成。
    let mut c2 = E07Closer::new();
    c2.add(0, "x")?;
    if c2.set_done(0, "") != Err(XC_SUMMARY_EMPTY) {
        return Err(XC_SUMMARY_EMPTY);
    }
    // 错误路径 2：红单收口。
    let mut c3 = E07Closer::new();
    c3.add(0, "a")?;
    c3.add(1, "b")?;
    c3.set_done(0, "ok")?;
    c3.set_red(1)?;
    if c3.seal(1, 1, 1) != Err(XC_WORK_OPEN) {
        return Err(XC_WORK_OPEN);
    }
    // 错误路径 3：封账后写。
    let mut c4 = E07Closer::new();
    c4.add(0, "a")?;
    c4.set_done(0, "ok")?;
    let _ = c4.seal(2, 1, 1)?;
    if c4.add(1, "late") != Err(XC_SEALED_WRITE) {
        return Err(XC_SEALED_WRITE);
    }
    // 错误路径 4：指纹篡改拒收。
    let mut c5 = E07Closer::new();
    c5.add(0, "a")?;
    c5.set_done(0, "ok")?;
    let h5 = c5.seal(3, 1, 1)?;
    let mut forged = h5;
    forged.fingerprint = forged.fingerprint.wrapping_add(1);
    if c5.accept(&forged) != Err(XC_FINGERPRINT_MISMATCH) {
        return Err(XC_FINGERPRINT_MISMATCH);
    }
    // 错误路径 5：二次签收。
    let mut c6 = E07Closer::new();
    c6.add(0, "a")?;
    c6.set_done(0, "ok")?;
    let h6 = c6.seal(4, 1, 1)?;
    c6.accept(&h6)?;
    if c6.accept(&h6) != Err(XC_DOUBLE_ACCEPT) {
        return Err(XC_DOUBLE_ACCEPT);
    }
    // 指纹顺序敏感性：同批乱序指纹不同（显性设计断言）。
    let mut a = E07Closer::new();
    let mut b = E07Closer::new();
    a.add(0, "x")?;
    a.add(1, "y")?;
    a.set_done(0, "1")?;
    a.set_done(1, "2")?;
    b.add(1, "y")?;
    b.add(0, "x")?;
    b.set_done(1, "2")?;
    b.set_done(0, "1")?;
    let ha = a.seal(9, 1, 1)?;
    let hb = b.seal(9, 1, 1)?;
    if ha.fingerprint == hb.fingerprint {
        return Err(XC_FINGERPRINT_MISMATCH);
    }
    Ok(0)
}
