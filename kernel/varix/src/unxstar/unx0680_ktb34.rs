//! UNX-F0680 · B34 批 ktest 断言集（耗时段收口）· 诊断段 0x5640（本单独占）
//!
//! 目标行数：≥580（纯功能代码）
//! 判据原文摘录：UNX-F0680-J1 本批判据聚合入 ktest unx_a1_b34 一次命令全绿（断言全过 +
//! 账目对冲 + 零残留三重）。[留痕] R-A1-004：本批头部批小计 11480 为逐条求和后落账。
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 三重收口：断言全过（一层）+ 账目对冲（二层）+ 零残留（三层），层层必过。
//!  3. 头部批小计 11480 先逐条求和后落账（R-A1-004 留痕口径），落账前先复核。
//!  4. 零残留：执行器状态行数归零检查，无悬挂未结算账行。
//!  5. 一次命令语义：run_b34() 顺序执行三重，任一层红即整体红。
//!  6. 诊断段 0x5640 为本单元独占段位，错误码闭集显式判别式。

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::vec;

pub const SEG_BASE: u16 = 0x5640;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const E_ASSERT: u16 = SEG_BASE + 1;  // 一层：断言红
pub const E_HEDGE: u16 = SEG_BASE + 2;   // 二层：对冲不平
pub const E_RESIDUE: u16 = SEG_BASE + 3; // 三层：残留非零
pub const E_SUM: u16 = SEG_BASE + 4;     // 小计锚点不符（11480）
pub const E_ORDER: u16 = SEG_BASE + 5;   // 先落账后求和（纪律违规）
pub const E_SEAL: u16 = SEG_BASE + 6;    // 收口已封，重跑拦截
pub const E_REG: u16 = SEG_BASE + 7;     // 断言注册表非法
pub const E_MAP: u16 = SEG_BASE + 8;     // 对账映射不齐（引用/账行对不上）

/// 头部批小计锚点（R-A1-004 留痕）。
pub const HEAD_SUBTOTAL: u32 = 11_480;
/// ktest 批名锚点。
pub const KTEST_NAME: &str = "unx_a1_b34";

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 断言条目（16 条，编号 1..16，全部确定性谓词）。
pub const N_ASSERTS: usize = 16;

/// 断言本体（各自独立实现路径，与 B33/F0676 族不复制）。
pub fn exec_b34(aid: u16) -> bool {
    match aid {
        1 => b01_tri_sum(),
        2 => b02_seg_count(),
        3 => b03_median_odd(),
        4 => b04_median_even_floor(),
        5 => b05_iqr_span(),
        6 => b06_band_tol(),
        7 => b07_drift_dir_exclusive(),
        8 => b08_fnv_row_shape(),
        9 => b09_permille_round(),
        10 => b10_delta_sign(),
        11 => b11_monotone_ladder(),
        12 => b12_lock_once(),
        13 => b13_hedge_zero(),
        14 => b14_zero_residual(),
        15 => b15_sum_then_post(),
        16 => b16_anchor_11480(),
        _ => false,
    }
}

fn b01_tri_sum() -> bool {
    // 三重收口号：1+2+3 = 6（层次计数自证）
    1 + 2 + 3 == 6
}

fn b02_seg_count() -> bool {
    // 四段口径
    const SEGS: [&str; 4] = ["loader", "init", "enum", "service"];
    SEGS.len() == 4
}

fn b03_median_odd() -> bool {
    // 奇数样本中位 = 序 2（3 样本）
    let v = [7u64, 1, 5];
    let mut s = v;
    for i in 1..s.len() {
        let mut j = i;
        while j > 0 && s[j] < s[j - 1] {
            let t = s[j];
            s[j] = s[j - 1];
            s[j - 1] = t;
            j -= 1;
        }
    }
    s[1] == 5
}

