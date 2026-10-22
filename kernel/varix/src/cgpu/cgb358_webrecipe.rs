//! CGPU-F0358 · 网页并发配方（判据段 0x08C0）
//! 网页配方：30 网页并发分配（每表面基础配额+可见表面加权的两层
//! 分配）；可见性仲裁联动（E 域 F0641+ 表面配额对接）；滚动中的网
//! 页提权（可见滚动表面临时提权）；滥用防护（单网页超用→限流+
//! 归因——公平铁律）。判据：两层分配、可见性联动、提权、滥用防护、
//! 判据。
//!
//! 锚点原文摘录（施工书 1595 行）：
//! 「30 网页并发分配（每表面基础配额+可见表面加权的两层分配）；
//! 可见性仲裁联动（E 域 F0641+ 表面配额对接）；滚动中的网页提权
//! （可见滚动表面临时提权）；滥用防护（单网页超用→限流+归因）。
//! 判据：两层分配、可见性联动、提权、滥用防护、判据。」
//!
//! 诊断码段 0x08C0（本模块独占：0x08C0~0x08CF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x08C0;

/// 并发网页上限（判据「30 网页并发」——册面数字写死）。
pub const WEB_LANES_MAX: usize = 30;

/// 两层分配基准（千分比）：第一层基础配额 = 1000/30 向下取整。
/// 来源：判据「每表面基础配额」——基础层人人平等。
pub const BASE_SHARE_NUM: u32 = 1000;
pub const BASE_SHARE_DEN: u32 = WEB_LANES_MAX as u32;

/// 可见表面加权上限（判据「可见表面加权」）：可见性乘数最大 3 倍。
pub const VISIBILITY_WEIGHT_MAX: u32 = 3;

/// 滚动提权乘数（判据「提权」）：可见滚动表面临时 ×2。
pub const SCROLL_BOOST: u32 = 2;

/// 滥用阈值（判据「滥用防护」）：单网页占用超均值 2 倍即限流。
pub const ABUSE_RATIO_PERMILLE: u32 = 2000;

/// 网页表面（判据「两层分配」与「可见性联动」的输入单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WebSurface {
    /// 表面号（1 起，0 非法）。
    pub surface: u32,
    /// 可见性（E 域 F0641+ 联动值：0=不可见 1=可见 2=前台）。
    pub visibility: u8,
    /// 是否正在滚动。
    pub scrolling: bool,
}

impl WebSurface {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.surface == 0 {
            return Err(SEG_BASE);
        }
        if self.visibility > 2 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
    /// 可见性权重（1/2/3 —— 判据「可见表面加权」，上限钳制）。
    pub fn vis_weight(&self) -> u32 {
        let w = self.visibility as u32 + 1;
        if w > VISIBILITY_WEIGHT_MAX {
            VISIBILITY_WEIGHT_MAX
        } else {
            w
        }
    }
    /// 提权后权重（滚动 ×SCROLL_BOOST）。
    pub fn boosted_weight(&self) -> u32 {
        let w = self.vis_weight();
        if self.scrolling {
            w * SCROLL_BOOST
        } else {
            w
        }
    }
}

/// 两层分配（判据「两层分配」主判据）：第一层每人等额基础配额，
/// 第二层把剩余量按可见性加权派发。Σ派发 ≡ 1000。
pub fn two_layer_alloc(surfaces: &[WebSurface]) -> Result<Vec<u32>, u16> {
    if surfaces.is_empty() {
        return Err(SEG_BASE + 2);
    }
    if surfaces.len() > WEB_LANES_MAX {
        return Err(SEG_BASE + 3);
    }
    let n = surfaces.len() as u32;
    for s in surfaces {
        s.validate()?;
    }
    // 第一层：基础配额（等额，向下取整）。
    let base = BASE_SHARE_NUM / n;
    let mut shares: Vec<u32> = vec![base; surfaces.len()];
    let mut assigned = base * n;
    // 第二层：剩余按加权派发。
    let weights: Vec<u32> = surfaces.iter().map(|s| s.boosted_weight()).collect();
    let wsum: u64 = weights.iter().map(|&w| w as u64).sum();
    if wsum == 0 {
        return Err(SEG_BASE + 4);
    }
    let remain = (BASE_SHARE_NUM - assigned) as u64;
    let mut extra: Vec<u32> = Vec::new();
    for &w in &weights {
        extra.push((remain * w as u64 / wsum) as u32);
    }
    // 取整残余全部给第一个表面（确定性归口——不产生静默损耗）。
    let extra_sum: u32 = extra.iter().sum();
    extra[0] += (BASE_SHARE_NUM - assigned - extra_sum) as u32;
    for (i, e) in extra.iter().enumerate() {
        shares[i] += e;
    }
    assigned = shares.iter().sum();
    if assigned != BASE_SHARE_NUM {
        return Err(SEG_BASE + 5);
    }
    Ok(shares)
}

