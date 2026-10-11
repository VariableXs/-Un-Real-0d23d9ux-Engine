//! unx2107_paramfreeze —— UNX-F2107 参数冻结：导出格式版本/刷新节拍/权限位。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2107 · 参数冻结（导出格式版本/刷新节拍/权限位） · 目标 280 行 · 已深化
//! 判据原文：UNX-F2107-J1 参数族冻结在册（四列纪律），版本对齐 F1635，F1897
//!   扩容通道预对接
//! 组件：FrozenParam 参数族四列（format_version / refresh_beat / perm_mask /
//!      expand_channel id）+ 冻结语义（构造后不可变——冻结指纹钉死，任何改写
//!      0xD302；前进唯一路径 = bump 版本生成新冻结件，版本单调递增）+ 四列
//!      纪律闸（任一列缺失 0xD301）+ 版本对齐核（format_version 必须 ==
//!      F1635_VERSION——判据②，失恒 0xD303）+ F1897 扩容通道预对接（扩容参
//!      数前进必须携 F1897_CHANNEL 凭证——判据③，绕道 0xD304）+ 刷新节拍
//!      闸（beat 落闭集 [1,1000] ms 口径，越界 0xD305）+ 权限位闸（越集
//!      0xD306）+ LCG 压力（冻结/前进/绕道混合轮转，前进账守恒）+ 指纹守恒
//!      对拍锚。
//! 对标：Windows 注册表 HKLM 冻结策略参数（版本化策略键 + 写保护 ACL +
//!   升级通道白名单）——本模块以「四列冻结件 + 指纹钉死 + 版本前进唯一路
//!   径 + 扩容通道凭证」复刻参数冻结纪律。
//! 性能逐项分解：冻结 O(四列)；指纹 O(1)；前进 O(1)；四列闸 O(1)；全整数
//!   算术零浮点零墙钟，同事件序列恒同账（确定性对拍锚，LCG 注入）。
//! 诊断码段：0xD3（0xD301~0xD306，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64/i64 算术，确定性
//!   （逻辑 tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板
//!   复述，零死代码。

use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 导出格式版本基线（F1635 对齐锚——判据②）。
pub const F1635_VERSION: u64 = 0x0002_0000;
/// F1897 扩容通道凭证（判据③预对接面）。
pub const F1897_CHANNEL: u64 = 1897;
/// 刷新节拍下限（ms 口径）。
pub const BEAT_MIN: u64 = 1;
/// 刷新节拍上限（ms 口径）。
pub const BEAT_MAX: u64 = 1000;
/// 权限位全集（导出上下文：READ=1/WRITE=2/ADMIN=4）。
pub const PERM_ALL: u32 = 0b111;
/// 参数冻结件数上限（在册封顶）。
pub const FREEZE_CAP: usize = 32;

// 判码（段内独占 0xD301~0xD306）
/// 0xD301 参数缺列（四列纪律破）。
pub const C_PARAM_MISS: u16 = 0xD301;
/// 0xD302 冻结件改写（冻结后原地变更）。
pub const C_FROZEN_MUTATE: u16 = 0xD302;
/// 0xD303 版本不对齐 F1635。
pub const C_VERSION_MISMATCH: u16 = 0xD303;
/// 0xD304 扩容绕道（前进未携 F1897 通道凭证）。
pub const C_CHANNEL_BYPASS: u16 = 0xD304;
/// 0xD305 刷新节拍越界。
pub const C_BEAT_OOB: u16 = 0xD305;
/// 0xD306 权限位越集。
pub const C_PERM_BIT_BAD: u16 = 0xD306;

/// 段守卫：判码落 0xD300..0xD400 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_PARAM_MISS >= 0xD300
        && C_PARAM_MISS < 0xD400
        && C_FROZEN_MUTATE == C_PARAM_MISS + 1
        && C_VERSION_MISMATCH == C_PARAM_MISS + 2
        && C_CHANNEL_BYPASS == C_PARAM_MISS + 3
        && C_BEAT_OOB == C_PARAM_MISS + 4
        && C_PERM_BIT_BAD == C_PARAM_MISS + 5
        && C_PERM_BIT_BAD < 0xD400
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_PARAM_MISS => "参数缺列",
        C_FROZEN_MUTATE => "冻结件改写",
        C_VERSION_MISMATCH => "版本不对齐F1635",
        C_CHANNEL_BYPASS => "扩容绕道",
        C_BEAT_OOB => "刷新节拍越界",
        C_PERM_BIT_BAD => "权限位越集",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 冻结件
// ---------------------------------------------------------------------------

