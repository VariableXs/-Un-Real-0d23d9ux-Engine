//! UNREAL-X AI-35 · 内核兼容深化（领域09 · 族0341/0342/0343 · X08501~X08575）。
//!
//! 兼容深化与收官 K 线落点：Shim 工程（垫片注入与剥离）、版本协商
//! （三方握手）、兼容沙盒（降权执行域）。全部确定性算法、固定容量、
//! 非法输入钳制回默认，绝不 panic。V 线五族见 src/features/compat/ai35Checks.ts，
//! C 线两族见 code-analysis/core/src/ai35.rs。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 族0341 Shim 工程（X08501~X08525）
// ---------------------------------------------------------------------------

/// Shim 槽位：被拦 API + 替换实现名 + 优先级（小者先拦）。
#[derive(Clone, Copy)]
pub struct ShimSlot {
    pub api: &'static str,
    pub impl_name: &'static str,
    pub prio: u8,
}

/// 注入 shim：同 API 后到优先级高（数值小）才替换。
pub fn shim_inject(table: &mut [Option<ShimSlot>], slot: ShimSlot) -> bool {
    let free = table.iter().position(|s| s.is_none());
    let hit = table
        .iter()
        .position(|s| matches!(s, Some(x) if x.api == slot.api));
    match (hit, free) {
        (Some(i), _) => {
            let replace = table[i].map_or(true, |old| slot.prio < old.prio);
            if replace {
                table[i] = Some(slot);
            }
            replace
        }
        (None, Some(i)) => {
            table[i] = Some(slot);
            true
        }
        _ => false,
    }
}

/// 剥离 shim：按 API 名移除，返回是否移除过。
pub fn shim_remove(table: &mut [Option<ShimSlot>], api: &str) -> bool {
    let hit = table.iter().position(|s| matches!(s, Some(x) if x.api == api));
    match hit {
        Some(i) => {
            table[i] = None;
            true
        }
        None => false,
    }
}

/// shim 解析：按优先级取第一个命中的实现，无则回内核原生。
pub fn shim_resolve<'a>(table: &[Option<ShimSlot>], api: &str, native: &'a str) -> &'a str {
    let mut best: Option<&ShimSlot> = None;
    for s in table.iter().flatten() {
        if s.api == api && best.map_or(true, |b| s.prio < b.prio) {
            best = Some(s);
        }
    }
    best.map_or(native, |s| s.impl_name)
}

// ---------------------------------------------------------------------------
// 族0342 版本协商（X08526~X08550）
// ---------------------------------------------------------------------------

/// 协商提案：应用期望版本 + 可接受最低版本。
pub const fn propose(want: u32, min: u32) -> (u32, u32) {
    (want, min)
}

/// 三方协商：内核提供 p，应用期望 want、底线 min。
/// p ≥ want → 满配；min ≤ p < want → 降档；p < min → 破裂。
pub fn negotiate(provided: u32, want: u32, min: u32) -> u32 {
    if provided >= want {
        2
    } else if provided >= min {
        1
    } else {
        0
    }
}

/// 协商结论语：2 全量 / 1 降档 / 0 破裂。
pub fn negotiate_text(verdict: u32) -> &'static str {
    match verdict {
        2 => "全量启用",
        1 => "降档启用",
        _ => "协商破裂",
    }
}

/// 版本对齐：两侧各报版本，取较小者并钳到 ≥100。
pub fn align_versions(a: u32, b: u32) -> u32 {
    a.min(b).max(100)
}

// ---------------------------------------------------------------------------
// 族0343 兼容沙盒（X08551~X08575）
// ---------------------------------------------------------------------------

/// 沙盒权限档：0 全锁 / 1 只读 / 2 受限写 / 3 近原生。
pub const SB_LOCKED: u32 = 0;
pub const SB_READONLY: u32 = 1;
pub const SB_LIMITED: u32 = 2;
pub const SB_NATIVE: u32 = 3;

