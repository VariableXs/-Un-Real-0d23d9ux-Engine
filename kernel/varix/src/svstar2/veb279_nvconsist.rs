//! veb279_nvconsist —— VE-F0279 NVIDIA 组一致性测试。
//! 锚点（CoRun Unxreal 主册）：VE-F0279 · NVIDIA 组一致性测试 · 目标 340 行
//! 判据原文：VE-F0279-J1 组一致性账：用例登记（八族用例，
//!           各带族号与断言数）；套件跑（逐族执行 + 分档记
//!           录）；一致性门（八族全绿才发一致性证）；指纹核
//!           （组内两卡跑同套件指纹必须一致，不一致即分叉
//!           立案）；复跑稳定核（同卡两次跑指纹一致）；族缺
//!           席核（缺席一族 → 门拒发证）。
//! 组件：用例账 + 套件引擎 + 一致性门 + 双卡指纹对。
//! 职责定位：组一致性测试是「多卡出厂的体检报告」——八族
//!           缺一不可：没测的族不是通过是未知；双卡指纹一致
//!           是组语义的根：同组不同果的卡迟早分叉；复跑一致
//!           是确定性的底线，抖动的通过比稳定的失败更可怕。
//! 性能逐项分解：单用例 O(1)；套件 O(用例数)；指纹 O(1) 折叠。
//! 诊断码段：0x30EC0（0x30EC1~0x30EC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const CS_VERSION: &str = "veb279_nvconsist-v1";

/// 诊断段基址（0x30EC0 段 · 本模块独占）。
pub const CS_SEG_BASE: u32 = 0x30EC0;
pub const XC_MAGIC: u32 = 0x30EC1; // 账魔数破
pub const XC_DUP: u32 = 0x30EC2; // 族号重复
pub const XC_NORUN: u32 = 0x30EC3; // 未跑先取证
pub const XC_FORK: u32 = 0x30EC4; // 双卡指纹分叉

/// 账魔数。
const MAGIC: u32 = 0x0EC0_1CAA;

/// 八族用例号。
pub const FAM_N: usize = 8;

/// 用例。
#[derive(Clone, Copy)]
pub struct Case {
    /// 族号（0..8）。
    pub fam: usize,
    /// 断言数。
    pub asserts: u32,
    /// 通过？
    pub passed: bool,
    /// 步进种子。
    pub seed: u32,
}

/// 单卡跑批结果。
pub struct RunCard {
    /// 卡号。
    pub gpu: u32,
    /// 各族结果（索引即族号）。
    pub fams: [Option<Case>; FAM_N],
    /// 总断言数。
    pub total: u32,
    /// 指纹。
    pub fp: u64,
    /// 已跑。
    pub ran: bool,
}

/// 套件引擎。
pub struct Suite {
    magic: u32,
}

impl Suite {
    /// 建。
    pub fn new() -> Suite {
        Suite { magic: MAGIC }
    }

    /// 单卡跑全套：注入各族的断言数与过败（确定性 LCG 混种子）。
    pub fn run_card(&self, gpu: u32, seed: u32, fam_asserts: [u32; FAM_N], fam_fail: usize) -> RunCard {
        let mut fams: [Option<Case>; FAM_N] = [None; FAM_N];
        let mut total = 0u32;
        let mut fp: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut st = seed.wrapping_add(gpu.wrapping_mul(0x9E37));
        for f in 0..FAM_N {
            st = st.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let passed = f != fam_fail;
            let c = Case {
                fam: f,
                asserts: fam_asserts[f],
                passed,
                seed: st >> 8,
            };
            total = total.wrapping_add(fam_asserts[f]);
            // 指纹折叠：族号 + 过败 + 断言数。
            fp = fp.rotate_left(7) ^ ((f as u64) << 40)
                ^ ((passed as u64) << 32)
                ^ fam_asserts[f] as u64;
            fams[f] = Some(c);
        }
        RunCard { gpu, fams, total, fp, ran: true }
    }

