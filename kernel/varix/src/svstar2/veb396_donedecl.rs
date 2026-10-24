//! veb396_donedecl —— VE-F0396 B 域完成宣告。
//! 锚点（CoRun Unxreal 主册）：VE-F0396 · B 域完成宣告 · 目标 300 行
//! 判据原文：VE-F0396-J1 完成宣告前置四闸：判据闸（全子卡绿）、
//!           缺陷闸（致命清零）、文档闸（文档索引完整）、验收
//!           闸（验收矩阵收）；四闸全过才许宣告（任何一闸红
//!           拒宣告并报闸名）；宣告出完成章（时间戳 + 域号哈
//!           希）；重复宣告拒（一章定音）；宣告后变更拒（冻
//!           结生效）。
//! 组件：四闸检查器 + 完成章 + 冻结态。
//! 职责定位：完成宣告是「B 域的毕业典礼」——四闸是四张成绩单：
//!           判据是学分会考，缺陷是操行评定，文档是论文归档，
//!           验收是毕业答辩；缺一不可；完成章一发全域冻结，
//!           毕业了还回教室改考卷是流程事故。
//! 性能逐项分解：闸检 O(输入规模)；章 O(1)。
//! 诊断码段：0x30880（0x30881~0x30889 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const DD_VERSION: &str = "veb396_donedecl-v1";

/// 诊断段基址（0x30880 段 · 本模块独占）。
pub const DD_SEG_BASE: u32 = 0x30880;
pub const XC_GATE_CHECK: u32 = 0x30881; // 判据闸红
pub const XC_GATE_FATAL: u32 = 0x30882; // 缺陷闸红
pub const XC_GATE_DOC: u32 = 0x30883; // 文档闸红
pub const XC_GATE_ACCEPT: u32 = 0x30884; // 验收闸红
pub const XC_TWICE: u32 = 0x30885; // 重复宣告
pub const XC_FROZEN: u32 = 0x30886; // 宣告后变更
pub const XC_MAGIC: u32 = 0x30887; // 账魔数破
pub const XC_STAMP: u32 = 0x30888; // 章不一致
pub const XC_INPUT: u32 = 0x30889; // 闸输入不合法

/// 四闸输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateInput {
    /// 判据全绿。
    pub checks_green: bool,
    /// 致命缺陷开放数。
    pub fatal_open: u32,
    /// 文档索引完整（注册数 = 索引数）。
    pub doc_complete: bool,
    /// 验收矩阵已收（全过）。
    pub accepted: bool,
}

/// 完成宣告器。
pub struct DoneDecl {
    declared: bool,
    stamp: Option<u32>,
    domain: &'static str,
    magic: u32,
}

impl DoneDecl {
    /// 建。
    pub fn new(domain: &'static str) -> DoneDecl {
        DoneDecl { declared: false, stamp: None, domain, magic: 0x0880_0E0C }
    }

    /// 四闸检查：逐闸过，红闸报码。
    pub fn check_gates(&self, gi: &GateInput) -> Result<(), u32> {
        if self.magic != 0x0880_0E0C {
            return Err(XC_MAGIC);
        }
        if !gi.checks_green {
            return Err(XC_GATE_CHECK);
        }
        if gi.fatal_open > 0 {
            return Err(XC_GATE_FATAL);
        }
        if !gi.doc_complete {
            return Err(XC_GATE_DOC);
        }
        if !gi.accepted {
            return Err(XC_GATE_ACCEPT);
        }
        Ok(())
    }

    /// 宣告（先过闸）：完成章 = FNV32(域号, 时间戳)。
    pub fn declare(&mut self, gi: &GateInput, ts: u32) -> Result<u32, u32> {
        self.check_gates(gi)?;
        if self.declared {
            return Err(XC_TWICE);
        }
        let mut h: u32 = 0x811C_9DC5;
        for b in self.domain.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= ts;
        h = h.wrapping_mul(0x0100_0193);
        self.stamp = Some(h);
        self.declared = true;
        Ok(h)
    }

    /// 宣告后变更拒。
    pub fn mutate(&self) -> Result<(), u32> {
        if self.declared {
            return Err(XC_FROZEN);
        }
        Ok(())
    }

    /// 章核。
    pub fn verify(&self, ts: u32) -> Result<bool, u32> {
        let st = match self.stamp {
            Some(s) => s,
            None => return Err(XC_STAMP),
        };
        let mut h: u32 = 0x811C_9DC5;
        for b in self.domain.as_bytes() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= ts;
        h = h.wrapping_mul(0x0100_0193);
        Ok(st == h)
    }

    /// 宣告态。
    pub fn is_declared(&self) -> bool {
        self.declared
    }
}

pub fn run_veb396_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0396");
    let good = GateInput { checks_green: true, fatal_open: 0, doc_complete: true, accepted: true };
    // 族一：逐闸红。
    let mut d = DoneDecl::new("VE-B");
    let bad1 = GateInput { checks_green: false, ..good };
    s.add("判据闸红", d.check_gates(&bad1) == Err(XC_GATE_CHECK), "第一闸");
    let bad2 = GateInput { fatal_open: 2, ..good };
    s.add("缺陷闸红", d.check_gates(&bad2) == Err(XC_GATE_FATAL), "第二闸");
    let bad3 = GateInput { doc_complete: false, ..good };
    s.add("文档闸红", d.check_gates(&bad3) == Err(XC_GATE_DOC), "第三闸");
    let bad4 = GateInput { accepted: false, ..good };
    s.add("验收闸红", d.check_gates(&bad4) == Err(XC_GATE_ACCEPT), "第四闸");
    s.add("四闸全过", d.check_gates(&good).is_ok(), "放行");
    s.add("闸红不宣告", d.is_declared() == false, "还没毕业");
    // 族二：宣告。
    let st = d.declare(&good, 0x2026_1022);
    s.add("宣告出章", st.is_ok(), "毕业");
    s.add("已宣告", d.is_declared(), "一章定音");
    s.add("重复宣告拒", d.declare(&good, 1) == Err(XC_TWICE), "二次毕业");
    s.add("章核过", d.verify(0x2026_1022) == Ok(true), "同戳验真");
    s.add("错戳验伪", d.verify(1) == Ok(false), "异戳拒");
    // 族三：宣告后冻结。
    s.add("宣告后变更拒", d.mutate() == Err(XC_FROZEN), "冻结生效");
    // 族四：宣告前可变。
    let mut e = DoneDecl::new("VE-B2");
    s.add("宣告前可变", e.mutate().is_ok(), "未冻结");
    // 族五：先闸后章次序不可绕（闸红直接宣告拒）。
    let mut f = DoneDecl::new("VE-B3");
    let badf = GateInput { checks_green: false, ..good };
    s.add("闸红宣告拒", f.declare(&badf, 1) == Err(XC_GATE_CHECK), "闸先行");
    s.add("仍可补闸宣告", f.declare(&good, 7).is_ok(), "补考过");
    s.add("魔数核", DD_SEG_BASE == 0x30880, "段基址");
    s
}
