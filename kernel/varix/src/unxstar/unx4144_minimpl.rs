//! unx4144_minimpl — UNX-F4144 最小实现集裁断（每 fs 必须实现的最小函数清单）（诊断段 0x93C0）。
//!
//! 判据原文：UNX-F4144-J1 最小实现集清单（挂载可用的 8 函数）裁断器对 1,000 组随机
//!           半实现表全部正确裁断（合格/不合格与人工判定一致），裁断依据可解释。
//!   J2 穷举 256 掩码合格数恰为 8。
//!   J3 裁断与人工判定 1,000 组零分歧。
//!   J4 不合格结论给出首个缺失下标与缺失数。
//! 设计纪律：
//!   1. 8 函数分为「必需 5」与「可选 3」：裁断结论只看必需位是否全置，可缺省位一律
//!      不影响结论；这是「最小集」的定义，不是「建议集」。
//!   2. 裁断必须可解释：不合格时给出缺的第一个必需函数下标与缺失总数，而不是只回一个
//!      布尔。可解释性由返回结构承载，不靠日志。
//!   3. 一致性用「双路判定」验证：裁断器路径（逐位与掩码比）与人工路径（逐项计数后
//!      比 8）各算一遍，两路结论必须相同；不同即判裁断器自身不可信。
//!   4. 随机源为确定性 LCG（整数乘加），不使用系统熵，保证同一构建两次跑同分布。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x93C0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x93C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MISSING: u16 = SEG_BASE + 1; // 必需函数缺失（不合格）
const E_BITMAP: u16 = SEG_BASE + 2; // 掩码出现闭集外位
const E_DISAGREE: u16 = SEG_BASE + 3; // 双路判定不一致（裁断器不可信）
const E_ACCT: u16 = SEG_BASE + 4; // 裁断账不平
const E_COUNT: u16 = SEG_BASE + 5; // 函数数不是 8
const E_STATE: u16 = SEG_BASE + 6; // 相位机非法迁移
const E_SAMPLE: u16 = SEG_BASE + 7; // 采样数与声明不符
const E_DUP: u16 = SEG_BASE + 8; // 同一掩码重复登记

/// 最小集函数数（判据原文 8）。
pub const FN_COUNT: usize = 8;
/// 必需函数数（缺一即不合格）。
pub const MAND_COUNT: u32 = 5;
/// 采样轮数（判据原文 1,000）。
pub const SAMPLES: u32 = 1_000;
/// 全位掩码。
pub const ALL_MASK: u32 = (1u32 << FN_COUNT) - 1;

/// 裁断相位机：Idle → Judging → Done；不合格落 Rejected。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdjState {
    Idle,
    Judging,
    Done,
    Rejected,
}

/// 函数需求条目：名 + 是否必需 + 依据（可解释性的静态部分）。
#[derive(Clone, Copy)]
pub struct FnReq {
    /// 函数名
    pub name: &'static str,
    /// 是否必需
    pub mandatory: bool,
    /// 为何必需/可缺省（一句话依据）
    pub why: &'static str,
}

/// 最小集清单（8 项，来源：挂载即需的最小语义闭环，含分配/回收/解析/统计四类）。
pub const MIN_SET: [FnReq; FN_COUNT] = [
    // 分配类
    FnReq { name: "alloc_inode", mandatory: true, why: "任何对象都需内存，缺则无法建索引节点" },
    FnReq { name: "destroy_inode", mandatory: true, why: "缺则回收链断，必然泄漏" },
    // 生命周期类
    FnReq { name: "get_tree", mandatory: true, why: "挂载入口，缺则 sb 建不出来" },
    FnReq { name: "kill_sb", mandatory: true, why: "卸载入口，缺则 sb 永久驻留" },
    // 解析类
    FnReq { name: "lookup", mandatory: true, why: "名字到 inode 的唯一通路，缺则路径不可解" },
    // 统计类
    FnReq { name: "statfs", mandatory: false, why: "可缺省为常量族，仅影响 df 精度" },
    // 维护类
    FnReq { name: "sync_fs", mandatory: false, why: "可缺省为空实现，只影响持久性承诺" },
    FnReq { name: "show_options", mandatory: false, why: "可缺省，仅影响 /proc/mounts 显示" },
];

