//! unx2108_scenbook —— UNX-F2108 导出回归场景预告。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B26）：UNX-F2108 · 导出回归场景预告 · 目标 280 行 · 已深化
//! 判据原文：UNX-F2108-J1 正路径场景预告 8 场景三要素入册（→B30 主账扩容；
//!   累计 88/102），ktest 注册在册，skip 态零容忍带因
//! 组件：Scenario 预告场景三要素（场景 id / 判据锚 / 前置面——判据①三列）
//!      + ScenarioBook 预告册（8 场景封闭入册 + 场景名册内唯一 0xD402 + 要
//!      素闸 0xD401 + 累计账 88→96/102 口径核，越册 0xD405）+ ktest 注册表
//!      （场景 → ktest 名在册映射，未注册 0xD404）+ skip 零容忍账（skip 态
//!      必带 reason 非空，无因 0xD403）+ 场景越集闸（0xD406）+ LCG 压力（注
//!      册/skip 带因/无因注入轮转，零容忍账守恒）+ 指纹守恒对拍锚。
//! 对标：Linux kselftest 回归场景预告（TEST_GEN_PROGS 扩容 + skip 带因纪
//!   律）——本模块以「封闭场景册 + ktest 注册映射 + skip 因果账」复刻回归
//!   预告纪律，预告即承诺。
//! 性能逐项分解：入册 O(册长)；注册 O(1)（映射表直插）；skip 核 O(1)；
//!   累计账 O(1)；全整数算术零浮点零墙钟，同事件序列恒同账（确定性对拍
//!   锚，LCG 注入）。
//! 诊断码段：0xD4（0xD401~0xD406，全域 grep 确认无诊断基数占用——命中均为
//!   GB2312 汉字编码偶然值，先入库者占段）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，确定性（逻辑
//!   tick 注入，零墙钟），逐条定制数据结构/状态机/错误路径，零模板复述，
//!   零死代码。
use alloc::format;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与判码
// ---------------------------------------------------------------------------

/// 预告场景数（封闭八场景——判据①）。
pub const SCEN_N: usize = 8;
/// 累计基线（本批前）。
pub const ACC_BASE: u64 = 88;
/// 累计目标（全册 102）。
pub const ACC_TOTAL: u64 = 102;
/// 本批推进后累计（88 + 8 = 96）。
pub const ACC_AFTER: u64 = ACC_BASE + SCEN_N as u64;
/// ktest 名长度上限。
pub const KTEST_LEN: usize = 48;
/// skip 因果文本上限。
pub const REASON_CAP: usize = 120;

// 判码（段内独占 0xD401~0xD406）
/// 0xD401 场景缺要素（三要素不全）。
pub const C_SCEN_MISS: u16 = 0xD401;
/// 0xD402 场景重名（册内唯一破）。
pub const C_SCEN_DUP: u16 = 0xD402;
/// 0xD403 skip 无因（零容忍破）。
pub const C_SKIP_NOREASON: u16 = 0xD403;
/// 0xD404 ktest 未注册。
pub const C_KTEST_UNREG: u16 = 0xD404;
/// 0xD405 累计账越册（>102）。
pub const C_BOOK_OOB: u16 = 0xD405;
/// 0xD406 场景越集（id 落 8 场景闭集外）。
pub const C_SCEN_OOB: u16 = 0xD406;

/// 段守卫：判码落 0xD400..0xD500 开区间且严格递增互异。
pub const fn seg_guard() -> bool {
    C_SCEN_MISS >= 0xD400
        && C_SCEN_MISS < 0xD500
        && C_SCEN_DUP == C_SCEN_MISS + 1
        && C_SKIP_NOREASON == C_SCEN_MISS + 2
        && C_KTEST_UNREG == C_SCEN_MISS + 3
        && C_BOOK_OOB == C_SCEN_MISS + 4
        && C_SCEN_OOB == C_SCEN_MISS + 5
        && C_SCEN_OOB < 0xD500
}

/// 判码 → 文本（闭集穷举）。
pub fn code_text(code: u16) -> &'static str {
    match code {
        C_SCEN_MISS => "场景缺要素",
        C_SCEN_DUP => "场景重名",
        C_SKIP_NOREASON => "skip无因",
        C_KTEST_UNREG => "ktest未注册",
        C_BOOK_OOB => "累计账越册",
        C_SCEN_OOB => "场景越集",
        _ => "未知判码",
    }
}

// ---------------------------------------------------------------------------
// 场景与预告册
// ---------------------------------------------------------------------------