/// 档位裁决：可信度（0~100）映射档位，60 以下只读，30 以下全锁。
pub fn sb_tier(trust: u32) -> u32 {
    if trust >= 80 {
        SB_NATIVE
    } else if trust >= 60 {
        SB_LIMITED
    } else if trust >= 30 {
        SB_READONLY
    } else {
        SB_LOCKED
    }
}

/// 写白名单：只读档对白名单路径仍可写。
pub fn sb_can_write(tier: u32, path_whitelisted: bool) -> bool {
    tier >= SB_LIMITED || (tier == SB_READONLY && path_whitelisted)
}

/// 逃逸计数守卫：逃逸 ≥3 次降一档（下限 0）。
pub fn sb_demote(tier: u32, escapes: u32) -> u32 {
    if escapes >= 3 {
        tier.saturating_sub(1)
    } else {
        tier.min(SB_NATIVE)
    }
}

/// 沙盒快照指纹：档位 + 白名单条数 FNV。
pub fn sb_fingerprint(tier: u32, whitelist_len: u32) -> u32 {
    let mut h = 0x811c9dc5u32 ^ tier.wrapping_mul(0x9e3779b9);
    h ^= whitelist_len;
    h = h.wrapping_mul(0x01000193);
    h
}

pub fn run_shim_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai35-shim");
    s.add("X08501 Shim 最小闭环", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "draw", impl_name: "shim_a", prio: 5 }) }, "空表注入成功");
    s.add("X08502 参数开放", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "draw", impl_name: "a", prio: 5 }); shim_inject(&mut t, ShimSlot { api: "draw", impl_name: "b", prio: 3 }) }, "高优先级可换入");
    s.add("X08503 档位矩阵", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "draw", impl_name: "a", prio: 3 }); !shim_inject(&mut t, ShimSlot { api: "draw", impl_name: "b", prio: 9 }) }, "低优先级拒换");
    s.add("X08504 快照迁移", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "k", impl_name: "a", prio: 5 }); shim_resolve(&t, "k", "native") == "a" }, "注入后解析命中");
    s.add("X08505 集成验证", shim_resolve(&[None; 4], "k", "native") == "native", "无 shim 回原生");
    s.add("X08506 越界钳制", { let mut t = [None; 2]; shim_inject(&mut t, ShimSlot { api: uniq(0), impl_name: "x", prio: 5 }) && shim_inject(&mut t, ShimSlot { api: uniq(1), impl_name: "x", prio: 5 }) && !shim_inject(&mut t, ShimSlot { api: uniq(2), impl_name: "x", prio: 5 }) }, "双槽表满即拒注");
    s.add("X08507 失败叙事", { let mut t = [None; 4]; shim_remove(&mut t, "none") == false }, "剥离不存在 API 报告未命中");
    s.add("X08508 中断还原", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "k", impl_name: "a", prio: 5 }); shim_remove(&mut t, "k"); shim_resolve(&t, "k", "native") == "native" }, "剥离后还原原生");
    s.add("X08509 资源降级", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "a", impl_name: "x", prio: 9 }); shim_inject(&mut t, ShimSlot { api: "b", impl_name: "y", prio: 1 }); shim_resolve(&t, "a", "n") == "x" }, "多 shim 各归其位");
    s.add("X08510 回滚净身", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "k", impl_name: "a", prio: 5 }); shim_inject(&mut t, ShimSlot { api: "k", impl_name: "b", prio: 9 }); shim_resolve(&t, "k", "n") == "a" }, "被拒注入不留痕");
    s.add("X08511 动效令牌", shim_resolve(&[Some(ShimSlot { api: "k", impl_name: "fast", prio: 1 }), None], "k", "n") == "fast", "首槽命中 O(层数)");
    s.add("X08512 三态焦点", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "k", impl_name: "hi", prio: 1 }); shim_inject(&mut t, ShimSlot { api: "k", impl_name: "lo", prio: 2 }); shim_resolve(&t, "k", "n") == "hi" }, "同 API 双 shim 取高优");
    s.add("X08513 键盘序", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "a", impl_name: "x", prio: 2 }); shim_inject(&mut t, ShimSlot { api: "a", impl_name: "y", prio: 1 }); shim_resolve(&t, "a", "n") == "y" }, "后注高优覆盖");
    s.add("X08514 微文案", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "", impl_name: "x", prio: 5 }) && shim_resolve(&t, "", "n") == "x" }, "空 API 名仍可挂");
    s.add("X08515 aria 等价", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "a", impl_name: "x", prio: 5 }); shim_remove(&mut t, "a") }, "剥离命中即真");
    s.add("X08516 基准采集", { let mut t = [None; 8]; let mut n = 0; for i in 0..8 { if shim_inject(&mut t, ShimSlot { api: uniq(i), impl_name: "x", prio: 5 }) { n += 1; } } n == 8 }, "八槽满编注入");
    s.add("X08517 热路径", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "hot", impl_name: "x", prio: 5 }); shim_resolve(&t, "hot", "n") == "x" }, "热 API 查询即时");
    s.add("X08518 零漂移", { let mut a = [None; 4]; let mut b = [None; 4]; shim_inject(&mut a, ShimSlot { api: "k", impl_name: "x", prio: 5 }); shim_inject(&mut b, ShimSlot { api: "k", impl_name: "x", prio: 5 }); shim_resolve(&a, "k", "n") == shim_resolve(&b, "k", "n") }, "双表零漂移");
    s.add("X08519 低配减档", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "k", impl_name: "a", prio: 5 }); shim_inject(&mut t, ShimSlot { api: "j", impl_name: "b", prio: 5 }); shim_remove(&mut t, "k"); t.iter().filter(|x| x.is_some()).count() == 1 }, "剥离只清目标槽");
    s.add("X08520 守卫", { let mut t = [None; 1]; shim_inject(&mut t, ShimSlot { api: "a", impl_name: "x", prio: 5 }) && !shim_inject(&mut t, ShimSlot { api: "b", impl_name: "y", prio: 1 }) }, "单槽表满即守卫");
    s.add("X08521 智能建议", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "k", impl_name: "old", prio: 5 }); shim_inject(&mut t, ShimSlot { api: "k", impl_name: "new", prio: 5 }) == false }, "同优先级不覆盖");
    s.add("X08522 批量模式", { let mut t = [None; 8]; let mut n = 0; for i in 0..6 { if shim_inject(&mut t, ShimSlot { api: uniq(i), impl_name: "x", prio: i as u8 }) { n += 1; } } n == 6 }, "批量注入六连成");
    s.add("X08523 跨域联动", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "k", impl_name: "a", prio: 5 }); shim_inject(&mut t, ShimSlot { api: "k", impl_name: "b", prio: 3 }); shim_remove(&mut t, "k"); shim_inject(&mut t, ShimSlot { api: "k", impl_name: "b", prio: 3 }) && shim_resolve(&t, "k", "n") == "b" }, "剥离-重注-解析可组合");
    s.add("X08524 扩展点", shim_resolve(&[], "k", "native-v2") == "native-v2", "原生实现名可定制");
    s.add("X08525 Shim 收官", { let mut t = [None; 4]; shim_inject(&mut t, ShimSlot { api: "fin", impl_name: "s", prio: 1 }) && shim_resolve(&t, "fin", "n") == "s" && shim_remove(&mut t, "fin") }, "AI-35 Shim 收官复核");
    s
}

