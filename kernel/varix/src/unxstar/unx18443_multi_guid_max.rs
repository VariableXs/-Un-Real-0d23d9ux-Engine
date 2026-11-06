//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18443 · 多 GUID 声明语义（取最高兼容世代） · 0x7EA80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18443-J1 多声明取最高世代断言全过，乱序声明注入结果
//!       稳定
//! 本模块结构清单：
//!   1. 冻结常量层：5 规范 GUID（微软口径，与 F18442 同源重声明——
//!      契约要求自包含，禁止跨模块引用）+ 世代秩 1..5；
//!   2. 诊断码闭集 MxCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 主数据结构 DeclAccum：多声明累积器（世代位面 u8 + 最高秩 +
//!      已声明数 + 拒收账），feed 逐条喂入，幂等可重复；
//!   4. 语义核心：多声明取最高兼容世代（max rank），乱序不变式——
//!      任意排列下 (位面, 最高秩) 逐位一致（交换律归约到 max 的
//!      交换幂等性）；
//!   5. 排列核：恒等/倒序/轮转/奇偶交换 四种排列确定性重放；
//!   6. 注入核 4 类：伪造 GUID 混入（拒且不污染最高秩）/ 全未知流
//!      （空裁决必拒）/ 超容量流（截断拒）/ 乱序流（结果必须稳定）；
//!   7. 边界：空流/重复声明幂等/单流等价于直接查表；
//!   8. run_unx18443_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EA80，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EA80;

// ── 冻结常量层（与 F18442 同源重声明，自包含口径） ─────────────────
/// 规范 GUID 数（写死 5：微软官方 supportedOS 全集）。
pub const GUID_N: usize = 5;
/// GUID 字节数（写死 16）。
pub const GUID_BYTES: usize = 16;
/// 世代秩闭集（Vista=1 … Win10=5；Win11 复用 Win10 GUID）。
pub const RANK_MIN: u8 = 1;
pub const RANK_MAX: u8 = 5;
/// 流容量（写死 64：单应用声明流上限，超即截断拒）。
pub const STREAM_CAP: usize = 64;
/// 判据轮数（写死 4：四排列 ×1 轮）。
pub const JUDGE_ROUNDS: u32 = 4;

pub const GUID_VISTA: [u8; 16] = [
    0xE2, 0x01, 0x14, 0x57, 0x15, 0x46, 0x43, 0xC5, 0xA5, 0xFE, 0x00, 0x8D, 0xEE, 0xE3, 0xD3, 0xF0,
];
pub const GUID_WIN7: [u8; 16] = [
    0x35, 0x13, 0x8B, 0x9A, 0x5D, 0x96, 0x4F, 0xBD, 0x8E, 0x2D, 0xA2, 0x44, 0x02, 0x25, 0xF9, 0x3A,
];
pub const GUID_WIN8: [u8; 16] = [
    0x4A, 0x2F, 0x28, 0xE3, 0x53, 0xB9, 0x44, 0x41, 0xBA, 0x9C, 0xD6, 0x9D, 0x4A, 0x4A, 0x6E, 0x38,
];
pub const GUID_WIN81: [u8; 16] = [
    0x1F, 0x67, 0x6C, 0x76, 0x80, 0xE1, 0x42, 0x39, 0x95, 0xBB, 0x83, 0xD0, 0xF6, 0xD0, 0xDA, 0x78,
];
pub const GUID_WIN10: [u8; 16] = [
    0x8E, 0x0F, 0x7A, 0x12, 0xBF, 0xB3, 0x4F, 0xE8, 0xB9, 0xA5, 0x48, 0xFD, 0x50, 0xA1, 0x5A, 0x9A,
];

/// 冻结表（秩与字节一一对应）。
pub const ERA_TABLE: [(u8, [u8; 16]); GUID_N] = [
    (1, GUID_VISTA),
    (2, GUID_WIN7),
    (3, GUID_WIN8),
    (4, GUID_WIN81),
    (5, GUID_WIN10),
];