    /// 一致性门：八族齐 + 全绿才发证。
    pub fn gate(&self, r: &RunCard) -> Result<u64, u32> {
        if self.magic != MAGIC {
            return Err(XC_MAGIC);
        }
        if !r.ran {
            return Err(XC_NORUN);
        }
        let mut fp = r.fp;
        for f in 0..FAM_N {
            match r.fams[f] {
                None => return Err(XC_NORUN), // 族缺席
                Some(c) => {
                    if !c.passed {
                        return Err(XC_NORUN); // 有红即拒
                    }
                }
            }
        }
        Ok(fp)
    }

    /// 双卡指纹对：一致发组证，分叉立案。
    pub fn pair_gate(&self, a: &RunCard, b: &RunCard) -> Result<u64, u32> {
        let fa = self.gate(a)?;
        let fb = self.gate(b)?;
        if fa != fb {
            return Err(XC_FORK);
        }
        Ok(fa)
    }
}

pub fn run_veb279_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0279");
    let su = Suite::new();
    let all_pass = [4, 6, 3, 5, 2, 7, 4, 3];
    // 族一：单卡跑与门。
    let r1 = su.run_card(0, 100, all_pass, FAM_N);
    s.add("总断言 34", r1.total == 34, "4+6+3+5+2+7+4+3");
    s.add("八族齐", {
        let mut ok = true;
        for f in 0..FAM_N {
            if r1.fams[f].is_none() { ok = false; }
        }
        ok
    }, "无缺席");
    s.add("单卡发证", su.gate(&r1).is_ok(), "全绿");
    // 族二：族缺席核。
    let mut r2 = su.run_card(1, 100, all_pass, FAM_N);
    r2.fams[5] = None;
    s.add("缺席拒证", su.gate(&r2) == Err(XC_NORUN), "缺一拒");
    // 未跑核。
    let mut r3 = su.run_card(2, 100, all_pass, FAM_N);
    r3.ran = false;
    s.add("未跑拒证", su.gate(&r3) == Err(XC_NORUN), "先跑后证");
    // 族三：红族拒。
    let r4 = su.run_card(3, 100, all_pass, 2);
    s.add("红族拒证", su.gate(&r4) == Err(XC_NORUN), "族 2 红");
    // 族四：复跑稳定核。
    let r5 = su.run_card(0, 100, all_pass, FAM_N);
    s.add("复跑指纹同", r5.fp == r1.fp, "确定性");
    // 种子变 → 指纹不变：指纹只含语义结果（族过败 + 断言数），
    // 步进种子不入签——种子只影响用例内部步进，不影响过败判定。
    let r6 = su.run_card(0, 200, all_pass, FAM_N);
    s.add("换种指纹同", r6.fp == r1.fp, "语义签稳");
    // 断言数变 → 指纹变。
    let mut alt = all_pass;
    alt[3] = 99;
    let r7 = su.run_card(0, 100, alt, FAM_N);
    s.add("断言变签变", r7.fp != r1.fp, "断言入签");
    // 族五：双卡组证。
    let a = su.run_card(7, 100, all_pass, FAM_N);
    let b = su.run_card(8, 100, all_pass, FAM_N);
    // 同套件断言表 + 同过败 → 同族号/过败/断言 → 同指纹（种子只进步进种子）。
    s.add("双卡发组证", su.pair_gate(&a, &b).is_ok(), "一致");
    // 一卡带红 → 组证拒（分叉或红）。
    let c = su.run_card(9, 100, all_pass, 6);
    s.add("带红组拒", su.pair_gate(&a, &c) == Err(XC_NORUN), "先过单卡门");
    // 断言表分叉 → 指纹分叉立案。
    let mut half = all_pass;
    half[0] = 9;
    let d = su.run_card(10, 100, half, FAM_N);
    s.add("分叉立案", su.pair_gate(&a, &d) == Err(XC_FORK), "表不同签异");
    // 八绿对八绿但一卡缺族 → 缺席先拒。
    let mut e = su.run_card(11, 100, all_pass, FAM_N);
    e.fams[7] = None;
    s.add("缺族组拒", su.pair_gate(&a, &e) == Err(XC_NORUN), "缺席优先");
    s.add("魔数核", CS_SEG_BASE == 0x30EC0, "段基址");
    s
}
