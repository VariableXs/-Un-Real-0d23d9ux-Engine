//! UNX-F56195 启动缺侧 N/A 深化实例件（0x61860 段）
//!
//! 锚点：UNX-F56195-J1 N/A 实例账（缺侧深化实例——实例号 × 缺侧子
//! 段 × 缺侧原因簇 × 补侧计划逐实例落账）；J2 实例语义（缺侧原因闭
//! 集合法 + 补侧计划非空 + N/A 显式三性质）；J3 实例指纹（实例账折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（启动域·缺侧 N/A 面面）。
//!
//! 结构：缺侧原因闭集 + N/A 实例簿 + 实例核（原因闸 + 计划闸 + 显式
//! 闸）+ 实例指纹 + 注入核（未知原因/空计划/未显式 N/A/重实例号/印
//! 后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61860 独占 0x20）。
pub const SEG_BASE: u32 = 0x61860;
pub const NA_UNK_REASON: u32 = SEG_BASE + 0x01; // 未知缺侧原因
pub const NA_EMPTY_PLAN: u32 = SEG_BASE + 0x02; // 空补侧计划
pub const NA_INEXPLICIT: u32 = SEG_BASE + 0x03; // 未显式 N/A
pub const NA_DUP_INST: u32 = SEG_BASE + 0x04; // 重实例号
pub const NA_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 缺侧原因闭集。
pub const NR_NO_AGENT: usize = 0; // 对侧无采集代理
pub const NR_NO_PERM: usize = 1; // 对侧无权限
pub const NR_STAGE_ABSENT: usize = 2; // 对侧无该阶段
pub const NR_COUNT: usize = 3;

pub const REASON_NAMES: [&str; NR_COUNT] = ["no-agent", "no-perm", "stage-absent"];

/// 一条 N/A 深化实例。
#[derive(Clone)]
pub struct NaInstance {
    pub inst_no: u32,
    pub sub_name: String,
    pub reason: usize,
    pub plan: String,
}

impl NaInstance {
    /// 显式 N/A：标记位由原因合法 + 计划非空共同支撑。
    pub fn explicit_ok(&self) -> bool {
        self.reason < NR_COUNT && !self.plan.is_empty() && !self.sub_name.is_empty()
    }

    fn render_line(&self) -> String {
        format!(
            "NA{}|{}|{}|plan={}",
            self.inst_no, self.sub_name, REASON_NAMES[self.reason], self.plan
        )
    }
}

/// N/A 实例簿。
pub struct NaRegistry {
    insts: Vec<NaInstance>,
    sealed: bool,
}

impl NaRegistry {
    pub fn new() -> Self {
        NaRegistry {
            insts: Vec::new(),
            sealed: false,
        }
    }

    /// 登记实例：原因闸 + 计划闸 + 子段名闸 + 重号拒 + 印后拒。
    pub fn register(&mut self, inst: NaInstance) -> Result<(), u32> {
        if self.sealed {
            return Err(NA_FROZEN);
        }
        if inst.reason >= NR_COUNT {
            return Err(NA_UNK_REASON);
        }
        if inst.plan.is_empty() {
            return Err(NA_EMPTY_PLAN);
        }
        if inst.sub_name.is_empty() {
            return Err(NA_INEXPLICIT);
        }
        if self.insts.iter().any(|x| x.inst_no == inst.inst_no) {
            return Err(NA_DUP_INST);
        }
        self.insts.push(inst);
        Ok(())
    }

    pub fn inst(&self, no: u32) -> Option<&NaInstance> {
        self.insts.iter().find(|x| x.inst_no == no)
    }

    pub fn len(&self) -> usize {
        self.insts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.insts.is_empty()
    }

    /// 子段查实例。
    pub fn by_sub(&self, sub: &str) -> Option<&NaInstance> {
        self.insts.iter().find(|x| x.sub_name == sub)
    }

    /// 各原因计数。
    pub fn reason_counts(&self) -> [usize; NR_COUNT] {
        let mut c = [0usize; NR_COUNT];
        for x in &self.insts {
            if x.reason < NR_COUNT {
                c[x.reason] += 1;
            }
        }
        c
    }

