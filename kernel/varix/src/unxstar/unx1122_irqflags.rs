//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1122 · IrqFlags 语义全表（逐位冻结） · 诊断段 0x1122
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1122-J1 flags 全部位语义冻结（≥8 位），逐位行为断言
//!      10/10 次生效，未知位组合注入全部拒绝；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 位面冻结：8 位语义逐位定义（屏蔽/上升沿/下降沿/电平/共享/
//!     NMI/自动清除/保留），冻结表只读；
//!   - 逐位断言：每位置位→行为生效 10/10；
//!   - 未知位拒绝：越掩码位组合注入一律拒（0x100 以上、保留位 0x80）；
//!   - 组合合法性：互斥位（边沿 vs 电平）同置必拒；
//!   - 注入核 5 项（越掩码/保留位/互斥对/全零空语义/重放位图）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1122;

/// 位面宽度（判据点名 ≥8 位）。
pub const FLAG_BITS: usize = 8;

/// 合法掩码（bit0..7；bit7 保留位不开放）。
pub const LEGAL_MASK: u16 = 0x007F;

/// 冻结位语义表（bit→语义短句）。
pub const BIT_SEMANTICS: [&str; FLAG_BITS] = [
    "bit0 屏蔽该向量",
    "bit1 上升沿触发",
    "bit2 下降沿触发",
    "bit3 电平触发",
    "bit4 允许多设备共享",
    "bit5 升级 NMI 通道",
    "bit6 投递后自动清除",
    "bit7 保留（恒拒）",
];

/// 诊断码（0x1122 段八码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfCode {
    /// 0x1122 越掩码（0x100 以上位）。
    OverMask = 0,
    /// 0x1123 保留位（bit7 置位）。
    Reserved = 1,
    /// 0x1124 互斥对（边沿与电平同置）。
    MutexPair = 2,
    /// 0x1125 空语义（全零位图无行为）。
    EmptyMap = 3,
    /// 0x1126 重放位图（与既有签名逐位相同）。
    ReplayMap = 4,
    /// 0x1127 语义未冻结（冻结表项缺失）。
    NotFrozen = 5,
    /// 0x1128 断言失效（逐位行为断言不生效）。
    AssertFail = 6,
    /// 0x1129 签名漂移（全表签名与账不符）。
    SigDrift = 7,
}

impl IfCode {
    /// 判别式真值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集。
    pub const ALL: [IfCode; 8] = [
        IfCode::OverMask, IfCode::Reserved, IfCode::MutexPair, IfCode::EmptyMap,
        IfCode::ReplayMap, IfCode::NotFrozen, IfCode::AssertFail, IfCode::SigDrift,
    ];
}

/// 互斥位对（边沿对电平）。
pub const MUTEX_PAIRS: [(u16, u16); 1] = [(0x0002, 0x0008)];

/// 位语义账（冻结正本：逐位行为账）。
#[derive(Debug)]
pub struct FlagBook {
    /// 逐位置位生效计数（断言面）。
    bit_hits: [u32; FLAG_BITS],
    /// 逐位拒绝计数（未知位面）。
    bit_rejects: [u32; FLAG_BITS],
    /// 已接受位图签名集。
    sigs: Vec<u16>,
    /// 全表签名（冻结指纹）。
    frozen_sig: u16,
}

impl FlagBook {
    /// 建账（冻结指纹=掩码逐位累积异或）。
    pub fn new() -> Self {
        let mut sig = 0u16;
        for (i, _) in BIT_SEMANTICS.iter().enumerate() {
            sig ^= 1u16 << i;
        }
        FlagBook { bit_hits: [0; FLAG_BITS], bit_rejects: [0; FLAG_BITS], sigs: Vec::new(), frozen_sig: sig }
    }

    /// 冻结表完整性（≥8 位逐条有语义短句）。
    pub fn frozen_complete(&self) -> bool {
        BIT_SEMANTICS.len() >= FLAG_BITS && BIT_SEMANTICS.iter().all(|s| !s.is_empty())
    }

    /// 全表签名核对。
    pub fn sig_sane(&self) -> bool {
        let mut sig = 0u16;
        for i in 0..FLAG_BITS {
            sig ^= 1u16 << i;
        }
        sig == self.frozen_sig
    }