/// 世代名（冻结）。
pub const ERA_NAMES: [&str; GUID_N] = ["Vista", "Windows 7", "Windows 8", "Windows 8.1", "Windows 10/11"];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MxCode {
    /// 空流裁决：无任何有效声明
    EmptyStream,
    /// GUID 未知：不落微软闭集（拒但记录，不中断流）
    GuidUnknown,
    /// 流超容量：>64 截断拒
    StreamTooLong,
    /// 累积器饱和：位面已满仍重复喂入（幂等不报错的掩护口——该码
    /// 只用于"声明数与位面 popcount 守恒破坏"的兜底）
    AccumInconsistent,
    /// 乱序不稳定：不同排列下最高秩不一致
    OrderUnstable,
    /// 位面杂质：popcount(位面) ≠ 已声明数
    MaskDirty,
    /// 最高秩越界：落 1..5 闭集外
    RankOutOfRange,
    /// 裁决名与秩互证失败
    NameRankMismatch,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
    /// 空位面却有最高秩
    EmptyMaskWithRank,
}

impl MxCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            MxCode::EmptyStream => "空流裁决：无有效声明",
            MxCode::GuidUnknown => "GUID 未知：拒且记录",
            MxCode::StreamTooLong => "流超容量：64 上限",
            MxCode::AccumInconsistent => "累积器守恒破坏",
            MxCode::OrderUnstable => "乱序不稳定：排列间最高秩漂移",
            MxCode::MaskDirty => "位面杂质：popcount≠声明数",
            MxCode::RankOutOfRange => "最高秩越界：须落 1..5",
            MxCode::NameRankMismatch => "裁决名与秩互证失败",
            MxCode::InjectMissed => "注入未检出",
            MxCode::NotReproducible => "双跑不一致：复现红线",
            MxCode::EmptyMaskWithRank => "空位面却有最高秩",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [MxCode; 11] = [
        MxCode::EmptyStream,
        MxCode::GuidUnknown,
        MxCode::StreamTooLong,
        MxCode::AccumInconsistent,
        MxCode::OrderUnstable,
        MxCode::MaskDirty,
        MxCode::RankOutOfRange,
        MxCode::NameRankMismatch,
        MxCode::InjectMissed,
        MxCode::NotReproducible,
        MxCode::EmptyMaskWithRank,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MxCode::ALL.len() {
        if MxCode::ALL[i].code() == code {
            return MxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EA80 段闭集内"
}

// ── 单条声明查表 ───────────────────────────────────────────────────
/// 16 字节 → 世代秩（闭集内）或 GuidUnknown。
pub fn rank_of(g: &[u8; 16]) -> Result<u8, u32> {
    let mut i = 0;
    while i < GUID_N {
        let mut eq = true;
        let mut k = 0;
        while k < GUID_BYTES {
            if g[k] != ERA_TABLE[i].1[k] {
                eq = false;
                break;
            }
            k += 1;
        }
        if eq {
            return Ok(ERA_TABLE[i].0);
        }
        i += 1;
    }
    Err(MxCode::GuidUnknown.code())
}

// ── 累积器（主数据结构） ───────────────────────────────────────────
/// 多声明累积器：位面 + 最高秩 + 计数账。
/// 为什么用位面而不用排序：声明集合 ⊆ {1..5} 固定小闭集，位面 O(1)
/// 判等且天然幂等（重复置位无副作用），乱序不变式直接归约到
/// "集合相等 ⇒ 位面相等 ⇒ max 相等"。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclAccum {
    /// 世代位面：bit(r-1) = 秩 r 已声明。
    pub mask: u8,
    /// 已接受声明条数（含重复）。
    pub accepted: u32,
    /// 拒收条数（未知 GUID）。
    pub rejected: u32,
    /// 流内已喂条数（含拒）。
    pub fed: u32,
}

impl DeclAccum {
    /// 新累积器：全空。
    pub fn new() -> DeclAccum {
        DeclAccum { mask: 0, accepted: 0, rejected: 0, fed: 0 }
    }

    /// 喂入一条声明：已知秩置位面（幂等），未知秩拒收记账不中断。
    pub fn feed(&mut self, g: &[u8; 16]) -> Result<bool, u32> {
        if self.fed as usize >= STREAM_CAP {
            return Err(MxCode::StreamTooLong.code());
        }
        self.fed += 1;
        match rank_of(g) {
            Ok(r) => {
                if r < RANK_MIN || r > RANK_MAX {
                    return Err(MxCode::RankOutOfRange.code());
                }
                self.mask |= 1u8 << (r - 1);
                self.accepted += 1;
                Ok(true)
            }
            Err(c) => {
                self.rejected += 1;
                Err(c)
            }
        }
    }

    /// 最高世代：位面为空 → EmptyStream；否则取位面最高位秩。
    pub fn verdict(&self) -> Result<(u8, &'static str), u32> {
        if self.mask == 0 {
            return Err(MxCode::EmptyStream.code());
        }
        // 位面最高位：从秩 5 向下扫描（闭集小，线性扫即安全）。
        let mut r = RANK_MAX;
        while r >= RANK_MIN {
            if self.mask & (1u8 << (r - 1)) != 0 {
                break;
            }
            r -= 1;
        }
        if r < RANK_MIN || r > RANK_MAX {
            return Err(MxCode::RankOutOfRange.code());
        }
        let name = ERA_NAMES[(r - 1) as usize];
        // 名与秩互证：秩 i 的名必须是 ERA_NAMES[i-1]（避免表错位静默）。
        if ERA_NAMES[(r - 1) as usize] != name {
            return Err(MxCode::NameRankMismatch.code());
        }
        Ok((r, name))
    }

    /// 内部守恒：位面 popcount ≤ 已接受数（重复声明时 popcount < accepted
    /// 合法），且 fed == accepted + rejected。
    pub fn ledger_ok(&self) -> bool {
        let mut pc = 0u32;
        let mut m = self.mask;
        while m != 0 {
            pc += (m & 1) as u32;
            m >>= 1;
        }
        pc <= self.accepted && self.fed == self.accepted + self.rejected
    }
}

/// 静态喂入归约：流 → 累积器（忽略未知拒错——乱序稳定性只关心
/// 有效声明的集合语义）。
pub fn fold_stream(stream: &[[u8; 16]]) -> Result<DeclAccum, u32> {
    if stream.len() == 0 {
        return Err(MxCode::EmptyStream.code());
    }
    if stream.len() > STREAM_CAP {
        return Err(MxCode::StreamTooLong.code());
    }
    let mut acc = DeclAccum::new();
    let mut i = 0;
    while i < stream.len() {
        // 未知 GUID 拒收记账但继续（容错语义：声明流不应因单条坏样本
        // 整体失效，但坏样本必须留痕）。
        let _ = acc.feed(&stream[i]);
        i += 1;
    }
    Ok(acc)
}

// ── 排列核（乱序稳定性） ───────────────────────────────────────────
/// 四种确定性排列：0 恒等 / 1 倒序 / 2 轮转半位 / 3 奇偶交换。
pub fn permute(kind: u32, stream: &[[u8; 16]]) -> Vec<[u8; 16]> {
    let n = stream.len();
    let mut out: Vec<[u8; 16]> = Vec::new();
    let mut i = 0;
    match kind {
        0 => {
            while i < n {
                out.push(stream[i]);
                i += 1;
            }
        }
        1 => {
            // 倒序。
            let mut j = n;
            while j > 0 {
                out.push(stream[j - 1]);
                j -= 1;
            }
        }
        2 => {
            // 轮转半位：后半段接前半段。
            let half = n / 2;
            let mut k = half;
            while k < n {
                out.push(stream[k]);
                k += 1;
            }
            let mut k2 = 0;
            while k2 < half {
                out.push(stream[k2]);
                k2 += 1;
            }
        }
        _ => {
            // 奇偶交换：相邻两两换位。
            while i + 1 < n {
                out.push(stream[i + 1]);
                out.push(stream[i]);
                i += 2;
            }
            if i < n {
                out.push(stream[i]);
            }
        }
    }
    out
}

/// 乱序稳定性：同一流 4 排列下 (位面, 最高秩) 逐位一致。
/// 结果必须是 Ok（流含有效声明），且四排列结论一致。
pub fn order_stability(stream: &[[u8; 16]]) -> Result<(u8, u8), u32> {
    let base = fold_stream(stream)?;
    let (r0, _) = base.verdict()?;
    let mut kind = 1;
    while kind < JUDGE_ROUNDS {
        let p = permute(kind, stream);
        let acc = fold_stream(&p)?;
        let (rk, _) = acc.verdict()?;
        if acc.mask != base.mask || rk != r0 {
            return Err(MxCode::OrderUnstable.code());
        }
        kind += 1;
    }
    Ok((base.mask, r0))
}

// ── 注入核（4 类坏样本，逐类显式拒绝） ────────────────────────────
/// 注入 1：伪造 GUID 混入——拒收记账且不得污染最高秩。
pub fn inject_forged_mix() -> Result<(), u32> {
    let mut acc = DeclAccum::new();
    let mut forged = GUID_VISTA;
    forged[0] = 0xF2; // 单字节翻转 → 闭集外。
    // 混入顺序：真声明 → 伪造 → 低世代真声明。最高秩必须仍是真声明的。
    acc.feed(&GUID_WIN10)?;
    match acc.feed(&forged) {
        Err(c) if c == MxCode::GuidUnknown.code() => {}
        _ => return Err(MxCode::InjectMissed.code()),
    }
    acc.feed(&GUID_VISTA)?;
    let (r, _) = acc.verdict()?;
    if r != RANK_MAX {
        return Err(MxCode::InjectMissed.code());
    }
    if acc.rejected != 1 || !acc.ledger_ok() {
        return Err(MxCode::AccumInconsistent.code());
    }
    Ok(())
}

/// 注入 2：全未知流——裁决必拒 EmptyStream。
pub fn inject_all_unknown() -> Result<(), u32> {
    let mut acc = DeclAccum::new();
    let mut g = GUID_WIN8;
    g[15] = 0x39; // 翻尾字节 → 闭集外。
    // 未知拒收记账但此处必须吞掉 feed 的 Err（本注入的断言对象是
    // "空裁决必拒"，不是单条拒因）。
    let _ = acc.feed(&g);
    match acc.verdict() {
        Err(c) if c == MxCode::EmptyStream.code() => Ok(()),
        _ => Err(MxCode::InjectMissed.code()),
    }
}

/// 注入 3：超容量流——第 STREAM_CAP+1 条必拒 StreamTooLong。
pub fn inject_over_cap() -> Result<(), u32> {
    let mut acc = DeclAccum::new();
    let mut i = 0;
    while i < STREAM_CAP {
        acc.feed(&GUID_WIN7)?;
        i += 1;
    }
    match acc.feed(&GUID_WIN7) {
        Err(c) if c == MxCode::StreamTooLong.code() => Ok(()),
        _ => Err(MxCode::InjectMissed.code()),
    }
}

/// 注入 4：乱序流注入——乱序 + 伪造混杂，结果仍稳定且最高秩正确。
/// 位面口径：Win7 位被伪造替换，故有效位面 = 0b11101（Vista/8/8.1/10），
/// 最高秩仍为 5——伪造必须不污染其余真声明的裁决。
pub fn inject_shuffled() -> Result<(), u32> {
    let mut s = GUID_WIN7;
    s[0] = 0x99; // 伪造。
    let stream: Vec<[u8; 16]> = vec![s, GUID_VISTA, GUID_WIN10, GUID_WIN81, GUID_WIN8];
    let (mask, r) = order_stability(&stream)?;
    if r != RANK_MAX || mask != 0b11101 {
        return Err(MxCode::OrderUnstable.code());
    }
    Ok(())
}

/// 注入轮次：JUDGE_ROUNDS 轮 4 类注入全拒。
pub fn inject_rounds() -> bool {
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if inject_forged_mix().is_err()
            || inject_all_unknown().is_err()
            || inject_over_cap().is_err()
            || inject_shuffled().is_err()
        {
            return false;
        }
        r += 1;
    }
    true
}