/// 预告场景三要素（判据①三列：场景 id / 判据锚 / 前置面）。
#[derive(Clone, Debug)]
pub struct Scenario {
    /// 场景 id（0..8 闭集）。
    pub id: u64,
    /// 判据锚（如 "UNX-F2108-J1-S3"）。
    pub anchor: String,
    /// 前置面（如 "B30 主账扩容"）。
    pub preface: String,
    /// 运行态：Run=正路径 / Skipped(带因)。
    pub skip_reason: Option<String>,
    /// ktest 注册名（空 = 未注册）。
    pub ktest: String,
}

impl Scenario {
    /// 三要素齐备核（判据①）。
    pub fn three_ok(&self) -> bool {
        self.id < SCEN_N as u64
            && !self.anchor.is_empty()
            && !self.preface.is_empty()
    }
    /// skip 零容忍核：Skipped 态必带非空因——判据③。
    pub fn skip_ok(&self) -> bool {
        match &self.skip_reason {
            None => true,
            Some(r) => !r.is_empty() && r.len() <= REASON_CAP,
        }
    }
    /// ktest 在册核。
    pub fn ktest_ok(&self) -> bool {
        !self.ktest.is_empty() && self.ktest.len() <= KTEST_LEN
    }
}

/// 预告册：8 场景封闭 + 累计账。
pub struct ScenarioBook {
    /// 场景账。
    pub scenes: Vec<Scenario>,
    /// 累计推进（基线 88 起）。
    pub acc: u64,
}

impl ScenarioBook {
    /// 空册（累计=基线）。
    pub fn new() -> ScenarioBook {
        ScenarioBook { scenes: Vec::new(), acc: ACC_BASE }
    }
    /// 入册：越集闸 → 三要素闸 → 重名闸 → ktest 闸 → skip 闸 → 累计闸。
    pub fn admit(&mut self, s: Scenario) -> Result<(), u16> {
        if s.id >= SCEN_N as u64 {
            return Err(C_SCEN_OOB);
        }
        if !s.three_ok() {
            return Err(C_SCEN_MISS);
        }
        if self.scenes.iter().any(|e| e.id == s.id) {
            return Err(C_SCEN_DUP);
        }
        if !s.ktest_ok() {
            return Err(C_KTEST_UNREG);
        }
        if !s.skip_ok() {
            return Err(C_SKIP_NOREASON);
        }
        if self.acc >= ACC_TOTAL {
            return Err(C_BOOK_OOB);
        }
        self.acc += 1;
        self.scenes.push(s);
        Ok(())
    }
    /// 累计账核：基线 + 册长 == 当前累计，且 ≤102——判据①累计面。
    pub const fn acc_ok(&self) -> bool {
        ACC_BASE + self.scenes.len() as u64 == self.acc && self.acc <= ACC_TOTAL
    }
    /// 全场景核：三要素 + skip 零容忍 + ktest 在册逐条过。
    pub fn all_ok(&self) -> bool {
        self.scenes.iter().all(|s| s.three_ok() && s.skip_ok() && s.ktest_ok())
    }
    /// 8 场景齐备核（判据①封闭面：册满 8 且 id 恰为 0..8 全集）。
    pub fn complete(&self) -> bool {
        if self.scenes.len() != SCEN_N {
            return false;
        }
        let mut seen = [false; SCEN_N];
        for s in &self.scenes {
            seen[s.id as usize] = true;
        }
        seen.iter().all(|&b| b)
    }
    /// ktest 注册表核：全场景注册名非空且互异。
    pub fn ktest_table_ok(&self) -> bool {
        for (i, s) in self.scenes.iter().enumerate() {
            if s.ktest.is_empty() {
                return false;
            }
            for other in self.scenes.iter().skip(i + 1) {
                if other.ktest == s.ktest {
                    return false;
                }
            }
        }
        true
    }
    /// 指纹：累计 + 全场景五元混合 FNV。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xD406_0000_0002_108F;
        h ^= self.acc;
        h = h.wrapping_mul(0x100_0000_01B3);
        for s in &self.scenes {
            h ^= s.id;
            h = h.wrapping_mul(0x100_0000_01B3);
            h ^= s.anchor.len() as u64;
            h = h.wrapping_mul(0x100_0000_01B3);
            h ^= s.ktest.len() as u64;
            h = h.wrapping_mul(0x100_0000_01B3);
            h ^= s.skip_reason.is_some() as u64;
            h = h.wrapping_mul(0x100_0000_01B3);
        }
        h
    }
}

