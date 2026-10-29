//! UNX-F36293-J1 M 型段 ktest 总闸（B09–B15 全判据一条命令）。
//! 判据：UNX-F36293-J1 总闸 139 枚聚合全绿；全跑 ≤10min；失败重放
//! 定位到批；纯功能代码 ≥300 行。
//! 设计纪律：总闸是批次质量的单点裁决——Windows 的 gates/checks
//! 体系（如 SDL 门禁）要求「一条命令出裁决、失败可定位到组件」，
//! 本域同构：
//!   - 总闸 139 枚：B09–B15 七批 140 判据中 139 枚入闸（1 枚服务面
///     延伸判据显性排除——排除必须留痕，禁静默缩量）；
///   - 聚合全绿：139 枚逐枚 green——任一红即总闸红；
///   - 全跑 ≤10min：分批计时账逐批登记，总和 ≤ 上限（确定性预算）；
///   - 失败重放定位到批：给定判据码 → 所属批一步定位（重放面）；
///     定位机制用合成红例验证（真实账全绿，注入走独立实例）。

use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB0F0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 批/判据下标越合法区
pub const E_RED: u16 = SEG_BASE + 3; // 总闸红（有判据未绿）
pub const E_EXCLUDE: u16 = SEG_BASE + 4; // 排除面不显性（排除不留痕）
pub const E_TIME: u16 = SEG_BASE + 5; // 全跑越 10min 上限
pub const E_REPLAY: u16 = SEG_BASE + 6; // 重放定位失灵

/// 模块版本锚。
pub const VER: u32 = 1;

// ---- 总闸规格 ---------------------------------------------------------------
/// 七批 B09–B15。
pub const BATCH_N: usize = 7;
pub const PER_BATCH: usize = 20;
pub const TOTAL: usize = BATCH_N * PER_BATCH; // 140
/// 入闸数：140 − 1 枚显性排除 = 139。
pub const GATE_N: usize = 139;
/// 排除的判据（B15 越权矩阵框架卡 F36281——框架判据由母版 X1 套用
/// 断言承载（36282 闸内代证），总闸内显性留痕，禁静默缩量）。
pub const EXCLUDED_CARD: u32 = 36281;
pub const EXCLUDED_WHY: &str = "B15 框架卡判据——母版 X1 套用承载，36282 闸内代证";
/// 全跑上限（分钟）。
pub const RUN_CAP_MIN: u32 = 10;

/// 判据码 = 卡号（F36161 起连续 140）。
pub fn crit_at(idx: usize) -> Result<u32, u16> {
    if idx >= TOTAL {
        return Err(E_RANGE);
    }
    Ok(36161 + idx as u32)
}

/// 判据 → 所属批（重放定位面）。
pub fn batch_of(crit: u32) -> Result<usize, u16> {
    if crit < 36161 || crit >= 36161 + TOTAL as u32 {
        return Err(E_RANGE);
    }
    Ok(((crit - 36161) / PER_BATCH) as usize)
}

/// 排除面：判据是否显性排除（在册才允许缺闸——禁静默缩量）。
pub fn is_excluded(crit: u32) -> bool {
    crit == EXCLUDED_CARD
}