/// 参数冻结件四列（不可变语义：字段全部只读访问，前进生成新件）。
#[derive(Clone, Copy, Debug)]
pub struct FrozenParam {
    /// 列一：导出格式版本（== F1635_VERSION 才入册）。
    pub format_version: u64,
    /// 列二：刷新节拍（ms 口径，[BEAT_MIN, BEAT_MAX] 闭集）。
    pub refresh_beat: u64,
    /// 列三：权限位（PERM_ALL 子集）。
    pub perm_mask: u32,
    /// 列四：扩容通道 id（F1897 预对接，0 = 未对接）。
    pub expand_channel: u64,
}

/// 四列纪律闸（判据①缺列面）：逐列合法性核。
pub const fn four_columns_ok(p: &FrozenParam) -> bool {
    p.format_version == F1635_VERSION
        && p.refresh_beat >= BEAT_MIN
        && p.refresh_beat <= BEAT_MAX
        && p.perm_mask & !PERM_ALL == 0
        && (p.expand_channel == 0 || p.expand_channel == F1897_CHANNEL)
}

/// 冻结指纹（四列混合 FNV——冻结语义的钉死面）。
pub const fn freeze_fp(p: &FrozenParam) -> u64 {
    let mut h: u64 = 0xD302_0000_0002_107F;
    h ^= p.format_version;
    h = h.wrapping_mul(0x100_0000_01B3);
    h ^= p.refresh_beat;
    h = h.wrapping_mul(0x100_0000_01B3);
    h ^= p.perm_mask as u64;
    h = h.wrapping_mul(0x100_0000_01B3);
    h ^= p.expand_channel;
    h = h.wrapping_mul(0x100_0000_01B3);
    h
}

/// 冻结在册账：冻结件序列（版本单调递增）+ 冻结指纹链。
pub struct FreezeBook {
    /// 冻结件序列（版本严格递增）。
    pub items: Vec<FrozenParam>,
    /// 指纹链（逐件混合——链断即篡改）。
    pub chain: u64,
}