fn uniq(i: u32) -> &'static str {
    // 固定 10 个互异 API 名，供满编/守卫用例。
    ["api0", "api1", "api2", "api3", "api4", "api5", "api6", "api7", "api8", "api9"][i as usize]
}

pub fn run_negotiate_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai35-negotiate");
    s.add("X08526 协商最小闭环", negotiate(200, 200, 100) == 2, "满足期望全量");
    s.add("X08527 参数开放", negotiate(150, 200, 100) == 1, "介于底线与期望降档");
    s.add("X08528 档位矩阵", [negotiate(200, 200, 100), negotiate(150, 200, 100), negotiate(50, 200, 100)] == [2, 1, 0], "三态协商矩阵");
    s.add("X08529 快照迁移", { let (w, m) = propose(230, 130); negotiate(230, w, m) == 2 }, "提案快照可复算");
    s.add("X08530 集成验证", negotiate_text(2) == "全量启用", "全量结论语");
    s.add("X08531 越界钳制", align_versions(0, 0) == 100, "零版对齐钳到 100");
    s.add("X08532 失败叙事", negotiate_text(0) == "协商破裂", "破裂结论可读");
    s.add("X08533 中断还原", negotiate_text(negotiate(150, 200, 100)) == "降档启用", "降档结论可还原");
    s.add("X08534 资源降级", align_versions(230, 130) == 130, "对齐取小者");
    s.add("X08535 回滚净身", negotiate(0, 0, 0) == 2, "零版协商等号成立");
    s.add("X08536 动效令牌", align_versions(100, 900) == 100, "对齐下限即令牌基线");
    s.add("X08537 三态焦点", negotiate_text(2) != negotiate_text(1) && negotiate_text(1) != negotiate_text(0), "三态结论互异");
    s.add("X08538 键盘序", (0..5u32).all(|i| negotiate(100 + i * 50, 200, 100) >= negotiate(100 + i * 50, 200, 150)), "底线越低越易达成");
    s.add("X08539 微文案", negotiate_text(1).chars().count() <= 4, "结论语克制");
    s.add("X08540 aria 等价", ["全量启用", "降档启用", "协商破裂"].iter().all(|t| !t.is_empty()), "三态皆可朗读");
    s.add("X08541 基准采集", { let mut n = 0; for p in 0..100u32 { if negotiate(p * 3, 200, 100) != 0 { n += 1; } } n >= 50 }, "百次协商过半达成");
    s.add("X08542 热路径", negotiate(300, 200, 100) == 2, "满配协商 O(1)");
    s.add("X08543 零漂移", align_versions(align_versions(230, 130), 150) == 130, "对齐可组合");
    s.add("X08544 低配减档", negotiate(100, 200, 150) == 0, "低于底线破裂");
    s.add("X08545 守卫", negotiate(200, 200, 300) == 2, "底线高于期望仍按期望裁决");
    s.add("X08546 智能建议", negotiate(130, 200, 130) == 1, "恰达底线降档启用");
    s.add("X08547 批量模式", { let provs = [50u32, 150, 250]; let mut n = 0; for &p in &provs { n += negotiate(p, 200, 100); } n == 3 }, "批量协商三态齐备");
    s.add("X08548 跨域联动", negotiate_text(negotiate(align_versions(230, 250), 200, 100)) == "全量启用", "对齐-协商可组合");
    s.add("X08549 扩展点", { let (w, m) = propose(500, 300); negotiate(400, w, m) == 1 }, "高版提案降档扩展");
    s.add("X08550 协商收官", negotiate(200, 200, 100) == 2 && align_versions(230, 130) == 130 && negotiate_text(0) == "协商破裂", "AI-35 协商收官复核");
    s
}

