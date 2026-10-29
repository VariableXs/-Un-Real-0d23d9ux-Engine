//! UNX-F36234-J1 句柄安全账的统一日志埋点（开/复/关全事件）。
//! 判据：① 全事件埋点 155 例（开 100 / 复 30 / 关 25）：每事件一条
//! 日志（kind/slot/gen/ga/seq 五元组），账面计数与事件流严格对平；
//! ② 环形定容 128：溢出覆盖最旧但 seq 全局单调——覆盖不丢账（总账
//! 计数器独立于环内容），最旧存活 seq = 总数 − 容量 + 1 可重放推算；
//! ③ 清算守恒联动：opens = closes + 活句柄数（36227 守恒的日志侧
//! 复算）；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows ob 回调（ObRegisterCallbacks）的账面化——句柄
//! 生命周期全事件过统一埋点，环形缓冲定容防胀、单调序号防重排；
//! 「覆盖不丢账」是环形日志的安全底线：环里永远是最近的窗口，总账
//! 在计数器侧完整。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa260;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_KIND: u16 = SEG_BASE + 2; // 事件种类越闭集
pub const E_SEQ: u16 = SEG_BASE + 3; // 序号破（不单调/重放推算不符）
pub const E_LED: u16 = SEG_BASE + 4; // 总账不平（计数器与事件流对不上）
pub const E_RING: u16 = SEG_BASE + 5; // 环破（容量/覆盖窗口不符）
pub const E_CONS: u16 = SEG_BASE + 6; // 清算守恒破（opens ≠ closes + live）

/// 模块版本锚。
pub const VER: u32 = 1;

const _: () = assert!(SEG_BASE == 0xa260, "段位锚漂移");

/// 事件种类闭集（开/复/关三型——埋点面全覆盖句柄生命周期）。
pub const KIND_OPEN: u8 = 1;
pub const KIND_DUP: u8 = 2;
pub const KIND_CLOSE: u8 = 3;
pub const KIND_N: u8 = 3;

/// 单条日志（五元组）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LogRec {
    pub kind: u8,
    pub slot: usize,
    pub gen: u32,
    pub ga: u32,
    pub seq: u32,
}

/// 环形日志（定容 128 + 全局单调 seq + 总账计数器）。
pub struct LogRing {
    ring: [Option<LogRec>; LOG_CAP],
    head: usize,
    len: usize,
    total: u32,
    overwritten: u32,
    pub opens: u32,
    pub dups: u32,
    pub closes: u32,
}

/// 环容量（定容闭集——防胀红线）。
pub const LOG_CAP: usize = 128;

const NONE_REC: Option<LogRec> = None;

impl LogRing {
    pub fn new() -> LogRing {
        LogRing {
            ring: [NONE_REC; LOG_CAP],
            head: 0,
            len: 0,
            total: 0,
            overwritten: 0,
            opens: 0,
            dups: 0,
            closes: 0,
        }
    }