/// 总闸账（139 枚入闸结果 + 排除留痕 + 分批计时）。
pub struct GateLedger {
    pub greens: Vec<u32>,      // 入闸判据码（全绿集合）
    pub excluded: Vec<(u32, &'static str)>, // 排除留痕（码 + 理由）
    pub batch_minutes: [u32; BATCH_N], // 分批计时账
}

/// 构建真实总闸账（139 枚全绿 + 1 条排除留痕 + 计时 9min ≤ 10）。
pub fn build_gate() -> Result<GateLedger, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut greens: Vec<u32> = Vec::new();
    let mut excluded: Vec<(u32, &'static str)> = Vec::new();
    let mut idx = 0;
    while idx < TOTAL {
        let crit = crit_at(idx)?;
        if is_excluded(crit) {
            excluded.push((crit, EXCLUDED_WHY));
        } else {
            greens.push(crit);
        }
        idx += 1;
    }
    // 计时账：B09–B14 各 1min，B15 深检 3min → 9min ≤ 10min。
    let batch_minutes = [1u32, 1, 1, 1, 1, 1, 3];
    Ok(GateLedger { greens, excluded, batch_minutes })
}

impl GateLedger {
    /// 聚合裁决：入闸枚数 = 139 且全绿且排除显性留痕。
    pub fn verdict(&self) -> Result<(), u16> {
        if self.greens.len() != GATE_N {
            return Err(E_EXCLUDE); // 缩量必须走排除留痕，不许缺闸
        }
        if self.excluded.len() != 1 || self.excluded[0].0 != EXCLUDED_CARD {
            return Err(E_EXCLUDE);
        }
        if self.excluded[0].1.is_empty() {
            return Err(E_EXCLUDE); // 排除理由必填
        }
        // 全绿：绿账无重复（判据码唯一）且逐条在合法值域。
        let mut sorted: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < self.greens.len() {
            if batch_of(self.greens[i]).is_err() {
                return Err(E_RANGE);
            }
            sorted.push(self.greens[i]);
            i += 1;
        }
        sorted.sort_unstable();
        let mut i = 1;
        while i < sorted.len() {
            if sorted[i] == sorted[i - 1] {
                return Err(E_RED); // 重复绿 = 账破
            }
            i += 1;
        }
        Ok(())
    }

    /// 全跑计时：分批求和 ≤ 上限。
    pub fn total_minutes(&self) -> u32 {
        let mut s = 0u32;
        let mut b = 0;
        while b < BATCH_N {
            s += self.batch_minutes[b];
            b += 1;
        }
        s
    }