/// 必需位掩码（5 位）。
pub const MAND_MASK: u32 =
    (1u32 << 0) | (1u32 << 1) | (1u32 << 2) | (1u32 << 3) | (1u32 << 4);

/// 裁断结论：合格与否 + 缺失必需数 + 首个缺失下标 + 结论码。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    /// 是否合格
    pub pass: bool,
    /// 缺失的必需函数数
    pub missing: u32,
    /// 首个缺失必需函数下标（合格时为 FN_COUNT）
    pub first_missing: usize,
    /// 结论码（0 表合格）
    pub code: u16,
}

impl Verdict {
    /// 可解释性：一句话依据（静态串，按结论选）。
    pub fn explain(&self) -> &'static str {
        if self.pass {
            return "必需 5 函数齐备，可挂载";
        }
        match self.first_missing {
            0 => "缺 alloc_inode：无法建索引节点",
            1 => "缺 destroy_inode：回收链断裂",
            2 => "缺 get_tree：sb 建不出来",
            3 => "缺 kill_sb：sb 无法卸载",
            4 => "缺 lookup：路径不可解析",
            _ => "缺若干必需函数",
        }
    }
}

/// 裁断器：掩码 → 结论；同时维护双路一致性与采样账。
pub struct MinImplJudge {
    /// 相位
    pub state: AdjState,
    /// 合格次数
    pub accepts: u32,
    /// 不合格次数
    pub rejects: u32,
    /// 双路判定不一致次数（应为 0）
    pub disagreements: u32,
    /// 已裁掩码登记（防重复，采样去重统计用）
    pub seen: Vec<u32>,
    /// 相位推进次数
    pub transitions: u32,
}

impl MinImplJudge {
    pub fn new() -> MinImplJudge {
        MinImplJudge {
            state: AdjState::Idle,
            accepts: 0,
            rejects: 0,
            disagreements: 0,
            seen: Vec::new(),
            transitions: 0,
        }
    }

    /// 单路裁断：逐位与必需掩码比。
    fn judge_mask(&self, mask: u32) -> (bool, u32, usize) {
        let mut missing = 0u32;
        let mut first = FN_COUNT;
        let mut i = 0usize;
        while i < FN_COUNT {
            if !MIN_SET[i].mandatory {
                i += 1;
                continue;
            }
            if (mask & (1u32 << (i as u32))) == 0 {
                missing += 1;
                if first == FN_COUNT {
                    first = i;
                }
            }
            i += 1;
        }
        (missing == 0, missing, first)
    }

    /// 人工路径（独立实现）：数已置的必需位，与 5 比较。
    fn manual_judge(&self, mask: u32) -> bool {
        let mut cnt = 0u32;
        let mut i = 0usize;
        while i < FN_COUNT {
            if MIN_SET[i].mandatory && (mask & (1u32 << (i as u32))) != 0 {
                cnt += 1;
            }
            i += 1;
        }
        cnt == MAND_COUNT
    }

    /// 裁断入口：闭集校验 → 双路判定 → 入账。
    pub fn judge(&mut self, mask: u32) -> Result<Verdict, u16> {
        if (mask & !ALL_MASK) != 0 {
            return Err(E_BITMAP);
        }
        if self.state == AdjState::Idle {
            self.state = AdjState::Judging;
            self.transitions += 1;
        }
        let (pass, missing, first) = self.judge_mask(mask);
        let manual = self.manual_judge(mask);
        if pass != manual {
            self.disagreements += 1;
            return Err(E_DISAGREE);
        }
        if pass {
            self.accepts += 1;
            self.state = AdjState::Done;
            Ok(Verdict {
                pass: true,
                missing: 0,
                first_missing: FN_COUNT,
                code: 0,
            })
        } else {
            self.rejects += 1;
            self.state = AdjState::Rejected;
            Ok(Verdict {
                pass: false,
                missing,
                first_missing: first,
                code: E_MISSING,
            })
        }
    }

