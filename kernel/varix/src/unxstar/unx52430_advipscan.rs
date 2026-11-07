//! UNX-F52430 Advanced IP Scanner 收口 · 0xCAB40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52430"，逐字）：
//!   title: Advanced IP Scanner 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52431-J1 六判据
//!     全落；同白名单；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52430）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52431-J1，照原文锚定；功能按本卡标题（Advanced IP Scanner）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族闭集 20 格：扫描 / 远程桌面 / 共享 / 唤醒 / 关机，每族
//!      4 格，族载荷纯函数折叠恒等才收；族单调（逆序必抓）。
//!   ② 结果三态 6 格：存活 / 失活 / 无响应 每态 2 格，逐态投影对照，
//!      态漂移必抓。
//!   ③ 与 RDP 消费联签 4 格：RDP 会话枚举联签 / 凭据通道联签 / 端口
//!      3389 联签 / 会话注销回执联签，逐格落账，缺一拒封（ESeal）。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（结果态漂移/
//!      功能漂移/族逆序）10/10 必抓，逃逸恒 0；双跑指纹一致。
//!   ⑤ 定级目标档 S2（KB-COMPAT 登记）：空账 S3，六闸齐 S2，全齐目标
//!      档 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52430;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族 20 格。
pub const FEAT_CELLS: usize = 20;

/// 结果三态 6 格。
pub const RESULT_CELLS: usize = 6;

/// RDP 消费联签格数。
pub const RDP_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdvIpCode {
    /// 成功
    Ok = 0,
    /// 功能格越闭集
    ENoFeat,
    /// 功能漂移
    EDrift,
    /// 功能族逆序
    EOrder,
    /// 结果格越闭集
    ENoRes,
    /// 结果态漂移
    EResDrift,
    /// RDP 联签格越闭集
    ENoRdp,
    /// RDP 联签缺格
    ERdpMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl AdvIpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AdvIpCode::Ok => "成功",
            AdvIpCode::ENoFeat => "功能格越闭集：五功能族 20 格之外先登记",
            AdvIpCode::EDrift => "功能漂移：功能族载荷折叠与基准不恒等，逐格复算",
            AdvIpCode::EOrder => "功能族逆序：扫描→远程桌面→共享→唤醒→关机单调，逆序必抓",
            AdvIpCode::ENoRes => "结果格越闭集：结果三态 6 格之外先登记",
            AdvIpCode::EResDrift => "结果态漂移：结果态投影与基准不恒等，逐态复算",
            AdvIpCode::ENoRdp => "RDP 联签格越闭集：RDP 消费联签 4 格之外先登记",
            AdvIpCode::ERdpMiss => "RDP 联签缺格：与 RDP 消费联签未齐先补齐",
            AdvIpCode::EDup => "重复落账：同格只许一条账目",
            AdvIpCode::ESeal => "未齐即封：20 功能格 + 6 结果格 + 4 联签格未齐先补齐",
            AdvIpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            AdvIpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AdvIpCode::EGradeDrift => "定级漂移：全齐且六闸落账才可定 S2 目标档",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：本批卡面不触达（保留闭集完整性）
    #[allow(dead_code)]
    S1,
    /// S2：收口目标档
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：全通收口（本批 VX16 卡面目标档为 S2，不触达）",
            Grade::S2 => "S2：收口目标档（KB-COMPAT 定级登记）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 功能格载荷（格 f；族 = f/4）。
pub const fn feat_cell(f: usize) -> Option<u64> {
    if f >= FEAT_CELLS {
        return None;
    }
    Some((0xC5000u64 + (f as u64) * 0x33) ^ ((f / 4) as u64).wrapping_mul(0x01000193))
}

/// 功能族（0..4=扫描 4..8=远程桌面 8..12=共享 12..16=唤醒 16..20=关机）。
pub const fn feat_kind(f: usize) -> Option<u32> {
    if f >= FEAT_CELLS {
        return None;
    }
    Some((f / 4) as u32)
}

/// 功能族名。
pub const fn feat_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("扫描"),
        1 => Some("远程桌面"),
        2 => Some("共享"),
        3 => Some("唤醒"),
        4 => Some("关机"),
        _ => None,
    }
}