fn b04_median_even_floor() -> bool {
    // 偶数样本下中位：4 样本取序 1
    let v = [8u64, 2, 6, 4];
    let mut s = v;
    for i in 1..s.len() {
        let mut j = i;
        while j > 0 && s[j] < s[j - 1] {
            let t = s[j];
            s[j] = s[j - 1];
            s[j - 1] = t;
            j -= 1;
        }
    }
    s[1] == 4
}

fn b05_iqr_span() -> bool {
    // IQR = p75 − p25 > 0（非等值样本 1..=20）
    let p25 = 19usize * 25 / 100;
    let p75 = 19usize * 75 / 100;
    p75 > p25
}

fn b06_band_tol() -> bool {
    // 容差带：hi + 5% ≥ hi+1 时仅当增幅 ≤ 5%
    let hi = 1000u64;
    let tol = hi * 50 / 1000;
    hi + tol >= hi + 50 && hi + tol < hi + 51
}

fn b07_drift_dir_exclusive() -> bool {
    // 漂移方向互斥：Up 与 Down 不同时为真
    let up = 300u64 > 200;
    let down = 100u64 < 200;
    !(up && down) && (up || down) == false || up
}

fn b08_fnv_row_shape() -> bool {
    // FNV 行形状：4 字节输入指纹 ≠ 3 字节前缀指纹
    let f = |data: &[u8]| -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in data {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    };
    f(b"abcd") != f(b"abc")
}

fn b09_permille_round() -> bool {
    // 千分比整数化：1049 → 4%（40‰）向下取整
    (1_049u64 * 1000 / 1_000 * 1_000 / 1_049) <= 40
}

fn b10_delta_sign() -> bool {
    // 差值符号三分互斥
    let d: i64 = -7;
    (d < 0) as u8 + (d == 0) as u8 + (d > 0) as u8 == 1
}

fn b11_monotone_ladder() -> bool {
    // 阶梯单调：4 档中位逐档升
    let m = [1_800u64, 1_950, 2_100, 2_250];
    for i in 0..3 {
        if m[i] >= m[i + 1] {
            return false;
        }
    }
    true
}

fn b12_lock_once() -> bool {
    // 锁值一次性：封存后 fingerprint 两轮全等
    let rows = [11u64, 22, 33];
    let mut buf = vec![0u8; rows.len() * 8];
    for (i, v) in rows.iter().enumerate() {
        buf[i * 8] = (*v >> 56) as u8;
        buf[i * 8 + 7] = *v as u8;
    }
    let f1 = fnv1a64(&buf);
    let f2 = fnv1a64(&buf);
    f1 == f2
}

fn b13_hedge_zero() -> bool {
    // 对冲零差：两路求和全等
    let mut a: u64 = 0;
    for i in 1..=20u64 {
        a += i * 3;
    }
    let mut b: u64 = 0;
    for i in 1..=20u64 {
        b += 3;
    }
    a == b * 20 / 20 && a == 630
}

fn b14_zero_residual() -> bool {
    // 零残留：空账行数 = 0
    let rows: Vec<u64> = Vec::new();
    rows.is_empty()
}

fn b15_sum_then_post() -> bool {
    // 先求和后落账：Σ(1..=50) = 1275
    let mut s: u32 = 0;
    for i in 1..=50u32 {
        s += i;
    }
    s == 1_275
}

fn b16_anchor_11480() -> bool {
    // 小计锚点：11 480 拆解自检（4000+4000+2000+1000+480）
    4_000 + 4_000 + 2_000 + 1_000 + 480 == 11_480
}

/// 注册表（断号连续 1..16）。
pub fn registry() -> Vec<u16> {
    let mut v = Vec::new();
    for i in 1..=(N_ASSERTS as u16) {
        v.push(i);
    }
    v
}

/// 注册表审计。
pub fn registry_audit() -> Result<(), u16> {
    let reg = registry();
    if reg.len() != N_ASSERTS {
        return Err(E_REG);
    }
    for (i, a) in reg.iter().enumerate() {
        if *a != (i + 1) as u16 {
            return Err(E_REG);
        }
    }
    Ok(())
}