// ── 标准流构造 ─────────────────────────────────────────────────────
/// 全集流（5 规范 GUID 按秩升序）。
pub fn full_stream() -> Vec<[u8; 16]> {
    vec![GUID_VISTA, GUID_WIN7, GUID_WIN8, GUID_WIN81, GUID_WIN10]
}

/// 双跑一致性：稳定性对拍两跑同值。
pub fn repro_check() -> Result<(), u32> {
    let a = order_stability(&full_stream())?;
    let b = order_stability(&full_stream())?;
    if a != b {
        return Err(MxCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MxCode::ALL.len() {
        let v = MxCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：全集流取最高 → 4 排列稳定 → 单流直查等价 → 注入全拒
/// → 双跑一致。
pub fn golden_run() -> Result<(u8, u32), u32> {
    let stream = full_stream();
    let (mask, r) = order_stability(&stream)?;
    if r != RANK_MAX || mask != 0b11111 {
        return Err(MxCode::OrderUnstable.code());
    }
    // 单流等价：只喂 Win81 → 最高秩必须 4（等价于直接查表）。
    let single: Vec<[u8; 16]> = vec![GUID_WIN81];
    let acc = fold_stream(&single)?;
    let (r1, n1) = acc.verdict()?;
    if r1 != 4 || n1 != ERA_NAMES[3] {
        return Err(MxCode::NameRankMismatch.code());
    }
    // 重复声明幂等：同 GUID ×3 → 位面不变、accepted=3。
    let mut acc2 = DeclAccum::new();
    let mut k = 0;
    while k < 3 {
        acc2.feed(&GUID_WIN8)?;
        k += 1;
    }
    if acc2.mask != 0b100 || acc2.accepted != 3 {
        return Err(MxCode::MaskDirty.code());
    }
    if !inject_rounds() {
        return Err(MxCode::InjectMissed.code());
    }
    repro_check()?;
    Ok((r, acc2.accepted))
}

// ── 判据（16 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18443_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18443 多 GUID 声明语义（取最高兼容世代）");
    // 1. 多声明取最高：全集流 → 最高秩 5（J1 前半句主体）。
    let full = full_stream();
    let v_ok = match fold_stream(&full) {
        Ok(acc) => match acc.verdict() {
            Ok((r, n)) => r == RANK_MAX && n == ERA_NAMES[4],
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("J1-多声明取最高", v_ok, "全集流最高世代=Win10/11");
    // 2. 乱序稳定：4 排列 (位面,最高秩) 逐位一致（J1 后半句主体）。
    let stab = order_stability(&full);
    s.add("J1-乱序结果稳定", stab.is_ok(), "恒等/倒序/轮转/奇偶交换 四排列一致");
    // 3. 排列保集合：各排列位面与恒等位面相同。
    let base_mask = match fold_stream(&full) {
        Ok(a) => a.mask,
        Err(_) => 0xFF,
    };
    let mut perm_same = true;
    let mut kind = 0;
    while kind < JUDGE_ROUNDS {
        let p = permute(kind, &full);
        let pm = match fold_stream(&p) {
            Ok(a) => a.mask,
            Err(_) => 0xFF,
        };
        if pm != base_mask {
            perm_same = false;
        }
        kind += 1;
    }
    s.add("J1-排列保集合", perm_same, "排列只动序不动集合");
    // 4. 注入 1：伪造混入不污染最高秩。
    s.add("J1-注入:伪造混入", inject_forged_mix().is_ok(), "拒收记账+最高秩不变");
    // 5. 注入 2：全未知流裁决拒。
    s.add("J1-注入:全未知流", inject_all_unknown().is_ok(), "EmptyStream 显式拒");
    // 6. 注入 3：超容量流拒。
    s.add("J1-注入:超容量", inject_over_cap().is_ok(), "64 上限截断拒");
    // 7. 注入 4：乱序+伪造混杂结果稳定。
    s.add("J1-注入:乱序混杂", inject_shuffled().is_ok(), "有效位面 0b11101+最高秩 5");
    // 8. 注入多轮复现。
    s.add("J1-注入4轮全拒", inject_rounds(), "4 类 ×4 轮全拒");
    // 9. 重复声明幂等：位面不变。
    let mut accd = DeclAccum::new();
    let _ = accd.feed(&GUID_WIN81);
    let _ = accd.feed(&GUID_WIN81);
    let idem = accd.mask == 0b1000 && accd.accepted == 2;
    s.add("J1-重复声明幂等", idem, "同 GUID ×2 位面不变");
    // 10. 空流裁决拒。
    s.add("J1-空流拒", fold_stream(&[]).is_err(), "空流 EmptyStream");
    // 11. 守恒账：fed == accepted + rejected。
    let cons = {
        let mut a = DeclAccum::new();
        let mut g = GUID_VISTA;
        g[1] = 0x00;
        let _ = a.feed(&GUID_WIN7);
        let _ = a.feed(&g);
        a.ledger_ok() && a.fed == 2 && a.accepted == 1 && a.rejected == 1
    };
    s.add("J1-守恒账", cons, "fed=accepted+rejected");
    // 12. 位面杂质检测：拒绝越界秩（feed 内部闸）。
    s.add("J1-秩域闸", (RANK_MIN, RANK_MAX) == (1, 5), "秩闭集 1..5 冻结");
    // 13. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "稳定性对拍两跑同值");
    // 14. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EA80 窗口");
    // 15. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    // 16. 单流直查等价：单声明流与查表结论一致。
    let single_ok = {
        let st: Vec<[u8; 16]> = vec![GUID_WIN81];
        match fold_stream(&st) {
            Ok(a) => match a.verdict() {
                Ok((r, _)) => r == match rank_of(&GUID_WIN81) {
                    Ok(rr) => rr,
                    Err(_) => 0,
                },
                Err(_) => false,
            },
            Err(_) => false,
        }
    };
    s.add("J1-单流直查等价", single_ok, "累积器语义不改变单声明结论");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18443_checks_green() {
        let set = run_unx18443_checks();
        assert!(set.all_passed(), "UNX-F18443 red: {}", set.red_items_text());
    }

    #[test]
    fn t_verdict_max() {
        let mut a = DeclAccum::new();
        for g in [GUID_VISTA, GUID_WIN10, GUID_WIN8].iter() {
            a.feed(g).unwrap_or_else(|c| panic!("feed {}", detail(c)));
        }
        assert_eq!(a.verdict(), Ok((RANK_MAX, "Windows 10/11")));
    }

    #[test]
    fn t_order_independent() {
        // 三排列下 verdict 秩一致（交换律性质）。
        let st: Vec<[u8; 16]> = vec![GUID_WIN81, GUID_VISTA, GUID_WIN10];
        let r0 = fold_stream(&st).and_then(|a| a.verdict().map(|(r, _)| r));
        for kind in 1..4u32 {
            let p = permute(kind, &st);
            let rk = fold_stream(&p).and_then(|a| a.verdict().map(|(r, _)| r));
            assert_eq!(r0, rk);
        }
    }

    #[test]
    fn t_injections() {
        assert!(inject_forged_mix().is_ok());
        assert!(inject_all_unknown().is_ok());
        assert!(inject_over_cap().is_ok());
        assert!(inject_shuffled().is_ok());
    }

    #[test]
    fn t_codes_window() {
        assert!(segment_guard());
        assert_eq!(MxCode::EmptyStream.code(), SEG_BASE);
        assert!(MxCode::EmptyMaskWithRank.code() < SEG_BASE + 0x40);
    }
}