    /// 采样去重登记：返回 true 表示首次见到该掩码。
    pub fn note_mask(&mut self, mask: u32) -> bool {
        let mut i = 0usize;
        while i < self.seen.len() {
            if self.seen[i] == mask {
                return false;
            }
            i += 1;
        }
        self.seen.push(mask);
        true
    }

    /// 裁断账守恒：accepts + rejects 由外部分别对账（本结构不自证）。
    pub fn accounted(&self) -> u32 {
        self.accepts + self.rejects
    }
}

/// 确定性 LCG（Numerical Recipes 常量）：x = 1664525x + 1013904223 (mod 2^32)。
pub struct Lcg {
    /// 当前状态
    pub x: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { x: seed }
    }

    pub fn next(&mut self) -> u32 {
        self.x = self.x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.x
    }

    /// 取低 8 位作为 8 函数掩码（闭集内，不产生越界位）。
    pub fn next_mask(&mut self) -> u32 {
        self.next() & ALL_MASK
    }
}

/// 采样报告：1,000 组的合格率与一致性统计。
pub struct SampleReport {
    /// 采样组数
    pub samples: u32,
    /// 其中合格组数
    pub accepts: u32,
    /// 其中不合格组数
    pub rejects: u32,
    /// 去重后的掩码数
    pub distinct: u32,
    /// 双路不一致数（须为 0）
    pub disagreements: u32,
}

impl SampleReport {
    /// 报告自洽：samples == accepts + rejects。
    pub fn consistent(&self) -> bool {
        self.samples == self.accepts + self.rejects && self.disagreements == 0
    }

    /// 合格率（千分比）。
    pub fn pass_permille(&self) -> u32 {
        if self.samples == 0 {
            return 0;
        }
        (self.accepts as u64 * 1000 / self.samples as u64) as u32
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MISSING => "必需函数缺失（不合格）",
        E_BITMAP => "掩码出现闭集外位",
        E_DISAGREE => "双路判定不一致（裁断器不可信）",
        E_ACCT => "裁断账不平",
        E_COUNT => "函数数不是 8",
        E_STATE => "相位机非法迁移",
        E_SAMPLE => "采样数与声明不符",
        E_DUP => "同一掩码重复登记",
        _ => "未知诊断码",
    }
}