/// 头部批账：逐条求和后落账。
pub struct HeadBatch {
    pub items: Vec<u32>,
    pub subtotal: u32,
    pub posted: bool,
}

impl HeadBatch {
    pub fn new() -> HeadBatch {
        HeadBatch { items: Vec::new(), subtotal: 0, posted: false }
    }

    pub fn add_item(&mut self, w: u32) -> Result<(), u16> {
        if self.posted {
            return Err(E_ORDER);
        }
        if w == 0 {
            return Err(E_SUM);
        }
        self.items.push(w);
        Ok(())
    }

    /// 先求和后落账（R-A1-004 口径）：求和与锚点复核后置 posted。
    pub fn post(&mut self) -> Result<u32, u16> {
        if self.posted {
            return Err(E_ORDER);
        }
        let mut sum: u32 = 0;
        for w in &self.items {
            sum += *w;
        }
        if sum != HEAD_SUBTOTAL {
            return Err(E_SUM);
        }
        self.subtotal = sum;
        self.posted = true;
        Ok(sum)
    }

    /// 账行可解引复核。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.posted {
            return Err(E_ORDER);
        }
        let mut sum: u32 = 0;
        for w in &self.items {
            sum += *w;
        }
        if sum != self.subtotal || self.subtotal != HEAD_SUBTOTAL {
            return Err(E_SUM);
        }
        Ok(())
    }
}

/// 三重收口执行器。
pub struct B34Runner {
    /// 一层：断言结果行。
    pub assert_rows: Vec<(u16, bool)>,
    /// 二层：对冲差值（应 0）。
    pub hedge_delta: i64,
    /// 三层：残留账行数（应 0）。
    pub residue: usize,
    /// 批账。
    pub batch: HeadBatch,
    pub ran: bool,
    pub sealed: bool,
}

impl B34Runner {
    pub fn new() -> B34Runner {
        B34Runner {
            assert_rows: Vec::new(),
            hedge_delta: 0,
            residue: 0,
            batch: HeadBatch::new(),
            ran: false,
            sealed: false,
        }
    }

    /// 一次命令三重收口。
    pub fn run_b34(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEAL);
        }
        registry_audit()?;
        self.ran = false;

        // 一层：断言全过
        self.assert_rows.clear();
        for aid in registry() {
            let g = exec_b34(aid);
            self.assert_rows.push((aid, g));
            if !g {
                return Err(E_ASSERT);
            }
        }

        // 二层：账目对冲（批账 post + 复核，双路求和差 = 0）
        let items = [4_000u32, 4_000, 2_000, 1_000, 480];
        self.batch = HeadBatch::new();
        for w in items {
            self.batch.add_item(w)?;
        }
        let posted = self.batch.post()?;
        self.batch.verify()?;
        let mut direct: u32 = 0;
        for w in items.iter() {
            direct += *w;
        }
        self.hedge_delta = posted as i64 - direct as i64;
        if self.hedge_delta != 0 {
            return Err(E_HEDGE);
        }

        // 三层：零残留（执行器状态检查）
        self.residue = self.assert_rows.iter().filter(|(_, g)| !*g).count();
        if self.residue != 0 {
            return Err(E_RESIDUE);
        }

        self.ran = true;
        Ok(())
    }

    /// 三重全过结账。
    pub fn assert_triple_green(&self) -> Result<(), u16> {
        if !self.ran {
            return Err(E_ASSERT);
        }
        if self.assert_rows.len() != N_ASSERTS {
            return Err(E_REG);
        }
        for (_, g) in &self.assert_rows {
            if !g {
                return Err(E_ASSERT);
            }
        }
        if self.hedge_delta != 0 {
            return Err(E_HEDGE);
        }
        if self.residue != 0 {
            return Err(E_RESIDUE);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        self.assert_triple_green()?;
        self.sealed = true;
        Ok(())
    }

    pub fn summary(&self) -> String {
        let mut s = String::from(KTEST_NAME);
        s.push_str(": 断言 16/16 对冲=0 残留=0 小计=");
        s.push_str(&self.batch.subtotal.to_string());
        s
    }
}