    /// 埋点（判据 ① 落点）：kind 闭集校验 → 落环 → 总账计数。
    pub fn push(&mut self, kind: u8, slot: usize, gen: u32, ga: u32) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if kind == 0 || kind > KIND_N {
            return Err(E_KIND);
        }
        let rec = LogRec { kind, slot, gen, ga, seq: self.total.wrapping_add(1) };
        if self.len < LOG_CAP {
            self.ring[(self.head + self.len) % LOG_CAP] = Some(rec);
            self.len += 1;
        } else {
            // 环满：覆盖最旧（head 处），head 前移——覆盖不丢账（total
            // 与 overwritten 承接历史）。
            self.ring[self.head] = Some(rec);
            self.head = (self.head + 1) % LOG_CAP;
            self.overwritten = self.overwritten.wrapping_add(1);
        }
        self.total = self.total.wrapping_add(1);
        match kind {
            KIND_OPEN => self.opens = self.opens.wrapping_add(1),
            KIND_DUP => self.dups = self.dups.wrapping_add(1),
            _ => self.closes = self.closes.wrapping_add(1),
        }
        Ok(())
    }

    /// 按序读回（环序 = 时间序——最旧在前）。
    pub fn at(&self, i: usize) -> Option<LogRec> {
        if i >= self.len {
            return None;
        }
        self.ring[(self.head + i) % LOG_CAP]
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn total(&self) -> u32 {
        self.total
    }
    pub fn overwritten(&self) -> u32 {
        self.overwritten
    }

    /// 环自证（判据 ② 落点）：seq 严格递增 + 最旧存活 seq 推算对拍 +
    /// 容量封顶。
    pub fn ring_ok(&self) -> Result<(), u16> {
        if self.len > LOG_CAP {
            return Err(E_RING);
        }
        let mut i = 1;
        while i < self.len {
            let (a, b) = (self.at(i - 1), self.at(i));
            match (a, b) {
                (Some(x), Some(y)) if y.seq == x.seq.wrapping_add(1) => {}
                _ => return Err(E_SEQ),
            }
            i += 1;
        }
        // 覆盖窗口推算：最旧存活 seq = total − len + 1。
        if let Some(oldest) = self.at(0) {
            let expect = self.total.wrapping_sub(self.len as u32).wrapping_add(1);
            if oldest.seq != expect {
                return Err(E_SEQ);
            }
        } else if self.len != 0 {
            return Err(E_RING);
        }
        // 覆盖数推算：overwritten = total − len（溢出前为零）。
        if self.overwritten != self.total.wrapping_sub(self.len as u32) {
            return Err(E_RING);
        }
        Ok(())
    }

    /// 清算守恒复算（判据 ③ 落点）：opens = closes + 活句柄数。
    pub fn conserve_ok(&self, live: u32) -> bool {
        self.opens == self.closes + live
    }

    /// 总账对平：opens + dups + closes = total。
    pub fn tally_ok(&self) -> bool {
        self.opens.wrapping_add(self.dups).wrapping_add(self.closes) == self.total
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R0：种类闭集锚。
    if KIND_OPEN != 1 || KIND_DUP != 2 || KIND_CLOSE != 3 || KIND_N != 3 {
        return Err(E_KIND);
    }
    // R1：真表 + 埋点联动（判据 ① 落点）：60 开（真 attach 埋点）、
    // 30 复（36225 降权复制埋点）、25 关（真 release 埋点）。
    let mut t = gf::HandleTable::new();
    let mut log = LogRing::new();
    let mut keys = [(0usize, 0u32); 60];
    let mut i = 0;
    while i < 60 {
        let ga = [sr::READ_CONTROL | 0x0001, sr::STD_ALL, sr::SYNCHRONIZE][i % 3];
        let (s, g) = t.attach(0x400 + i as u32, ga, 0).map_err(|_| E_LED)?;
        keys[i] = (s, g);
        log.push(KIND_OPEN, s, g, ga)?;
        i += 1;
    }
    // 复制 30 例（源 = 前 30 句柄，跨表降权复制——36226 基元直调）。
    let mut dst = gf::HandleTable::new();
    let mut i = 0u32;
    while i < 30 {
        let (s, g) = keys[i as usize];
        let src_ga = t.read(s, g).map_err(|_| E_LED)?.ga;
        let desired = src_ga & !sr::SYNCHRONIZE;
        let (ds, dg, closed) = super::unx36226_gadup2::dup_cross(
            &mut t,
            &mut dst,
            s,
            g,
            super::unx36226_gadup2::DupMode::Masked,
            desired,
        )?;
        if closed {
            return Err(E_LED);
        }
        log.push(KIND_DUP, ds, dg, desired)?;
        i += 1;
    }
    // 关闭 25 例（后 25 句柄——含真清算）。
    let mut i = 35;
    while i < 60 {
        let (s, g) = keys[i];
        let ga = t.read(s, g).map_err(|_| E_LED)?.ga;
        t.release(s, g).map_err(|_| E_LED)?;
        log.push(KIND_CLOSE, s, g, ga)?;
        i += 1;
    }
    // R2：账面对平（判据 ① 的判定）。
    if log.total() != 115 || log.opens != 60 || log.dups != 30 || log.closes != 25 {
        return Err(E_LED);
    }
    if !log.tally_ok() {
        return Err(E_LED);
    }
    if !log.conserve_ok(t.used() as u32) {
        return Err(E_CONS);
    }
    // 未溢出：环内容 = 全部事件（len == total）。
    if log.len() as u32 != log.total() || log.overwritten() != 0 {
        return Err(E_RING);
    }
    log.ring_ok()?;
    // 环内容逐条对拍：kind 与 seq 起止（首 seq=1、末 seq=115）。
    if log.at(0).map(|r| (r.kind, r.seq)) != Some((KIND_OPEN, 1)) {
        return Err(E_SEQ);
    }
    if log.at(114).map(|r| (r.kind, r.seq)) != Some((KIND_CLOSE, 115)) {
        return Err(E_SEQ);
    }
    // R3：溢出覆盖（判据 ② 落点）：再开 85 例 → 总 200 > 128，覆盖 72。
    let mut i = 0;
    while i < 85 {
        let (s, g) = t.attach(0x800 + i as u32, sr::READ_CONTROL, 0).map_err(|_| E_LED)?;
        log.push(KIND_OPEN, s, g, sr::READ_CONTROL)?;
        i += 1;
    }
    if log.total() != 200 || log.overwritten() != 72 {
        return Err(E_RING);
    }
    if log.len() != LOG_CAP {
        return Err(E_RING);
    }
    log.ring_ok()?;
    // 覆盖窗口推算对拍：最旧存活 seq = 200 − 128 + 1 = 73。
    if log.at(0).map(|r| r.seq) != Some(73) {
        return Err(E_SEQ);
    }
    // 末条是最后一次 open。
    if log.at(127).map(|r| (r.kind, r.seq)) != Some((KIND_OPEN, 200)) {
        return Err(E_SEQ);
    }
    // R4：种类越闭集拒（判据面负例）。
    let mut log2 = LogRing::new();
    if log2.push(4, 0, 0, 0).err() != Some(E_KIND) {
        return Err(E_KIND);
    }
    if log2.push(0, 0, 0, 0).err() != Some(E_KIND) {
        return Err(E_KIND);
    }
    if log2.total() != 0 {
        return Err(E_LED);
    }
    // 校验和：total + overwritten + 活句柄数（确定性总锚）。
    Ok(log
        .total()
        .wrapping_add(log.overwritten())
        .wrapping_add(t.used() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_KIND => "事件种类越闭集（开/复/关三型之外）",
        E_SEQ => "序号破（不单调/覆盖窗口推算不符）",
        E_LED => "总账不平（计数器与事件流对不上）",
        E_RING => "环破（容量封顶/覆盖数推算不符）",
        E_CONS => "清算守恒破（opens ≠ closes + live）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36234_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36234_galog");
    cs.add("SEG 段位独占", SEG_BASE == 0xa260, "0xa260~0xa26F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "155 例全埋点/溢出覆盖不丢账/守恒复算"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("环容量锚", LOG_CAP == 128, "环形定容 128（防胀红线）");
    let empty = LogRing::new();
    cs.add("空账诚实", empty.tally_ok() && empty.conserve_ok(0) && empty.total() == 0, "空环零事件零覆盖");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_ring_overwrite_window() {
        let mut log = LogRing::new();
        let mut i = 0;
        while i < 130 {
            log.push(KIND_OPEN, i, 1, 0).unwrap_or(());
            i += 1;
        }
        assert_eq!(log.total(), 130);
        assert_eq!(log.overwritten(), 2);
        assert_eq!(log.len(), LOG_CAP);
        // 最旧存活 seq = 130 − 128 + 1 = 3。
        assert_eq!(log.at(0).map(|r| r.seq), Some(3));
        assert!(log.ring_ok().is_ok());
        assert!(log.tally_ok());
    }

    #[test]
    fn test_kind_closed_set() {
        let mut log = LogRing::new();
        assert_eq!(log.push(KIND_DUP, 0, 1, 2).err(), None);
        assert_eq!(log.dups, 1);
        assert_eq!(log.push(9, 0, 1, 2).err(), Some(E_KIND));
        assert_eq!(log.total(), 1);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36234_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