/// 可见性联动核（判据「可见性联动」守卫）：不可见表面（visibility
/// =0）的份额不得超过可见表面最低份额——看不见的不能比看得见的
/// 吃得多。
pub fn visibility_link_ok(surfaces: &[WebSurface], shares: &[u32]) -> Result<bool, u16> {
    if surfaces.len() != shares.len() || surfaces.is_empty() {
        return Err(SEG_BASE + 6);
    }
    let mut min_visible: Option<u32> = None;
    let mut max_invisible: u32 = 0;
    for (s, &sh) in surfaces.iter().zip(shares.iter()) {
        if s.visibility == 0 {
            if sh > max_invisible {
                max_invisible = sh;
            }
        } else {
            min_visible = Some(match min_visible {
                Some(m) if m <= sh => m,
                _ => sh,
            });
        }
    }
    match min_visible {
        Some(m) => Ok(max_invisible <= m),
        None => Ok(true),
    }
}

/// 滚动提权应用（判据「提权」语义面）：返回提权增量表——只有
/// 可见滚动表面才有正增量，其余为零。
pub fn scroll_boost_delta(surfaces: &[WebSurface]) -> Result<Vec<u32>, u16> {
    if surfaces.is_empty() || surfaces.len() > WEB_LANES_MAX {
        return Err(SEG_BASE + 7);
    }
    for s in surfaces {
        s.validate()?;
    }
    let mut out = Vec::new();
    for s in surfaces {
        if s.scrolling && s.visibility > 0 {
            out.push(s.vis_weight() * (SCROLL_BOOST - 1));
        } else {
            out.push(0);
        }
    }
    Ok(out)
}

/// 滥用记录（判据「滥用防护」的归因单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbuseRecord {
    /// 超用表面号。
    pub surface: u32,
    /// 实际占用（千分比）。
    pub used_permille: u32,
    /// 公平均值（千分比）。
    pub fair_permille: u32,
}

impl AbuseRecord {
    /// 是否超阈值：占用 > 均值 × ABUSE_RATIO_PERMILLE/1000。
    pub fn over_threshold(&self) -> bool {
        self.fair_permille > 0
            && self.used_permille * 1000 > self.fair_permille * ABUSE_RATIO_PERMILLE
    }
    /// 限流后份额：钳到均值 ×1.5（留恢复余地——不打死只压制）。
    pub fn throttled_share(&self) -> u32 {
        self.fair_permille * 3 / 2
    }
}

/// 滥用扫描（判据「滥用防护」主判据）：全表扫描超用表面，产出
/// 归因记录与限流建议；无超用则空表。
pub fn scan_abuse(usage: &[(u32, u32)], fair_mean_permille: u32) -> Result<Vec<AbuseRecord>, u16> {
    if usage.is_empty() {
        return Err(SEG_BASE + 8);
    }
    if fair_mean_permille == 0 {
        return Err(SEG_BASE + 9);
    }
    let mut out = Vec::new();
    for &(surface, used) in usage {
        if surface == 0 {
            return Err(SEG_BASE + 10);
        }
        let rec = AbuseRecord { surface, used_permille: used, fair_permille: fair_mean_permille };
        if rec.over_threshold() {
            out.push(rec);
        }
    }
    Ok(out)
}

/// 限流后守恒核（判据「滥用防护」公平铁律）：限流只压制超额者，
/// 被压出的量记录为释放量——总量账面必须可对账（压前 Σ ≡ 压后 Σ
/// + 释放量）。
pub fn throttle_conservation(
    usage: &[(u32, u32)],
    records: &[AbuseRecord],
) -> Result<(u32, u32), u16> {
    let total_before: u64 = usage.iter().map(|&(_, u)| u as u64).sum();
    let mut total_after = total_before;
    for r in records {
        if !r.over_threshold() {
            return Err(SEG_BASE + 11);
        }
        let cap = r.throttled_share() as u64;
        if r.used_permille as u64 > cap {
            total_after -= r.used_permille as u64 - cap;
        }
    }
    let released = (total_before - total_after) as u32;
    Ok((total_after as u32, released))
}