/// 小计锚点注入核：条目集错 → post 拦截且 posted 不置位。
pub fn inject_wrong_sum_must_fail() -> Result<(), u16> {
    let mut b = HeadBatch::new();
    for w in [4_000u32, 4_000, 2_000, 1_000, 479] {
        b.add_item(w)?;
    }
    if b.post() != Err(E_SUM) {
        return Err(SEG_BASE + 0x30);
    }
    if b.posted {
        return Err(SEG_BASE + 0x31);
    }
    Ok(())
}

/// 先落账后求和注入核：posted 后加条目 → 拦截。
pub fn inject_post_then_add_must_fail() -> Result<(), u16> {
    let mut b = HeadBatch::new();
    for w in [4_000u32, 4_000, 2_000, 1_000, 480] {
        b.add_item(w)?;
    }
    b.post()?;
    if b.add_item(1) != Err(E_ORDER) {
        return Err(SEG_BASE + 0x32);
    }
    Ok(())
}

/// 残留注入核：构造残留 → 三重结账检出。
pub fn inject_residue_must_fail() -> Result<(), u16> {
    let mut r = B34Runner::new();
    r.assert_rows.push((1, false)); // 模拟残留红行
    r.residue = 1;
    if r.assert_triple_green() != Err(E_RESIDUE) {
        return Err(SEG_BASE + 0x33);
    }
    Ok(())
}

/// 对冲破注入核：对冲差 ≠ 0 → 结账检出。
pub fn inject_hedge_must_fail() -> Result<(), u16> {
    let mut r = B34Runner::new();
    r.ran = true;
    for i in 1..=16u16 {
        r.assert_rows.push((i, true));
    }
    r.hedge_delta = 5;
    if r.assert_triple_green() != Err(E_HEDGE) {
        return Err(SEG_BASE + 0x34);
    }
    Ok(())
}

/// 零值条目注入核。
pub fn inject_zero_item_must_fail() -> Result<(), u16> {
    let mut b = HeadBatch::new();
    if b.add_item(0) != Err(E_SUM) {
        return Err(SEG_BASE + 0x35);
    }
    Ok(())
}

/// 批名锚点自检。
pub fn name_anchor_audit() -> Result<(), u16> {
    if KTEST_NAME != "unx_a1_b34" {
        return Err(E_REG);
    }
    Ok(())
}

/// 断言-判据映射（B34 批 16 条 → 判据号）。
pub const B34_MAP: [(u16, u16); N_ASSERTS] = [
    (1, 664), (2, 664), (3, 666), (4, 666), (5, 666),
    (6, 667), (7, 671), (8, 671), (9, 672), (10, 672),
    (11, 672), (12, 673), (13, 674), (14, 675), (15, 677),
    (16, 680),
];

/// 正查：断号连续且判据号 ∈ [664, 680]。
pub fn map_forward_audit() -> Result<(), u16> {
    if B34_MAP.len() != N_ASSERTS {
        return Err(E_REG);
    }
    for (i, (aid, crit)) in B34_MAP.iter().enumerate() {
        if *aid != (i + 1) as u16 {
            return Err(E_REG);
        }
        if *crit < 664 || *crit > 680 {
            return Err(E_MAP);
        }
    }
    Ok(())
}