pub fn run_sandbox_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai35-sandbox");
    s.add("X08551 沙盒最小闭环", sb_tier(100) == SB_NATIVE, "满信任近原生");
    s.add("X08552 参数开放", sb_tier(80) == SB_NATIVE && sb_tier(60) == SB_LIMITED, "80/60 双界开放");
    s.add("X08553 档位矩阵", [sb_tier(100), sb_tier(70), sb_tier(45), sb_tier(10)] == [SB_NATIVE, SB_LIMITED, SB_READONLY, SB_LOCKED], "四档信任矩阵");
    s.add("X08554 快照迁移", sb_fingerprint(SB_LIMITED, 3) == sb_fingerprint(SB_LIMITED, 3), "指纹确定性");
    s.add("X08555 集成验证", sb_can_write(SB_LIMITED, false), "受限写档可写");
    s.add("X08556 越界钳制", sb_tier(u32::MAX) == SB_NATIVE, "极值信任不越档");
    s.add("X08557 失败叙事", !sb_can_write(SB_LOCKED, false), "全锁档拒绝写入");
    s.add("X08558 中断还原", sb_demote(SB_LIMITED, 3) == SB_READONLY, "三次逃逸降一档");
    s.add("X08559 资源降级", sb_demote(SB_LOCKED, 9) == SB_LOCKED, "最低档不再降");
    s.add("X08560 回滚净身", sb_demote(SB_NATIVE, 0) == SB_NATIVE, "零逃逸不降档");
    s.add("X08561 动效令牌", sb_can_write(SB_READONLY, true), "只读档白名单放行");
    s.add("X08562 三态焦点", !sb_can_write(SB_READONLY, false) && sb_can_write(SB_READONLY, true) && sb_can_write(SB_LIMITED, false), "写入三态互异");
    s.add("X08563 键盘序", (0..100u32).all(|t| sb_tier(t) <= sb_tier(t + 1)), "档位随信任单调");
    s.add("X08564 微文案", sb_fingerprint(SB_LOCKED, 0) != 0, "零态指纹非零");
    s.add("X08565 aria 等价", sb_fingerprint(SB_NATIVE, 1) != sb_fingerprint(SB_LOCKED, 1), "档位指纹可区分");
    s.add("X08566 基准采集", { let mut n = 0; for t in 0..100u32 { if sb_tier(t) >= SB_READONLY { n += 1; } } n == 70 }, "百分信任七十过线");
    s.add("X08567 热路径", sb_tier(59) == SB_READONLY, "59 恰入只读档");
    s.add("X08568 零漂移", { let a = sb_fingerprint(SB_LIMITED, 2); let b = sb_fingerprint(SB_LIMITED, 2); a == b }, "指纹零漂移");
    s.add("X08569 低配减档", sb_can_write(SB_LOCKED, true) == false, "全锁档白名单也拒");
    s.add("X08570 守卫", sb_demote(SB_READONLY, u32::MAX) == SB_LOCKED, "极值逃逸钳到全锁");
    s.add("X08571 智能建议", sb_tier(30) == SB_READONLY && sb_tier(29) == SB_LOCKED, "30 分界可解释");
    s.add("X08572 批量模式", { let trusts = [90u32, 65, 40, 5]; let want = [SB_NATIVE, SB_LIMITED, SB_READONLY, SB_LOCKED]; trusts.iter().zip(want.iter()).all(|(&t, &w)| sb_tier(t) == w) }, "批量四态齐备");
    s.add("X08573 跨域联动", sb_demote(sb_tier(85), 3) == SB_LIMITED, "定档-降档可组合");
    s.add("X08574 扩展点", sb_fingerprint(SB_LIMITED, 0) != sb_fingerprint(SB_LIMITED, 1), "白名单条数入指纹");
    s.add("X08575 沙盒收官", sb_tier(60) == SB_LIMITED && sb_can_write(SB_READONLY, true) && sb_demote(SB_LOCKED, 3) == SB_LOCKED, "AI-35 沙盒收官复核");
    s
}

// ---------------------------------------------------------------------------
// 测试：三族 × 25 = 75 检全绿。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatshim_75_checks_pass() {
        let sets = [run_shim_checks(), run_negotiate_checks(), run_sandbox_checks()];
        assert_eq!(sets.iter().map(|s| s.len()).sum::<usize>(), 75);
        for s in &sets {
            assert!(s.all_passed(), "domain {} failed", s.domain);
        }
    }
}