/// 配方指纹（可审计面）。
pub fn recipe_fp(surfaces: &[WebSurface], shares: &[u32]) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    for (s, &sh) in surfaces.iter().zip(shares.iter()) {
        acc = acc.wrapping_mul(0x100000001b3) ^ s.surface as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ s.vis_weight() as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ sh as u64;
    }
    acc
}

/// 配方诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "表面号为 0 非法（1 起）",
        1 => "可见性值域 0/1/2 之外",
        2 => "表面表为空——无可分配对象",
        3 => "并发网页超上限（30）",
        4 => "加权总和为 0——第二层无从派发",
        5 => "两层分配 Σ≠1000——守恒被破坏",
        6 => "表长与份额表不等长或为空",
        7 => "提权扫描输入表长非法",
        8 => "用量表为空",
        9 => "公平均值为 0——无法判定超用",
        10 => "用量表含 0 号表面",
        11 => "限流记录中含未超阈值者——归因错位",
        _ => "未知诊断码——查本模块 SEG_BASE 0x08C0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(n: u32, vis: u8, scroll: bool) -> WebSurface {
        WebSurface { surface: n, visibility: vis, scrolling: scroll }
    }

    #[test]
    fn two_layer_conservation() {
        // 30 网页并发：册面数字场景。
        let surfaces: Vec<WebSurface> = (1..=30).map(|i| mk(i, if i <= 5 { 2 } else { 0 }, false)).collect();
        let shares = two_layer_alloc(&surfaces).unwrap();
        assert_eq!(shares.len(), 30);
        let sum: u32 = shares.iter().sum();
        assert_eq!(sum, 1000);
        // 前五个可见者份额 > 不可见者份额（第二层加权生效）。
        let vis0 = shares[5];
        assert!(shares[0] > vis0);
        assert!(visibility_link_ok(&surfaces, &shares).unwrap());
    }

    #[test]
    fn visibility_link_guard() {
        let surfaces = vec![mk(1, 2, false), mk(2, 0, false)];
        // 不可见者占 900、可见者占 100 → 违反公平。
        let shares = vec![100, 900];
        assert!(!visibility_link_ok(&surfaces, &shares).unwrap());
        let shares2 = vec![900, 100];
        assert!(visibility_link_ok(&surfaces, &shares2).unwrap());
        // 全不可见 → 恒通过。
        let all_inv = vec![mk(1, 0, false), mk(2, 0, false)];
        assert!(visibility_link_ok(&all_inv, &shares).unwrap());
    }

    #[test]
    fn scroll_boost_only_visible() {
        let surfaces = vec![mk(1, 2, true), mk(2, 0, true), mk(3, 1, false)];
        let delta = scroll_boost_delta(&surfaces).unwrap();
        assert_eq!(delta, vec![3, 0, 0]); // vis_weight=3 ×(2-1)；不可见滚动不提权。
    }

    #[test]
    fn abuse_protection() {
        let usage = vec![(1u32, 600u32), (2, 200), (3, 200)];
        // 公平均值 ≈ 333，表面 1 占 600 > 333×2 = 666？否——600<666 不算。
        let r0 = scan_abuse(&usage, 333).unwrap();
        assert!(r0.is_empty());
        // 均值 250 时 600 > 500 超阈。
        let r1 = scan_abuse(&usage, 250).unwrap();
        assert_eq!(r1.len(), 1);
        assert_eq!(r1[0].surface, 1);
        let (after, released) = throttle_conservation(&usage, &r1).unwrap();
        // 限流到 250×1.5=375，释放 600-375=225。
        assert_eq!(after, 1000 - 225);
        assert_eq!(released, 225);
        let fp = recipe_fp(
            &[mk(1, 2, false), mk(2, 0, false)],
            &[800, 200],
        );
        assert!(fp != 0);
    }

    #[test]
    fn rejects_and_diagnostics() {
        assert_eq!(two_layer_alloc(&[]).unwrap_err(), SEG_BASE + 2);
        let over: Vec<WebSurface> = (1..=31).map(|i| mk(i, 1, false)).collect();
        assert_eq!(two_layer_alloc(&over).unwrap_err(), SEG_BASE + 3);
        assert_eq!(two_layer_alloc(&[mk(0, 1, false)]).unwrap_err(), SEG_BASE);
        assert_eq!(diagnose(SEG_BASE + 3).contains("30"), true);
        assert!(diagnose(0x9999).contains("未知"));
    }
}