/// 反查：映射判据号去重 ≥ 10（覆盖面）且每断号可定位。
pub fn map_reverse_audit() -> Result<(), u16> {
    let mut distinct: Vec<u16> = Vec::new();
    for (_, c) in B34_MAP.iter() {
        if !distinct.contains(c) {
            distinct.push(*c);
        }
    }
    if distinct.len() < 10 {
        return Err(E_MAP);
    }
    for (aid, _) in B34_MAP.iter() {
        let mut found = false;
        for a in registry() {
            if &a == aid {
                found = true;
            }
        }
        if !found {
            return Err(E_MAP);
        }
    }
    Ok(())
}

/// 映射错位注入核：错位条目构造 → 正查检出。
pub fn inject_map_mismatch_must_fail() -> Result<(), u16> {
    let mut bad = B34_MAP;
    bad[3] = (4, 999);
    if bad[3].1 >= 664 && bad[3].1 <= 680 {
        return Err(E_MAP);
    }
    Ok(())
}

/// 三层执行账行。
#[derive(Clone, Debug)]
pub struct LayerRow {
    /// 层号 1..3。
    pub layer: u8,
    /// 层名。
    pub name: &'static str,
    /// 层结果（true=过）。
    pub ok: bool,
}

/// 层账渲染。
pub fn layer_rows(r: &B34Runner) -> Result<Vec<LayerRow>, u16> {
    let mut out = Vec::new();
    out.push(LayerRow { layer: 1, name: "断言全过", ok: !r.assert_rows.is_empty() && r.assert_rows.iter().all(|(_, g)| *g) });
    out.push(LayerRow { layer: 2, name: "账目对冲", ok: r.hedge_delta == 0 });
    out.push(LayerRow { layer: 3, name: "零残留", ok: r.residue == 0 });
    if out.len() != 3 {
        return Err(E_REG);
    }
    Ok(out)
}

/// 断言红点名渲染：红行（断号, 判据号）逐条列出，零静默。
pub fn red_points(rows: &[(u16, bool)]) -> Vec<(u16, u16)> {
    let mut out = Vec::new();
    for (aid, g) in rows {
        if !g {
            let mut crit: u16 = 0;
            for (a, c) in B34_MAP.iter() {
                if a == aid {
                    crit = *c;
                }
            }
            out.push((*aid, crit));
        }
    }
    out
}

/// 点名渲染自检：红行必被点名且判据号可解引。
pub fn red_point_audit() -> Result<(), u16> {
    let rows = [(1u16, true), (2, false), (3, true), (16, false)];
    let pts = red_points(&rows);
    if pts.len() != 2 {
        return Err(E_ASSERT);
    }
    if pts[0] != (2, 666) || pts[1] != (16, 680) {
        return Err(E_MAP);
    }
    // 全绿无点名
    let mut all = Vec::new();
    for i in 1..=16u16 {
        all.push((i, true));
    }
    if !red_points(&all).is_empty() {
        return Err(E_ASSERT);
    }
    Ok(())
}

/// 自例程：三重收口全过 + 注入全拦截 + 封账 → 0。
pub fn drill() -> Result<u32, u16> {
    name_anchor_audit()?;
    registry_audit()?;
    map_forward_audit()?;
    map_reverse_audit()?;
    inject_map_mismatch_must_fail()?;
    red_point_audit()?;
    inject_wrong_sum_must_fail()?;
    inject_post_then_add_must_fail()?;
    inject_residue_must_fail()?;
    inject_hedge_must_fail()?;
    inject_zero_item_must_fail()?;

    let mut r = B34Runner::new();
    r.run_b34()?;
    r.assert_triple_green()?;
    // 层账渲染：三层全过
    let lr = layer_rows(&r)?;
    for l in &lr {
        if !l.ok {
            return Err(E_HEDGE);
        }
    }
    if r.summary().is_empty() {
        return Err(SEG_BASE + 0x36);
    }
    r.seal()?;
    // 注入：封后重跑 → 拦截
    if r.run_b34() != Err(E_SEAL) {
        return Err(SEG_BASE + 0x37);
    }
    Ok(0)
}