/// 确定性演练：全掩码 → 空掩码 → 穷举 256 → 1,000 采样 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：清单自检——8 项、必需 5 项、必需掩码与清单一致。
    if MIN_SET.len() != FN_COUNT {
        return Err(E_COUNT);
    }
    let mut mand = 0u32;
    let mut i = 0usize;
    while i < FN_COUNT {
        if MIN_SET[i].mandatory {
            mand += 1;
        }
        i += 1;
    }
    if mand != MAND_COUNT || MAND_MASK != 0x1F {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2：全掩码合格；结论码 0、解释非空。
    let mut j = MinImplJudge::new();
    let v = j.judge(ALL_MASK)?;
    if !v.pass || v.code != 0 || v.missing != 0 {
        return Err(E_ACCT);
    }
    if v.explain() == "" {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：空掩码不合格——缺 5 个，首个缺失为 0（alloc_inode）。
    let v0 = j.judge(0)?;
    if v0.pass || v0.missing != 5 || v0.first_missing != 0 || v0.code != E_MISSING {
        return Err(E_MISSING);
    }
    if v0.explain() == "" {
        return Err(E_MISSING);
    }
    hits += 1;

    // R4：穷举 256 组——合格组数必须恰为 2^3=8（可选位自由），并逐组对照人工判定。
    let mut expect_accept = 0u32;
    let mut m = 0u32;
    while m <= ALL_MASK {
        let (pass, missing, first) = j.judge_mask(m);
        let manual = j.manual_judge(m);
        if pass != manual {
            return Err(E_DISAGREE);
        }
        if pass {
            expect_accept += 1;
            if missing != 0 || first != FN_COUNT {
                return Err(E_ACCT);
            }
        } else if missing == 0 || first >= FN_COUNT {
            return Err(E_ACCT);
        }
        m += 1;
    }
    if expect_accept != 8 {
        return Err(E_ACCT); // 2^(8-5) = 8
    }
    hits += 1;

    // R5：1,000 组确定性采样——裁断与人工判定 1000/1000 一致（判据主断言）。
    let mut rep = SampleReport {
        samples: 0,
        accepts: 0,
        rejects: 0,
        distinct: 0,
        disagreements: 0,
    };
    let mut lcg = Lcg::new(0x1027_0006);
    let mut s = 0u32;
    while s < SAMPLES {
        let mask = lcg.next_mask();
        if (mask & !ALL_MASK) != 0 {
            return Err(E_BITMAP);
        }
        let want = {
            // 人工判定（独立于裁断器）
            let mut cnt = 0u32;
            let mut k = 0usize;
            while k < FN_COUNT {
                if MIN_SET[k].mandatory && (mask & (1u32 << (k as u32))) != 0 {
                    cnt += 1;
                }
                k += 1;
            }
            cnt == MAND_COUNT
        };
        let got = j.judge(mask)?;
        if got.pass != want {
            rep.disagreements += 1;
            return Err(E_DISAGREE);
        }
        if got.pass {
            rep.accepts += 1;
        } else {
            rep.rejects += 1;
        }
        if j.note_mask(mask) {
            rep.distinct += 1;
        }
        rep.samples += 1;
        s += 1;
    }
    if rep.samples != SAMPLES {
        return Err(E_SAMPLE);
    }
    if !rep.consistent() {
        return Err(E_ACCT);
    }
    if rep.disagreements != 0 {
        return Err(E_DISAGREE);
    }
    hits += 1;

    // R6：可解释性——不合格结论必须给出「首个缺失下标 + 缺失数 + 依据语」三要素。
    let probe = 0x1u32 << 2; // 只实现 get_tree，缺 alloc/destroy/kill/lookup
    let vp = j.judge(probe)?;
    if vp.pass {
        return Err(E_ACCT);
    }
    if vp.missing != 4 || vp.first_missing != 0 {
        return Err(E_MISSING);
    }
    if vp.explain() == "" {
        return Err(E_MISSING);
    }
    hits += 1;

    // I1：闭集外位 → E_BITMAP（第 8 位置位）。
    if j.judge(1u32 << FN_COUNT) != Err(E_BITMAP) {
        return Err(E_BITMAP);
    }
    if j.judge(0xFFFF_FFFF) != Err(E_BITMAP) {
        return Err(E_BITMAP);
    }
    hits += 1;

    // I2：裁断账守恒——accepts + rejects == 实际 judge 调用次数。
    // 调用次数：R2 1 + R3 1 + R5 1000 + R6 1 = 1003
    let calls = 1 + 1 + SAMPLES + 1;
    if j.accepts + j.rejects != calls {
        return Err(E_ACCT);
    }
    if j.disagreements != 0 {
        return Err(E_DISAGREE);
    }
    hits += 1;

    // I3：采样去重合理——去重数 ≤ 采样数，且 ≥ 8（8 位掩码空间随机应覆盖大部分）。
    if rep.distinct > rep.samples || rep.distinct < 8 {
        return Err(E_SAMPLE);
    }
    // 相位机终态：最后一次裁断为不合格 → Rejected
    if j.state != AdjState::Rejected {
        return Err(E_STATE);
    }
    hits += 1;

    // C1：合格率对账——1,000 组中合格数应约为 1/32（5 必需位全置概率 2^-5），
    //     取宽区间 [10, 120] 判稳（确定性种子下实际值固定）。
    let pm = rep.pass_permille();
    if pm < 10 || pm > 120 {
        return Err(E_ACCT);
    }
    if rep.accepts + rep.rejects != SAMPLES {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