    /// 实例指纹。
    pub fn reg_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for x in &self.insts {
            h = h.wrapping_mul(0x0100_0193) ^ x.inst_no;
            for b in x.sub_name.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
            h = h.wrapping_mul(0x0100_0193) ^ x.reason as u32;
            for b in x.plan.bytes() {
                h = h.wrapping_mul(0x0100_0193) ^ b as u32;
            }
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封簿印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.insts.is_empty() || !self.insts.iter().all(|x| x.explicit_ok()) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render_reg(&self) -> String {
        let mut out = String::new();
        for x in &self.insts {
            out.push_str(&x.render_line());
            out.push('\n');
        }
        if self.sealed {
            out.push_str("NAREG-SEAL\n");
        }
        out
    }
}

impl Default for NaRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三实例（cred 无代理 / evtpath 阶段缺席 / menu 无权限）。
fn standard_reg() -> NaRegistry {
    let mut r = NaRegistry::new();
    let _ = r.register(NaInstance {
        inst_no: 1,
        sub_name: "cred".to_string(),
        reason: NR_NO_AGENT,
        plan: "deploy-agent-cred".to_string(),
    });
    let _ = r.register(NaInstance {
        inst_no: 2,
        sub_name: "evtpath".to_string(),
        reason: NR_STAGE_ABSENT,
        plan: "map-to-input-stack".to_string(),
    });
    let _ = r.register(NaInstance {
        inst_no: 3,
        sub_name: "menu".to_string(),
        reason: NR_NO_PERM,
        plan: "request-bcd-read".to_string(),
    });
    r
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：未知原因
    let mut r = NaRegistry::new();
    if r.register(NaInstance {
        inst_no: 1,
        sub_name: "x".to_string(),
        reason: 9,
        plan: "p".to_string(),
    }) == Err(NA_UNK_REASON)
    {
        caught += 1;
    }
    // 注 2：空计划
    let mut r = NaRegistry::new();
    if r.register(NaInstance {
        inst_no: 1,
        sub_name: "x".to_string(),
        reason: NR_NO_AGENT,
        plan: String::new(),
    }) == Err(NA_EMPTY_PLAN)
    {
        caught += 1;
    }
    // 注 3：未显式 N/A（空子段名）
    let mut r = NaRegistry::new();
    if r.register(NaInstance {
        inst_no: 1,
        sub_name: String::new(),
        reason: NR_NO_AGENT,
        plan: "p".to_string(),
    }) == Err(NA_INEXPLICIT)
    {
        caught += 1;
    }
    // 注 4：重实例号
    let mut r = standard_reg();
    if r.register(NaInstance {
        inst_no: 1,
        sub_name: "y".to_string(),
        reason: NR_NO_AGENT,
        plan: "p".to_string(),
    }) == Err(NA_DUP_INST)
    {
        caught += 1;
    }
    // 注 5：印后登记
    let mut r = standard_reg();
    let _ = r.seal();
    if r.register(NaInstance {
        inst_no: 9,
        sub_name: "z".to_string(),
        reason: NR_NO_AGENT,
        plan: "p".to_string(),
    }) == Err(NA_FROZEN)
    {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 N/A 实例账；J2 实例语义；J3 实例指纹。
pub fn run_unx56195_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56195");
    // J1-1 三实例全登记
    let r = standard_reg();
    let ok = r.len() == 3 && (1..=3).all(|n| r.inst(n).is_some());
    s.add("56195-J1-三实例全登记", ok, if ok { "" } else { "实例账不全" });
    // J1-2 渲染导出齐
    let mut r2 = standard_reg();
    let _ = r2.seal();
    let text = r2.render_reg();
    let ok = text.contains("NA1|cred|no-agent|plan=deploy-agent-cred")
        && text.contains("NA3|menu|no-perm|plan=request-bcd-read")
        && text.contains("NAREG-SEAL");
    s.add("56195-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 显式 N/A 三性质（全部实例）
    let ok = (1..=3).all(|n| r.inst(n).map(|x| x.explicit_ok()).unwrap_or(false));
    s.add("56195-J2-显式NA", ok, if ok { "" } else { "显式性漂移" });
    // J2-2 原因闭集合法
    let ok = r.reason_counts().iter().sum::<usize>() == 3;
    s.add("56195-J2-原因闭集", ok, if ok { "" } else { "原因越界" });
    // J2-3 子段查实例（cred → 1）
    let ok = r.by_sub("cred").map(|x| x.inst_no == 1).unwrap_or(false);
    s.add("56195-J2-子段查实例", ok, if ok { "" } else { "子段查漂移" });
    // J2-4 计划非空抽查
    let ok = r.inst(2).map(|x| !x.plan.is_empty()).unwrap_or(false);
    s.add("56195-J2-计划非空", ok, if ok { "" } else { "计划漂移" });
    // J2-5 重号拒
    let mut r3 = standard_reg();
    let ok = r3
        .register(NaInstance {
            inst_no: 2,
            sub_name: "dup".to_string(),
            reason: NR_NO_AGENT,
            plan: "p".to_string(),
        })
        == Err(NA_DUP_INST);
    s.add("56195-J2-重号拒", ok, if ok { "" } else { "重号闸失效" });
    // J2-6 未知原因拒
    let mut r4 = NaRegistry::new();
    let ok = r4
        .register(NaInstance {
            inst_no: 1,
            sub_name: "x".to_string(),
            reason: 42,
            plan: "p".to_string(),
        })
        == Err(NA_UNK_REASON);
    s.add("56195-J2-未知原因拒", ok, if ok { "" } else { "原因闸失效" });
    // J3-1 实例指纹确定
    let ok = standard_reg().reg_fp() == standard_reg().reg_fp();
    s.add("56195-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 实例变签变
    let mut r5 = standard_reg();
    let fp0 = r5.reg_fp();
    if let Some(x) = r5.insts.first_mut() {
        x.plan.push('x');
    }
    let ok = r5.reg_fp() != fp0;
    s.add("56195-J3-实例变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封簿印
    let mut r6 = standard_reg();
    let ok = r6.seal() && r6.sealed();
    s.add("56195-J3-封簿印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒登记
    let ok = r6
        .register(NaInstance {
            inst_no: 9,
            sub_name: "z".to_string(),
            reason: NR_NO_AGENT,
            plan: "p".to_string(),
        })
        == Err(NA_FROZEN);
    s.add("56195-J3-印后拒登记", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56195-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56195_all_checks_green() {
        let set = run_unx56195_checks();
        assert!(set.all_passed(), "UNX-F56195 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56195_reg_stable() {
        let mut r = standard_reg();
        assert!(r.seal());
    }
}