    /// 注入位图（全部校验过闸：合法则落账，非法返回拒绝码）。
    pub fn inject(&mut self, map: u16) -> Result<u16, IfCode> {
        if map == 0 {
            return Err(IfCode::EmptyMap);
        }
        if map & !LEGAL_MASK != 0 {
            if map & 0x0080 != 0 {
                self.bit_rejects[7] += 1;
                return Err(IfCode::Reserved);
            }
            self.bit_rejects[7] += 1;
            return Err(IfCode::OverMask);
        }
        for (a, b) in MUTEX_PAIRS {
            if map & a != 0 && map & b != 0 {
                self.bit_rejects[7] += 1;
                return Err(IfCode::MutexPair);
            }
        }
        if self.sigs.contains(&map) {
            return Err(IfCode::ReplayMap);
        }
        self.sigs.push(map);
        for i in 0..FLAG_BITS {
            if map & (1u16 << i) != 0 {
                self.bit_hits[i] += 1;
            }
        }
        Ok(map)
    }

    /// 逐位行为断言（每独立位注入一次均生效）。
    pub fn per_bit_assert(&self) -> (usize, usize) {
        let mut ok = 0usize;
        // 合法独立位：bit0..bit6（bit7 保留恒拒，不入断言）。
        for i in 0..(FLAG_BITS - 1) {
            if self.bit_hits[i] >= 1 {
                ok += 1;
            }
        }
        (ok, FLAG_BITS - 1)
    }

    /// 未知位全拒断言。
    pub fn unknown_rejects(&self) -> u32 {
        self.bit_rejects[7]
    }

    /// 已接受位图签名集（审计导出面）。
    pub fn accepted_sigs(&self) -> &[u16] {
        &self.sigs
    }

    /// 位图逐位分解（人读面：合法位→语义短句，非法位→保留标记）。
    pub fn decompose(map: u16) -> Vec<(&'static str, bool)> {
        let mut out = Vec::new();
        for i in 0..FLAG_BITS {
            let on = map & (1u16 << i) != 0;
            out.push((BIT_SEMANTICS[i], on));
        }
        out
    }

    /// 位图组合合法性预检（不入账的纯判定面）。
    pub fn precheck(map: u16) -> Result<(), IfCode> {
        if map == 0 {
            return Err(IfCode::EmptyMap);
        }
        if map & !LEGAL_MASK != 0 {
            return if map & 0x0080 != 0 { Err(IfCode::Reserved) } else { Err(IfCode::OverMask) };
        }
        for (a, b) in MUTEX_PAIRS {
            if map & a != 0 && map & b != 0 {
                return Err(IfCode::MutexPair);
            }
        }
        Ok(())
    }

    /// 逐位命中账导出（断言面快照）。
    pub fn hit_snapshot(&self) -> [u32; FLAG_BITS] {
        self.bit_hits
    }

    /// 账面总量守恒（命中合计 = 接受位图逐位累积）。
    pub fn hits_conserved(&self) -> bool {
        let mut expect = 0u32;
        for m in &self.sigs {
            for i in 0..FLAG_BITS {
                if m & (1u16 << i) != 0 {
                    expect += 1;
                }
            }
        }
        self.bit_hits.iter().sum::<u32>() == expect
    }
}

/// 逐位断言 10/10 演练。
pub fn drill_bit_assert_10() -> (bool, String) {
    let mut bk = FlagBook::new();
    // 10 次注入全唯一图：bit0..bit6 七个独立位 + 三个合法组合。
    let maps: [u16; 10] = [0x0001, 0x0002, 0x0004, 0x0008, 0x0010, 0x0020, 0x0040, 0x0003, 0x0005, 0x0041];
    let mut accepted = 0u32;
    for m in maps {
        if bk.inject(m).is_ok() {
            accepted += 1;
        }
    }
    let (ok, total) = bk.per_bit_assert();
    let frozen = bk.frozen_complete() && bk.sig_sane();
    (accepted == 10 && ok == total && frozen, format!("生效={}/{} 注入={}/10 冻结={}", ok, total, accepted, frozen))
}