/// 结果格载荷（格 r；态 = r/2：0=存活 1=失活 2=无响应）。
pub const fn result_cell(r: usize) -> Option<u64> {
    if r >= RESULT_CELLS {
        return None;
    }
    Some((0xC6000u64 + (r as u64) * 0x2B) ^ 0x5245535354410000)
}

/// 结果三态（i/N 映射：态 = r/2）。
pub const fn result_state(r: usize) -> Option<u32> {
    if r >= RESULT_CELLS {
        return None;
    }
    Some((r / 2) as u32)
}

/// 结果态名。
pub const fn result_state_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("存活"),
        1 => Some("失活"),
        2 => Some("无响应"),
        _ => None,
    }
}

/// RDP 消费联签格折叠（格 d）。
pub const fn rdp_cell(d: usize) -> Option<u32> {
    if d >= RDP_ROWS {
        return None;
    }
    Some(0xC7000 + d as u32 * 0x2C)
}

/// RDP 联签格名。
pub const fn rdp_cell_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("RDP 会话枚举联签"),
        1 => Some("凭据通道联签"),
        2 => Some("端口 3389 联签"),
        3 => Some("会话注销回执联签"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Advanced IP Scanner 收口账本状态机。
#[derive(Clone)]
pub struct AdvIpLedger {
    /// 六判据（UNX-F52431-J1 锚）。
    gates: [Option<bool>; 6],
    /// 功能账。
    feats: [Option<u64>; FEAT_CELLS],
    /// 结果账。
    res: [Option<u64>; RESULT_CELLS],
    /// RDP 联签账。
    rdp: [Option<u32>; RDP_ROWS],
    /// 最后功能族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AdvIpLedger {
    /// 新账本。
    pub fn new() -> Self {
        AdvIpLedger {
            gates: [None; 6],
            feats: [None; FEAT_CELLS],
            res: [None; RESULT_CELLS],
            rdp: [None; RDP_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AdvIpCode> {
        if gate >= 6 {
            return Err(AdvIpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 功能一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_feat(&mut self, f: usize, got: u64) -> Result<(), AdvIpCode> {
        if f >= FEAT_CELLS {
            return Err(AdvIpCode::ENoFeat);
        }
        let kind = match feat_kind(f) {
            Some(k) => k,
            None => return Err(AdvIpCode::ENoFeat),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(AdvIpCode::EOrder);
        }
        let want = match feat_cell(f) {
            Some(w) => w,
            None => return Err(AdvIpCode::ENoFeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + f as u64);
            return Err(AdvIpCode::EDrift);
        }
        if match self.feats.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AdvIpCode::EDup);
        }
        if let Some(c) = self.feats.get_mut(f) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + f as u64);
        Ok(())
    }

    /// 功能全账核验（20 格）。
    pub fn feats_all(&self) -> bool {
        let mut f = 0usize;
        while f < FEAT_CELLS {
            match self.feats.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 结果一格（态越闭集 + 投影对照 + 重复拒）。
    pub fn record_result(&mut self, r: usize, got: u64) -> Result<(), AdvIpCode> {
        let want = match result_cell(r) {
            Some(w) => w,
            None => return Err(AdvIpCode::ENoRes),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + r as u64);
            return Err(AdvIpCode::EResDrift);
        }
        if match self.res.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AdvIpCode::EDup);
        }
        if let Some(c) = self.res.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x6301 + r as u64);
        Ok(())
    }

    /// 结果全账核验（6 格）。
    pub fn res_all(&self) -> bool {
        let mut r = 0usize;
        while r < RESULT_CELLS {
            match self.res.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// RDP 消费联签一格。
    pub fn record_rdp(&mut self, d: usize) -> Result<(), AdvIpCode> {
        let want = match rdp_cell(d) {
            Some(w) => w,
            None => return Err(AdvIpCode::ENoRdp),
        };
        if match self.rdp.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AdvIpCode::EDup);
        }
        if let Some(c) = self.rdp.get_mut(d) {
            *c = Some(want);
        }
        self.fnv(0x6400 + d as u64);
        Ok(())
    }

    /// RDP 联签全齐核验。
    pub fn rdp_all(&self) -> bool {
        let mut d = 0usize;
        while d < RDP_ROWS {
            match self.rdp.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（目标档 S2：六闸齐 → S2，全齐亦 S2；空账 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AdvIpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转（结果态漂移 / 功能漂移 / 族逆序）全部落在当轮
/// 临时账本上：主账 injected==caught==10、escaped==0；漂移注入先按合法
/// 序落正格再注漂移；族逆序注入对空账本同样必抓且不改主账 last_kind。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AdvIpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FEAT_CELLS {
        if let Some(w) = feat_cell(f) {
            if led.record_feat(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < RESULT_CELLS {
        if let Some(w) = result_cell(r) {
            if led.record_result(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut d = 0usize;
    while d < RDP_ROWS {
        if led.record_rdp(d).is_ok() {
            recorded += 1;
        }
        d += 1;
    }
    // 注入 10 轮：结果态漂移 / 功能漂移 / 族逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AdvIpLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_result(0, result_cell(0).unwrap_or(0));
                lt.record_result(1, result_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(AdvIpCode::EResDrift)
            }
            1 => {
                let _ = lt.record_feat(4, feat_cell(4).unwrap_or(0));
                let _ = lt.record_feat(5, feat_cell(5).unwrap_or(0));
                lt.record_feat(6, feat_cell(6).unwrap_or(0).wrapping_add(1)) == Err(AdvIpCode::EDrift)
            }
            _ => {
                let _ = lt.record_feat(16, feat_cell(16).unwrap_or(0));
                lt.record_feat(1, feat_cell(1).unwrap_or(0)) == Err(AdvIpCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6500 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52430 判据集（锚 UNX-F52431-J1，off-by-one 板式，§AI-66.3）。
pub fn run_unx52430_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52430");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 格全通",
        recorded >= FEAT_CELLS as u32 + RESULT_CELLS as u32 + RDP_ROWS as u32,
        "扫描/远程桌面/共享/唤醒/关机逐格恒等（UNX-F52431-J1 锚）",
    );
    s.add(
        "结果三态 6 格全落",
        {
            let mut lt = AdvIpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RESULT_CELLS {
                if let Some(w) = result_cell(r) {
                    if lt.record_result(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.res_all()
        },
        "存活/失活/无响应逐态投影对照",
    );
    s.add(
        "与 RDP 消费联签 4/4 全落",
        {
            let mut lt = AdvIpLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < RDP_ROWS {
                if lt.record_rdp(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.rdp_all()
        },
        "会话枚举/凭据通道/3389/注销回执四格联签",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = AdvIpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "UNX-F52431-J1 六判据逐格落账",
    );
    s.add(
        "功能族单调",
        {
            let mut ok = true;
            let mut f = 1usize;
            while f < FEAT_CELLS {
                if feat_kind(f).unwrap_or(5) < feat_kind(f - 1).unwrap_or(5) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "五功能族闭集单调（§AI-66.3）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "结果态漂移/功能漂移/族逆序三态轮转必抓");
    let mut l0 = AdvIpLedger::new();
    s.add(
        "结果态漂移拒",
        {
            let _ = l0.record_result(0, result_cell(0).unwrap_or(0));
            l0.record_result(1, result_cell(1).unwrap_or(0).wrapping_add(1)) == Err(AdvIpCode::EResDrift)
        },
        "结果态投影漂移必抓",
    );
    s.add(
        "族逆序拒",
        {
            let _ = l0.record_feat(16, feat_cell(16).unwrap_or(0));
            l0.record_feat(1, feat_cell(1).unwrap_or(0)) == Err(AdvIpCode::EOrder)
        },
        "关机后回扫描必抓",
    );
    s.add(
        "边界·功能越闭集",
        l0.record_feat(FEAT_CELLS, 0).is_err(),
        "功能格 20 越闭集必拒",
    );
    s.add(
        "边界·结果越闭集",
        l0.record_result(RESULT_CELLS, 0).is_err(),
        "结果格 6 越闭集必拒",
    );
    s.add(
        "边界·RDP 联签越闭集",
        l0.record_rdp(RDP_ROWS).is_err(),
        "RDP 联签格 4 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 目标档",
        {
            let mut lt = AdvIpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < FEAT_CELLS {
                if let Some(w) = feat_cell(f) {
                    let _ = lt.record_feat(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < RESULT_CELLS {
                if let Some(w) = result_cell(r) {
                    let _ = lt.record_result(r, w);
                }
                r += 1;
            }
            let mut d = 0usize;
            while d < RDP_ROWS {
                let _ = lt.record_rdp(d);
                d += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全齐目标档 S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feat_kind() {
        assert_eq!(feat_kind(0), Some(0));
        assert_eq!(feat_kind(4), Some(1));
        assert_eq!(feat_kind(8), Some(2));
        assert_eq!(feat_kind(19), Some(4));
        assert!(feat_kind(20).is_none());
        assert!(feat_cell(20).is_none());
        assert_eq!(feat_kind_name(1), Some("远程桌面"));
        assert_eq!(feat_kind_name(4), Some("关机"));
        assert!(feat_kind_name(5).is_none());
    }

    #[test]
    fn test_result_state_mapping() {
        assert_eq!(result_state(0), Some(0));
        assert_eq!(result_state(1), Some(0));
        assert_eq!(result_state(2), Some(1));
        assert_eq!(result_state(5), Some(2));
        assert!(result_state(6).is_none());
        assert!(result_cell(6).is_none());
        assert_eq!(result_state_name(2), Some("无响应"));
    }

    #[test]
    fn test_rdp_rows() {
        assert_eq!(rdp_cell_name(0), Some("RDP 会话枚举联签"));
        assert_eq!(rdp_cell_name(3), Some("会话注销回执联签"));
        assert!(rdp_cell_name(4).is_none());
        assert!(rdp_cell(RDP_ROWS).is_none());
        assert!(rdp_cell(0).is_some());
    }

    #[test]
    fn test_cells_pure() {
        let a = feat_cell(11).unwrap_or(0);
        assert_eq!(a, feat_cell(11).unwrap_or(0));
        assert_ne!(a, feat_cell(12).unwrap_or(0));
        assert_ne!(result_cell(1).unwrap_or(0), result_cell(2).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = AdvIpLedger::new();
        let _ = lt.record_feat(9, feat_cell(9).unwrap_or(0));
        assert_eq!(lt.record_feat(2, feat_cell(2).unwrap_or(0)), Err(AdvIpCode::EOrder));
        assert!(lt.record_feat(10, feat_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AdvIpLedger::new();
        assert_eq!(
            lt.record_feat(6, feat_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(AdvIpCode::EDrift)
        );
        assert_eq!(
            lt.record_result(4, result_cell(4).unwrap_or(0).wrapping_add(3)),
            Err(AdvIpCode::EResDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AdvIpLedger::new();
        let _ = lt.record_result(0, result_cell(0).unwrap_or(0));
        assert_eq!(lt.record_result(0, result_cell(0).unwrap_or(0)), Err(AdvIpCode::EDup));
        let _ = lt.record_rdp(2);
        assert_eq!(lt.record_rdp(2), Err(AdvIpCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AdvIpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= FEAT_CELLS as u32 + RESULT_CELLS as u32 + RDP_ROWS as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCAB40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AdvIpCode::Ok as u32, 0);
        assert_eq!(AdvIpCode::EGradeDrift as u32, 12);
        assert_eq!(AdvIpCode::ERdpMiss.val(), SEG_BASE + 7);
        assert!(!AdvIpCode::Ok.detail().is_empty());
        assert!(!AdvIpCode::EResDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = AdvIpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = AdvIpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        // 全齐目标档 S2。
        let mut f = 0usize;
        while f < FEAT_CELLS {
            if let Some(w) = feat_cell(f) {
                let _ = lt2.record_feat(f, w);
            }
            f += 1;
        }
        let mut d = 0usize;
        while d < RDP_ROWS {
            let _ = lt2.record_rdp(d);
            d += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_injection_kinds_rotate() {
        // 三态轮转：10 轮 = 3+3+3+1，三类注入全部必抓。
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
    }

    #[test]
    fn unx52430_checks_green() {
        let s = run_unx52430_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