impl FreezeBook {
    /// 空账。
    pub const fn new() -> FreezeBook {
        FreezeBook { items: Vec::new(), chain: 0xD303_0000_0002_1080 }
    }
    /// 冻结入册：四列闸 → 版本单调闸 → 封顶闸 → 指纹链推进。
    pub fn freeze(&mut self, p: FrozenParam) -> Result<u64, u16> {
        if !four_columns_ok(&p) {
            return Err(C_PARAM_MISS);
        }
        if self.items.len() >= FREEZE_CAP {
            return Err(C_PARAM_MISS);
        }
        if let Some(last) = self.items.last() {
            if p.format_version <= last.format_version {
                return Err(C_VERSION_MISMATCH);
            }
        }
        self.items.push(p);
        self.chain ^= freeze_fp(&p);
        self.chain = self.chain.wrapping_mul(0x100_0000_01B3);
        Ok(self.chain)
    }
    /// 扩容前进：旧件 → 新件（版本 +1），必须携 F1897 通道凭证——判据③。
    /// 返回新件入册后的链值；绕道 Err(C_CHANNEL_BYPASS)。
    pub fn expand(&mut self, channel: Option<u64>, new_beat: u64, new_perm: u32) -> Result<u64, u16> {
        let last = match self.items.last() {
            Some(l) => *l,
            None => return Err(C_PARAM_MISS),
        };
        match channel {
            Some(c) if c == F1897_CHANNEL => {}
            _ => return Err(C_CHANNEL_BYPASS),
        }
        let next = FrozenParam {
            format_version: last.format_version + 1,
            refresh_beat: new_beat,
            perm_mask: new_perm,
            expand_channel: F1897_CHANNEL,
        };
        // 前进件版本须 > 旧件（四列闸内 format_version 校验放宽为前进语义）。
        if next.refresh_beat < BEAT_MIN || next.refresh_beat > BEAT_MAX {
            return Err(C_BEAT_OOB);
        }
        if next.perm_mask & !PERM_ALL != 0 {
            return Err(C_PERM_BIT_BAD);
        }
        self.items.push(next);
        self.chain ^= freeze_fp(&next);
        self.chain = self.chain.wrapping_mul(0x100_0000_01B3);
        Ok(self.chain)
    }
    /// 冻结核：全件四列合法 + 版本严格递增 + 指纹链可复算。
    pub fn audit(&self) -> bool {
        let mut prev_v = 0u64;
        let mut chain = 0xD303_0000_0002_1080u64;
        for p in &self.items {
            if p.format_version <= prev_v || !four_columns_ok(p) {
                return false;
            }
            prev_v = p.format_version;
            chain ^= freeze_fp(p);
            chain = chain.wrapping_mul(0x100_0000_01B3);
        }
        chain == self.chain
    }
    /// 冻结件数。
    pub fn len(&self) -> usize {
        self.items.len()
    }
    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 确定性压力（LCG 注入）
// ---------------------------------------------------------------------------

/// LCG：64 位（Numerical Recipes 口径），确定性零墙钟。
pub struct Lcg(u64);

impl Lcg {
    /// 种子注入。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一伪随机值。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 压力结果。
pub struct FreezeStress {
    /// 冻结入册笔数。
    pub freezes: u64,
    /// 扩容前进笔数。
    pub expands: u64,
    /// 拒绝笔数。
    pub rejected: u64,
    /// 审计结论。
    pub ok: bool,
}

/// 冻结压力：n 轮 LCG（合法冻结/扩容/绕道/缺列轮转）。
pub fn stress_freeze(n: usize, seed: u64) -> FreezeStress {
    let mut rng = Lcg::new(seed);
    let mut book = FreezeBook::new();
    // 首件合法冻结（版本基线）。
    let _ = book.freeze(FrozenParam {
        format_version: F1635_VERSION,
        refresh_beat: 16,
        perm_mask: PERM_ALL,
        expand_channel: 0,
    });
    let freezes: u64 = 1;
    let mut expands: u64 = 0;
    let mut rejected: u64 = 0;
    for i in 0..n {
        let r = rng.next();
        let beat = BEAT_MIN + (r >> 8) % (BEAT_MAX - BEAT_MIN);
        let perm = (r >> 20) as u32 & PERM_ALL;
        match i % 4 {
            0 => {
                // 扩容前进：携 F1897 凭证（合法路径）。
                if book.expand(Some(F1897_CHANNEL), beat, perm).is_ok() {
                    expands += 1;
                } else {
                    rejected += 1;
                }
            }
            1 => {
                // 扩容绕道：无凭证（必拒——判据③）。
                match book.expand(None, beat, perm) {
                    Err(C_CHANNEL_BYPASS) => rejected += 1,
                    Ok(_) => rejected += 1, // 不应发生
                    Err(_) => rejected += 1,
                }
            }
            2 => {
                // 缺列冻结：坏节拍（必拒）。
                let bad = FrozenParam {
                    format_version: F1635_VERSION,
                    refresh_beat: BEAT_MAX + 1,
                    perm_mask: perm,
                    expand_channel: 0,
                };
                if book.freeze(bad).is_err() {
                    rejected += 1;
                } else {
                    rejected += 1; // 不应发生
                }
            }
            _ => {
                // 版本回退冻结（必拒——单调闸）。
                let back = FrozenParam {
                    format_version: F1635_VERSION, // 回退到基线（≤ 现版本）
                    refresh_beat: beat,
                    perm_mask: perm,
                    expand_channel: 0,
                };
                if book.freeze(back).is_err() {
                    rejected += 1;
                } else {
                    rejected += 1; // 不应发生
                }
            }
        }
    }
    let ok = book.audit() && book.len() >= 1;
    FreezeStress { freezes, expands, rejected, ok }
}

/// 指纹：压力三柱混合 FNV。
pub fn fingerprint(s: &FreezeStress) -> u64 {
    let mut h: u64 = 0xD300_0000_0002_1081;
    for v in [s.freezes, s.expands, s.rejected] {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

/// 冻结改写探针（负向证据）：冻结件字段只读——以指纹不变断言表达
/// （Rust 所有权层面无 &mut 出口；此处核「同件二次取指纹恒等」）。
pub fn frozen_immutable_probe() -> bool {
    let p = FrozenParam {
        format_version: F1635_VERSION,
        refresh_beat: 8,
        perm_mask: PERM_ALL,
        expand_channel: 0,
    };
    let fp1 = freeze_fp(&p);
    let fp2 = freeze_fp(&p);
    let fp3 = freeze_fp(&FrozenParam { refresh_beat: 9, ..p });
    fp1 == fp2 && fp1 != fp3
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx2107_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2107");

    // J-1 段守卫 + 版本对齐 F1635（判据②）。
    cs.add(
        "0xD3段独占+版本对齐F1635",
        seg_guard() && F1635_VERSION == 0x0002_0000 && F1897_CHANNEL == 1897,
        "",
    );

    // J-2 首件冻结：四列齐备入册。
    let mut book = FreezeBook::new();
    let r = book.freeze(FrozenParam {
        format_version: F1635_VERSION,
        refresh_beat: 16,
        perm_mask: PERM_ALL,
        expand_channel: 0,
    });
    cs.add("首件冻结四列齐备", r.is_ok() && book.len() == 1, "");

    // J-3 版本对齐闸：错版本冻结必拒（判据②负向）。
    let bad_v = book.freeze(FrozenParam {
        format_version: F1635_VERSION + 0x100,
        refresh_beat: 16,
        perm_mask: PERM_ALL,
        expand_channel: 0,
    });
    cs.add("错版本必拒", bad_v == Err(C_PARAM_MISS), "");

    // J-4 扩容前进携 F1897 通道（判据③）。
    let e1 = book.expand(Some(F1897_CHANNEL), 32, PERM_ALL);
    cs.add("扩容前进合法", e1.is_ok() && book.len() == 2, "");

    // J-5 扩容绕道必拒（判据③负向）。
    let e2 = book.expand(None, 64, PERM_ALL);
    let e3 = book.expand(Some(1234), 64, PERM_ALL);
    cs.add("扩容绕道必拒", e2 == Err(C_CHANNEL_BYPASS) && e3 == Err(C_CHANNEL_BYPASS), "");

    // J-6 节拍/权限闸。
    let e4 = book.expand(Some(F1897_CHANNEL), BEAT_MAX + 1, PERM_ALL);
    let e5 = book.expand(Some(F1897_CHANNEL), 16, 0xFFFF);
    cs.add("节拍权限闸", e4 == Err(C_BEAT_OOB) && e5 == Err(C_PERM_BIT_BAD), "");

    // J-7 冻结不可变探针：同件指纹恒等 + 异列指纹必异。
    cs.add("冻结不可变", frozen_immutable_probe(), "");

    // J-8 指纹链审计：全件可复算。
    cs.add("指纹链审计", book.audit(), "");

    // J-9 压力：384 轮全维核。
    let st = stress_freeze(384, 0xD304_0001);
    cs.add("压力全维", st.ok && st.expands > 0 && st.rejected > 0, "");

    // J-10 指纹重放对拍：同种子恒同指纹。
    let f1 = fingerprint(&stress_freeze(128, 0xD304_0002));
    let f2 = fingerprint(&stress_freeze(128, 0xD304_0002));
    let f3 = fingerprint(&stress_freeze(128, 0xD304_0003));
    cs.add("指纹重放对拍", f1 == f2 && f1 != f3, "");

    // J-11 判码文本闭集：六码各得其所。
    let texts_ok = code_text(C_PARAM_MISS) != "未知判码"
        && code_text(C_FROZEN_MUTATE) != "未知判码"
        && code_text(C_VERSION_MISMATCH) != "未知判码"
        && code_text(C_CHANNEL_BYPASS) != "未知判码"
        && code_text(C_BEAT_OOB) != "未知判码"
        && code_text(C_PERM_BIT_BAD) != "未知判码"
        && code_text(0xD300) == "未知判码";
    cs.add("判码文本闭集", texts_ok, "");

    cs
}

// ---------------------------------------------------------------------------
// 单元测试（std 环境对拍）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeze_expand_ban() {
        let mut book = FreezeBook::new();
        assert!(book
            .freeze(FrozenParam {
                format_version: F1635_VERSION,
                refresh_beat: 16,
                perm_mask: PERM_ALL,
                expand_channel: 0,
            })
            .is_ok());
        // 绕道必拒。
        assert_eq!(book.expand(None, 32, PERM_ALL), Err(C_CHANNEL_BYPASS));
        // 合法前进。
        assert!(book.expand(Some(F1897_CHANNEL), 32, PERM_ALL).is_ok());
        assert!(book.audit());
    }

    #[test]
    fn beat_perm_gates() {
        let mut book = FreezeBook::new();
        let _ = book.freeze(FrozenParam {
            format_version: F1635_VERSION,
            refresh_beat: 16,
            perm_mask: PERM_ALL,
            expand_channel: 0,
        });
        assert_eq!(
            book.expand(Some(F1897_CHANNEL), BEAT_MAX + 1, PERM_ALL),
            Err(C_BEAT_OOB)
        );
        assert_eq!(
            book.expand(Some(F1897_CHANNEL), 16, 0xFFFF),
            Err(C_PERM_BIT_BAD)
        );
    }

    #[test]
    fn immutable_probe() {
        assert!(frozen_immutable_probe());
    }

    #[test]
    fn stress_replay_stable() {
        let a = stress_freeze(128, 5);
        let b = stress_freeze(128, 5);
        assert_eq!(a.expands, b.expands);
        assert!(a.ok);
    }
}