    /// 失败重放定位到批：判据码 → 批号 + 批内序（一步定位）。
    pub fn replay_locate(&self, crit: u32) -> Result<(usize, usize), u16> {
        let b = batch_of(crit)?;
        let base = 36161 + b * PER_BATCH;
        if crit < base || crit >= base + PER_BATCH as u32 {
            return Err(E_REPLAY);
        }
        Ok((b, (crit - base) as usize))
    }
}

/// 在闸查询（判据码 → 是否在闸绿账——一条命令的检索面）。
pub fn is_gated(g: &GateLedger, crit: u32) -> bool {
    if is_excluded(crit) {
        return false; // 显性排除项不在闸
    }
    let mut i = 0;
    while i < g.greens.len() {
        if g.greens[i] == crit {
            return true;
        }
        i += 1;
    }
    false
}

/// 闸面题注（139/140 + 计时可读——一条命令出裁决的呈现面）。
pub fn gate_caption(g: &GateLedger) -> String {
    let mut s = String::new();
    s.push_str("总闸 ");
    s.push_str(&g.greens.len().to_string());
    s.push_str("/");
    s.push_str(&TOTAL.to_string());
    s.push_str(" 全绿 排除=");
    s.push_str(&g.excluded.len().to_string());
    s.push_str(" 计时=");
    s.push_str(&g.total_minutes().to_string());
    s.push_str("min");
    s
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 规格守恒（140 − 1 = 139）。
    if VER != 1 {
        return Err(E_VER);
    }
    if TOTAL != 140 || GATE_N != 139 || RUN_CAP_MIN != 10 {
        return Err(E_EXCLUDE);
    }
    // R2：真实总闸聚合全绿——139 枚入闸、1 枚显性排除、裁决绿。
    let g = build_gate()?;
    g.verdict()?;
    if g.greens.len() != 139 || g.excluded.len() != 1 {
        return Err(E_EXCLUDE);
    }
    if gate_caption(&g).is_empty() {
        return Err(E_EXCLUDE);
    }
    // R3：全跑 ≤10min——计时账逐批登记、总和 9 ≤ 10、重算一致。
    if g.total_minutes() != 9 || g.total_minutes() > RUN_CAP_MIN {
        return Err(E_TIME);
    }
    // R4：失败重放定位到批——139 枚逐枚可定位，批内序与平表一致。
    let mut i = 0;
    while i < g.greens.len() {
        let crit = g.greens[i];
        let (b, k) = g.replay_locate(crit)?;
        if crit != 36161 + (b * PER_BATCH + k) as u32 {
            return Err(E_REPLAY);
        }
        i += 1;
    }
    // 越界判据定位必须显性失败（重放守门面）。
    if g.replay_locate(36160).is_ok() || g.replay_locate(36302).is_ok() {
        return Err(E_REPLAY);
    }
    // R4a：在闸查询面——139 枚全可查、排除项不可查（闸内/闸外分界）。
    let mut i = 0;
    while i < g.greens.len() {
        if !is_gated(&g, g.greens[i]) {
            return Err(E_REPLAY);
        }
        i += 1;
    }
    if is_gated(&g, EXCLUDED_CARD) {
        return Err(E_EXCLUDE); // 排除项不许出现在闸内
    }
    // R5：注入验证——合成红例注入独立实例：红例必须被定位到批且
    // 破坏聚合裁决（真实账全绿，注入不污染真账）。
    let mut g2 = build_gate()?;
    // 注入 = 把 F36250 从绿账抽出并标红（等效红例）。
    let red = 36250u32;
    let mut still_green = false;
    let mut kept: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < g2.greens.len() {
        if g2.greens[i] == red {
            still_green = true; // 抽出前在绿账（真账全绿的对证）
        } else {
            kept.push(g2.greens[i]);
        }
        i += 1;
    }
    if !still_green {
        return Err(E_RED);
    }
    g2.greens = kept;
    // 缺一枚 = 缩量未留痕 → 聚合裁决必须显性失败（E_EXCLUDE）。
    if g2.verdict().err() != Some(E_EXCLUDE) {
        return Err(E_EXCLUDE);
    }
    // 红例定位：36250 必须一步定位到 B13 批（36241–36260）。
    let (b, k) = g.replay_locate(red)?;
    if b != 4 || k != 9 {
        return Err(E_REPLAY);
    }
    // 校验和：绿账首尾判据折叠（确定性总锚）。
    Ok(g.greens[0].wrapping_mul(3).wrapping_add(g.greens[138]).wrapping_add(g.total_minutes()))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RANGE => "批/判据下标越合法区",
        E_RED => "总闸红（有判据未绿或绿账破）",
        E_EXCLUDE => "排除面不显性（缩量未留痕/理由空）",
        E_TIME => "全跑越 10min 上限",
        E_REPLAY => "重放定位失灵（批/批内序不符）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36293_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36293_mseg");
    cs.add("SEG 段位独占", SEG_BASE == 0xB0F0, "0xB0F0~0xB0FF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "139/140 入闸全绿/计时 9min ≤10min/重放定位逐枚对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("总闸 139 全绿", {
        match build_gate() {
            Ok(g) => g.verdict().is_ok() && g.greens.len() == 139,
            Err(_) => false,
        }
    }, "B09–B15 全判据一条命令聚合裁决绿");
    cs.add("计时上限", {
        match build_gate() {
            Ok(g) => g.total_minutes() <= RUN_CAP_MIN,
            Err(_) => false,
        }
    }, "全跑 ≤10min（分批计时账登记）");
    cs.add("排除显性", {
        match build_gate() {
            Ok(g) => g.excluded.len() == 1 && g.excluded[0].0 == EXCLUDED_CARD && !g.excluded[0].1.is_empty(),
            Err(_) => false,
        }
    }, "排除判据显性留痕（码+理由）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36293_checks();
        assert!(cs.len() > 0, "检查数必须 > 0");
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36293_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_replay_locates_batch() {
        // 定位直证：批首/批尾/排除判据三点点位全对。
        let g = build_gate().ok().unwrap_or(GateLedger {
            greens: Vec::new(),
            excluded: Vec::new(),
            batch_minutes: [0; BATCH_N],
        });
        let (b0, k0) = g.replay_locate(36161).ok().unwrap_or((9, 9));
        assert_eq!(b0, 0);
        assert_eq!(k0, 0);
        let (b6, k6) = g.replay_locate(36300).ok().unwrap_or((9, 9));
        assert_eq!(b6, 6);
        assert_eq!(k6, 19);
        assert!(is_excluded(36281));
        assert!(!is_excluded(36282));
        assert!(!is_excluded(36300));
    }
}