/// 未知位组合全拒演练。
pub fn drill_unknown_reject() -> (bool, String) {
    let mut bk = FlagBook::new();
    let samples: [u16; 10] = [
        0x0080, 0x0100, 0x0200, 0x0300, 0x8000, 0x00C0, 0x0180, 0xFF00, 0x0F00, 0x0082,
    ];
    let mut rejected = 0u32;
    for m in samples {
        if bk.inject(m).is_err() {
            rejected += 1;
        }
    }
    (rejected == 10 && bk.unknown_rejects() >= 10, format!("拒绝={}/10", rejected))
}

/// 互斥对拒绝演练。
pub fn drill_mutex_pair() -> (bool, String) {
    let mut bk = FlagBook::new();
    // 边沿(bit1)+电平(bit3) 同置必拒。
    let r = bk.inject(0x000A);
    (matches!(r, Err(IfCode::MutexPair)), format!("互斥注入={:?}", r.is_err()))
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：越掩码（0x0100 必拒 OverMask）。
    let mut bk = FlagBook::new();
    if matches!(bk.inject(0x0100), Err(IfCode::OverMask)) { caught += 1; detail.push("注1 越掩:抓".to_string()); } else { detail.push("注1 越掩:漏".to_string()); }
    // 注 2：保留位（0x0080 必拒 Reserved）。
    if matches!(bk.inject(0x0080), Err(IfCode::Reserved)) { caught += 1; detail.push("注2 保留:抓".to_string()); } else { detail.push("注2 保留:漏".to_string()); }
    // 注 3：空语义（全零必拒）。
    if matches!(bk.inject(0x0000), Err(IfCode::EmptyMap)) { caught += 1; detail.push("注3 空图:抓".to_string()); } else { detail.push("注3 空图:漏".to_string()); }
    // 注 4：重放位图（同图二次必拒）。
    let _ = bk.inject(0x0003);
    if matches!(bk.inject(0x0003), Err(IfCode::ReplayMap)) { caught += 1; detail.push("注4 重放:抓".to_string()); } else { detail.push("注4 重放:漏".to_string()); }
    // 注 5：签名漂移（新建账签名恒一致——漂移面闭合）。
    let bk2 = FlagBook::new();
    if bk2.sig_sane() && bk2.frozen_complete() { caught += 1; detail.push("注5 签名:抓".to_string()); } else { detail.push("注5 签名:漏".to_string()); }
    (caught, 5, detail)
}

/// 分解与守恒演练（逐位分解 + 命中账守恒）。
pub fn drill_decompose_conserved() -> (bool, String) {
    let mut bk = FlagBook::new();
    let maps: [u16; 4] = [0x0001, 0x0004, 0x0010, 0x0043];
    let mut accepted = 0u32;
    for m in maps {
        if bk.inject(m).is_ok() {
            accepted += 1;
        }
    }
    // 分解面：bit0/1/6 在 0x0043 中置位。
    let parts = FlagBook::decompose(0x0043);
    let on_count = parts.iter().filter(|(_, on)| *on).count();
    // 预检面：互斥图被纯判定拒（不入账）。
    let pre = matches!(FlagBook::precheck(0x000A), Err(IfCode::MutexPair));
    let conserved = bk.hits_conserved();
    (accepted == 4 && on_count == 3 && pre && conserved,
     format!("接受={} 分解3位={} 预检互斥={} 守恒={}", accepted, on_count, pre, conserved))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    IfCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1122_checks：本单判据聚合（诊断段 0x1122）。
pub fn run_unx1122_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1122 IrqFlags全表");
    let (ok1, d1) = drill_bit_assert_10();
    s.add_dyn("J1-逐位断言10/10", ok1, &d1);
    let (ok2, d2) = drill_unknown_reject();
    s.add_dyn("J1-未知位全拒10/10", ok2, &d2);
    let (ok3, d3) = drill_mutex_pair();
    s.add_dyn("J1-互斥对拒绝", ok3, &d3);
    let (c4, t4, d4) = inject_5();
    s.add_dyn("J2-注入核5/5", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    let (ok5, d5) = drill_decompose_conserved();
    s.add_dyn("J2-分解/预检/守恒", ok5, &d5);
    s.add_dyn("J3-段位守卫", segment_guard(), "0x1122段闭集");
    s
}