/// 八场景封闭样板（正路径预告全集——判据①入册面）。
pub fn book_full() -> ScenarioBook {
    let mut b = ScenarioBook::new();
    let names = [
        "kt_exp_fmt_basic", "kt_exp_fmt_wrap", "kt_exp_checksum", "kt_exp_sign",
        "kt_exp_window", "kt_exp_agg", "kt_exp_tear", "kt_exp_reclaim",
    ];
    for i in 0..SCEN_N {
        let _ = b.admit(Scenario {
            id: i as u64,
            anchor: alloc::format!("UNX-F2108-J1-S{}", i + 1),
            preface: "B30 主账扩容".to_string(),
            skip_reason: None,
            ktest: names[i].to_string(),
        });
    }
    b
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
pub struct ScenStress {
    /// 入册成功笔数。
    pub admitted: u64,
    /// 拒绝笔数。
    pub rejected: u64,
    /// skip 带因笔数。
    pub skips_with_reason: u64,
    /// 无因拦截笔数。
    pub no_reason_blocked: u64,
    /// 总结论。
    pub ok: bool,
}

/// 场景册压力：n 轮 LCG（合法入册/重名/无因 skip/越集轮转）。
pub fn stress_scen(n: usize, seed: u64) -> ScenStress {
    let mut rng = Lcg::new(seed);
    let mut book = ScenarioBook::new();
    let mut admitted: u64 = 0;
    let mut rejected: u64 = 0;
    let mut skips_with_reason: u64 = 0;
    let mut no_reason_blocked: u64 = 0;
    for _i in 0..n {
        let r = rng.next();
        let id = (r % (SCEN_N as u64 + 2)) as u64; // 含越集 id
        let with_reason = r & 1 == 0;
        let s = Scenario {
            id,
            anchor: alloc::format!("UNX-F2108-J1-S{}", id + 1),
            preface: "B30".to_string(),
            // 带因轮转的三态：None = 运行态（合法，不走 skip 语义）；
            // Some(非空) = skip 态带因（合法）；Some(空) = skip 态无因
            // （**唯一触发 C_SKIP_NOREASON 的形状**）。零容忍判据核的是
            // "Skipped 态必带非空因"，违规形只有 Some("") 一种——把无因
            // 臂写成 None 是绕开了闸门而不是考验它，no_reason_blocked 会
            // 结构性恒 0，"压力全维"判据随之永假。
            skip_reason: if with_reason {
                Some("前置未就绪·带因".to_string())
            } else {
                Some(String::new())
            },
            ktest: alloc::format!("kt_s{}", id),
        };
        match book.admit(s) {
            Ok(()) => {
                admitted += 1;
                if book.scenes.last().map(|s| s.skip_reason.is_some()).unwrap_or(false) {
                    skips_with_reason += 1;
                }
            }
            Err(C_SKIP_NOREASON) => no_reason_blocked += 1,
            Err(_) => rejected += 1,
        }
    }
    let ok = book.acc_ok() && book.all_ok() && no_reason_blocked > 0;
    ScenStress { admitted, rejected, skips_with_reason, no_reason_blocked, ok }
}

/// 指纹：压力四柱混合 FNV。
pub fn fingerprint(s: &ScenStress) -> u64 {
    let mut h: u64 = 0xD400_0000_0002_1090;
    for v in [s.admitted, s.rejected, s.skips_with_reason, s.no_reason_blocked] {
        h ^= v;
        h = h.wrapping_mul(0x100_0000_01B3);
    }
    h
}

/// 无因 skip 探针（负向证据）：空 reason 必拒、None 必过——零容忍双证。
pub fn no_reason_probe() -> (bool, bool) {
    let mut b = ScenarioBook::new();
    // None（运行态）应过。
    let ok_none = b
        .admit(Scenario {
            id: 0,
            anchor: "A".to_string(),
            preface: "P".to_string(),
            skip_reason: None,
            ktest: "kt_a".to_string(),
        })
        .is_ok();
    // 空串 reason（skip 态）必拒。
    let blocked = b
        .admit(Scenario {
            id: 1,
            anchor: "B".to_string(),
            preface: "P".to_string(),
            skip_reason: Some(String::new()),
            ktest: "kt_b".to_string(),
        })
        == Err(C_SKIP_NOREASON);
    (ok_none, blocked)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 自检聚合（防自调：不由本模块任何函数调用）。
pub fn run_unx2108_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F2108");

    // J-1 段守卫 + 累计口径（88+8=96 ≤ 102）。
    cs.add(
        "0xD4段独占+累计口径",
        seg_guard() && ACC_BASE == 88 && ACC_AFTER == 96 && ACC_TOTAL == 102,
        "",
    );

    // J-2 八场景封闭入册（判据①）。
    let b = book_full();
    cs.add("八场景入册齐备", b.complete() && b.acc_ok() && b.acc == ACC_AFTER, "");

    // J-3 ktest 注册表：全注册且互异（判据②）。
    cs.add("ktest注册表", b.ktest_table_ok(), "");

    // J-4 skip 零容忍双证：None 过、空因拒（判据③）。
    let (ok_none, blocked) = no_reason_probe();
    cs.add("skip零容忍双证", ok_none && blocked, "");

    // J-5 重名闸：同 id 二次入册必拒。
    let mut b2 = ScenarioBook::new();
    let _ = b2.admit(Scenario {
        id: 0,
        anchor: "A".to_string(),
        preface: "P".to_string(),
        skip_reason: None,
        ktest: "kt_x".to_string(),
    });
    let dup = b2.admit(Scenario {
        id: 0,
        anchor: "A2".to_string(),
        preface: "P".to_string(),
        skip_reason: None,
        ktest: "kt_y".to_string(),
    });
    cs.add("场景重名必拒", dup == Err(C_SCEN_DUP), "");

    // J-6 越集闸：id=8 必拒。
    let oob = b2.admit(Scenario {
        id: 8,
        anchor: "Z".to_string(),
        preface: "P".to_string(),
        skip_reason: None,
        ktest: "kt_z".to_string(),
    });
    cs.add("场景越集必拒", oob == Err(C_SCEN_OOB), "");

    // J-7 缺要素闸：空锚必拒。
    let miss = b2.admit(Scenario {
        id: 1,
        anchor: String::new(),
        preface: "P".to_string(),
        skip_reason: None,
        ktest: "kt_m".to_string(),
    });
    cs.add("三要素闸", miss == Err(C_SCEN_MISS), "");

    // J-8 压力：384 轮全维核。
    let st = stress_scen(384, 0xD405_0001);
    cs.add("压力全维", st.ok && st.admitted > 0 && st.rejected > 0, "");

    // J-9 指纹重放对拍：同种子恒同指纹。
    let f1 = fingerprint(&stress_scen(128, 0xD405_0002));
    let f2 = fingerprint(&stress_scen(128, 0xD405_0002));
    let f3 = fingerprint(&stress_scen(128, 0xD405_0003));
    cs.add("指纹重放对拍", f1 == f2 && f1 != f3, "");

    // J-10 判码文本闭集：六码各得其所。
    let texts_ok = code_text(C_SCEN_MISS) != "未知判码"
        && code_text(C_SCEN_DUP) != "未知判码"
        && code_text(C_SKIP_NOREASON) != "未知判码"
        && code_text(C_KTEST_UNREG) != "未知判码"
        && code_text(C_BOOK_OOB) != "未知判码"
        && code_text(C_SCEN_OOB) != "未知判码"
        && code_text(0xD400) == "未知判码";
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
    fn full_book_complete() {
        let b = book_full();
        assert!(b.complete());
        assert!(b.acc_ok());
        assert_eq!(b.acc, 96);
        assert!(b.ktest_table_ok());
    }

    #[test]
    fn no_reason_zero_tolerance() {
        let (ok_none, blocked) = no_reason_probe();
        assert!(ok_none);
        assert!(blocked);
    }

    #[test]
    fn dup_and_oob() {
        let mut b = ScenarioBook::new();
        let s = |id: u64, k: &str| Scenario {
            id,
            anchor: "A".to_string(),
            preface: "P".to_string(),
            skip_reason: None,
            ktest: k.to_string(),
        };
        assert!(b.admit(s(0, "kt_a")).is_ok());
        assert_eq!(b.admit(s(0, "kt_b")), Err(C_SCEN_DUP));
        assert_eq!(b.admit(s(8, "kt_c")), Err(C_SCEN_OOB));
    }

    #[test]
    fn stress_replay_stable() {
        let a = stress_scen(128, 6);
        let c = stress_scen(128, 6);
        assert_eq!(a.admitted, c.admitted);
        assert_eq!(a.no_reason_blocked, c.no_reason_blocked);
        assert!(a.ok);
    }
}
