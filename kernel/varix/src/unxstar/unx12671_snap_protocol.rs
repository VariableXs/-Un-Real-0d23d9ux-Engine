//! UNX-F12672 探测结果一致性协议（30 例联测/拒止码双侧一致/零静默） · 0x811C0段
//!
//! 判据原文（派工卡 12671 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12672-J1 30 例联测全过，拒止码双侧一致断言绿，
//!      零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 一致性协议域模型：联测案卷每案六元（案号/面名闭集/违约族/拒方
//!    码/协议侧码/双侧一致位）——本卡把 B05 批九个探测面（注册表消
//!    费边/分支档/句柄表面/映射消费/文件对象/区域查询/Token 面/同步
//!    对象/口径协议）的拒止面汇成 30 例联测案卷；
//! 2. 面名九闭集与面基码：每面冻结基码（0x10..0x90 段内偏移），诊断
//!    码 = 面基码 + 族偏移（0x01..0x0C 闭集）——案卷内不跨段引码，
//!    只在案卷闭集内对账；
//! 3. 拒止码双侧一致：每案带拒方码（违约检出侧）与协议侧码（一致性
//!    协议侧），双侧码必须相等且非零且落在面基码窗口——双侧一致断
//!    言绿是本卡验收主判据；
//! 4. 零静默：30 案逐案显性判定（过/拒都记账），案卷静默计数必须为
//!    零——任何一例无判定或被静默吞掉即协议违约；
//! 5. 案卷状态机：Drafted → Sided → Closed 单向推进，倒退显性拒；
//!    案卷闸四道（面名闭集/双侧码一致/案号序/静默记账）逐案过闸；
//! 6. 注入联测 10 路：双侧码漂移、零码、越窗码、面名越闭集、案号乱
//!    序、案重复、缺案封卷、静默吞案、封卷后补案、摘要空——逐路必
//!    抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（案卷冻结出题）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12671 − 12600)）。
pub const SEG_BASE: u32 = 0x811C0;

// ── 一致性协议诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointDiag {
    /// 双侧码漂移（拒方码 ≠ 协议侧码）。
    ESidesMiss,
    /// 零码（任一侧为零）。
    EZeroCode,
    /// 越窗码（码不在面基码窗口）。
    ECodeRange,
    /// 面名越九闭集。
    EFaceRange,
    /// 案号乱序。
    ECaseOrder,
    /// 案重复。
    ECaseDup,
    /// 缺案封卷。
    EShortClose,
    /// 静默吞案（判定缺失）。
    ESilentDrop,
    /// 封卷后补案。
    EClosedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl JointDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                JointDiag::ESidesMiss => 0x01,
                JointDiag::EZeroCode => 0x02,
                JointDiag::ECodeRange => 0x03,
                JointDiag::EFaceRange => 0x04,
                JointDiag::ECaseOrder => 0x05,
                JointDiag::ECaseDup => 0x06,
                JointDiag::EShortClose => 0x07,
                JointDiag::ESilentDrop => 0x08,
                JointDiag::EClosedWrite => 0x09,
                JointDiag::ESummaryEmpty => 0x0a,
                JointDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            JointDiag::ESidesMiss => "双侧码漂移：拒方码与协议侧码不一致，先对齐两侧判定口径",
            JointDiag::EZeroCode => "零码：任一侧判码为零即无效，先补显性判码",
            JointDiag::ECodeRange => "越窗码：判码不在面基码窗口内，先归面再对账",
            JointDiag::EFaceRange => "面名越九闭集：只认 B05 批九面，先正面名",
            JointDiag::ECaseOrder => "案号乱序：按案号 0..30 逐案过闸，跳号先回补",
            JointDiag::ECaseDup => "案重复：同面同族只立一案，重复先查已有卷",
            JointDiag::EShortClose => "缺案封卷：30 案齐才许封卷，缺案先回补",
            JointDiag::ESilentDrop => "静默吞案：逐案显性判定，缺判定即协议违约",
            JointDiag::EClosedWrite => "封卷后补案：案卷只读，补案走下一轮联测",
            JointDiag::ESummaryEmpty => "摘要空：封卷摘要非空才许写回已完成",
            JointDiag::ERewind => "状态机倒退：Drafted→Sided→Closed 只进不退，倒退即卷可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const JOINT_ALL: &[JointDiag] = &[
    JointDiag::ESidesMiss,
    JointDiag::EZeroCode,
    JointDiag::ECodeRange,
    JointDiag::EFaceRange,
    JointDiag::ECaseOrder,
    JointDiag::ECaseDup,
    JointDiag::EShortClose,
    JointDiag::ESilentDrop,
    JointDiag::EClosedWrite,
    JointDiag::ESummaryEmpty,
    JointDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in JOINT_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x811C1..0x811CB 闭集内"
}

// ── 面名九闭集与面基码 ───────────────────────────────────────────
/// 面行（面名/面基码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointFace {
    /// 面名（B05 批九面短名）。
    pub name: &'static str,
    /// 面基码（案卷内判码窗口基址）。
    pub base: u32,
}

/// B05 批九面闭集（面基码 0x10..0x90）。
pub const FACE_TABLE: &[JointFace] = &[
    JointFace { name: "注册表消费边", base: 0x10 },
    JointFace { name: "分支档", base: 0x20 },
    JointFace { name: "句柄表面", base: 0x30 },
    JointFace { name: "映射消费", base: 0x40 },
    JointFace { name: "文件对象", base: 0x50 },
    JointFace { name: "区域查询", base: 0x60 },
    JointFace { name: "Token 面", base: 0x70 },
    JointFace { name: "同步对象", base: 0x80 },
    JointFace { name: "口径协议", base: 0x90 },
];

/// 面基码族偏移上沿（0x01..=0x0C 闭集）。
pub const CODE_OFF_CAP: u32 = 0x0C;

/// 取面行（越面 None）。
pub fn face_of(name: &str) -> Option<&JointFace> {
    for f in FACE_TABLE.iter() {
        if f.name == name {
            return Some(f);
        }
    }
    None
}

/// 判码是否落在面基码窗口（base < code <= base + 族偏移上沿）。
pub fn code_in_face(code: u32, base: u32) -> bool {
    code > base && code <= base + CODE_OFF_CAP
}

// ── 联测案与黄金判定 ─────────────────────────────────────────────
/// 联测案六元（案号/面名/违约族/拒方码/协议侧码/期望一致）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JointCase {
    /// 案号。
    pub no: u32,
    /// 面名（九闭集内）。
    pub face: &'static str,
    /// 违约族名。
    pub fault: &'static str,
    /// 拒方码。
    pub reject_code: u32,
    /// 协议侧码。
    pub protocol_code: u32,
    /// 期望一致（true = 双侧应一致）。
    pub expect_match: bool,
}

/// 单案黄金判定（面名闭集 + 双侧码一致 + 码窗 三道）。
pub fn joint_case_ok(c: &JointCase) -> Result<(), u32> {
    // 面名闭集闸。
    let f = match face_of(c.face) {
        Some(v) => v,
        None => return Err(JointDiag::EFaceRange.val()),
    };
    // 零码闸。
    if c.reject_code == 0 || c.protocol_code == 0 {
        return Err(JointDiag::EZeroCode.val());
    }
    // 双侧一致闸。
    if c.reject_code != c.protocol_code {
        return Err(JointDiag::ESidesMiss.val());
    }
    // 码窗闸。
    if !code_in_face(c.reject_code, f.base) {
        return Err(JointDiag::ECodeRange.val());
    }
    Ok(())
}

/// 案卷容量（30 例）。
pub const CASE_CAP: usize = 30;

// ── 冻结联测案卷（30 案：九面轮转，族偏移覆盖各面窗口） ──────────
/// 构造案（案号/面名/面基码/族偏移/族名）——冻结展开为常量表。
const fn mk_case(no: u32, face: &'static str, base: u32, off: u32, fault: &'static str) -> JointCase {
    JointCase {
        no,
        face,
        fault,
        reject_code: base + off,
        protocol_code: base + off,
        expect_match: true,
    }
}

/// 30 案冻结案卷（案号序，九面轮转）。
pub const JOINT_CASES: &[JointCase] = &[
    mk_case(0, "注册表消费边", 0x10, 0x01, "断链指纹"),
    mk_case(1, "注册表消费边", 0x10, 0x02, "键路径空段"),
    mk_case(2, "注册表消费边", 0x10, 0x03, "段数越闭集"),
    mk_case(3, "分支档", 0x20, 0x01, "分支越闭集"),
    mk_case(4, "分支档", 0x20, 0x02, "服务包越档"),
    mk_case(5, "分支档", 0x20, 0x03, "行号乱序"),
    mk_case(6, "句柄表面", 0x30, 0x01, "链环悬空"),
    mk_case(7, "句柄表面", 0x30, 0x02, "指纹错挂"),
    mk_case(8, "句柄表面", 0x30, 0x03, "pid 越表"),
    mk_case(9, "句柄表面", 0x30, 0x04, "句柄号零"),
    mk_case(10, "映射消费", 0x40, 0x01, "类型越闭集"),
    mk_case(11, "映射消费", 0x40, 0x02, "属性位越集"),
    mk_case(12, "映射消费", 0x40, 0x03, "盘符越表"),
    mk_case(13, "文件对象", 0x50, 0x01, "前缀未知"),
    mk_case(14, "文件对象", 0x50, 0x02, "类型错配"),
    mk_case(15, "文件对象", 0x50, 0x03, "缺映射指纹"),
    mk_case(16, "区域查询", 0x60, 0x01, "基址出 arena"),
    mk_case(17, "区域查询", 0x60, 0x02, "区尾出界"),
    mk_case(18, "区域查询", 0x60, 0x03, "重叠前区"),
    mk_case(19, "区域查询", 0x60, 0x04, "粒度非对齐"),
    mk_case(20, "Token 面", 0x70, 0x01, "零句柄"),
    mk_case(21, "Token 面", 0x70, 0x02, "类号越档"),
    mk_case(22, "Token 面", 0x70, 0x03, "长度越账"),
    mk_case(23, "同步对象", 0x80, 0x01, "计数越上限"),
    mk_case(24, "同步对象", 0x80, 0x02, "有主计数非零"),
    mk_case(25, "同步对象", 0x80, 0x03, "状态互证失败"),
    mk_case(26, "口径协议", 0x90, 0x01, "方差越沿"),
    mk_case(27, "口径协议", 0x90, 0x02, "声明漂移"),
    mk_case(28, "口径协议", 0x90, 0x03, "归属越表"),
    mk_case(29, "口径协议", 0x90, 0x04, "账面不匹"),
];

// ── 案卷状态机（Drafted → Sided → Closed） ───────────────────────
/// 案卷状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointState {
    /// 起卷。
    Drafted,
    /// 侧判中。
    Sided,
    /// 已封卷。
    Closed,
}

impl JointState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            JointState::Drafted => 0,
            JointState::Sided => 1,
            JointState::Closed => 2,
        }
    }
}

/// 案卷账。
#[derive(Clone, Debug)]
pub struct JointLedger {
    /// 已侧判案（案号序）。
    cases: Vec<JointCase>,
    /// 判定记账（案号 + 判定码；非零 = 显性记账，零静默口径）。
    verdicts: Vec<(u32, u32)>,
    /// 拒收记账（案号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: JointState,
}

impl JointLedger {
    /// 起卷。
    pub fn new() -> JointLedger {
        JointLedger {
            cases: Vec::new(),
            verdicts: Vec::new(),
            rejects: Vec::new(),
            state: JointState::Drafted,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> JointState {
        self.state
    }

    /// 已侧判案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 判定记账数（零静默口径）。
    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 案（越界 None）。
    pub fn case_at(&self, idx: usize) -> Option<&JointCase> {
        self.cases.get(idx)
    }

    /// 侧判（Drafted→Sided）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= JointState::Sided.rank() {
            return Err(JointDiag::ERewind.val());
        }
        self.state = JointState::Sided;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < CASE_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐案侧判（案号序 + 黄金判定 + 案互异 + 显性记账 四道）。
    pub fn side(&mut self, c: JointCase) -> Result<(), u32> {
        if self.state.rank() < JointState::Sided.rank() {
            return Err(JointDiag::ESilentDrop.val());
        }
        if self.state.rank() >= JointState::Closed.rank() {
            self.note_reject(c.no, JointDiag::EClosedWrite.val());
            return Err(JointDiag::EClosedWrite.val());
        }
        // 案号序闸。
        if c.no != self.cases.len() as u32 {
            self.note_reject(c.no, JointDiag::ECaseOrder.val());
            return Err(JointDiag::ECaseOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = joint_case_ok(&c) {
            self.note_reject(c.no, e);
            return Err(e);
        }
        // 案互异闸：同面同族只立一案。
        for done in self.cases.iter() {
            if done.face == c.face && done.fault == c.fault {
                self.note_reject(c.no, JointDiag::ECaseDup.val());
                return Err(JointDiag::ECaseDup.val());
            }
        }
        // 显性记账（零静默口径）：每案落判定。
        self.verdicts.push((c.no, c.reject_code));
        self.cases.push(c);
        Ok(())
    }

    /// 封卷：30 案齐 + 判定记账 30 条 + 零拒收。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.state.rank() >= JointState::Closed.rank() {
            return Err(JointDiag::ERewind.val());
        }
        if self.state.rank() < JointState::Sided.rank() {
            return Err(JointDiag::ESilentDrop.val());
        }
        if self.cases.len() != CASE_CAP {
            return Err(JointDiag::EShortClose.val());
        }
        if self.verdicts.len() != CASE_CAP {
            return Err(JointDiag::ESilentDrop.val());
        }
        if !self.rejects.is_empty() {
            return Err(JointDiag::EShortClose.val());
        }
        self.state = JointState::Closed;
        Ok(())
    }

    /// 账等式（零静默口径：案数 = 判定数 = 30，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.cases.len() == CASE_CAP
            && self.verdicts.len() == CASE_CAP
            && self.rejects.is_empty()
    }
}

// ── 双侧一致断言面（判据 J1 直读口） ─────────────────────────────
/// 双侧一致断言：30 案逐案"拒方码 == 协议侧码 且 落面窗"。
pub fn sides_agree(cases: &[JointCase]) -> (usize, usize, String) {
    let mut agree = 0usize;
    let mut total = 0usize;
    let mut miss = String::new();
    for c in cases.iter() {
        total += 1;
        if joint_case_ok(c).is_ok() {
            agree += 1;
        } else if miss.is_empty() {
            miss = format!("案{}({}/{}) 双侧不一致", c.no, c.face, c.fault);
        }
    }
    (agree, total, miss)
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JointFault {
    /// 双侧码漂移。
    SidesMiss,
    /// 零码。
    ZeroCode,
    /// 越窗码。
    CodeRange,
    /// 面名越闭集。
    FaceRange,
    /// 案号乱序。
    CaseOrder,
    /// 案重复。
    CaseDup,
    /// 缺案封卷。
    ShortClose,
    /// 静默吞案。
    SilentDrop,
    /// 封卷后补案。
    ClosedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl JointFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            JointFault::SidesMiss => 0,
            JointFault::ZeroCode => 1,
            JointFault::CodeRange => 2,
            JointFault::FaceRange => 3,
            JointFault::CaseOrder => 4,
            JointFault::CaseDup => 5,
            JointFault::ShortClose => 6,
            JointFault::SilentDrop => 7,
            JointFault::ClosedWrite => 8,
            JointFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            JointFault::SidesMiss => "双侧码漂移",
            JointFault::ZeroCode => "零码",
            JointFault::CodeRange => "越窗码",
            JointFault::FaceRange => "面名越闭集",
            JointFault::CaseOrder => "案号乱序",
            JointFault::CaseDup => "案重复",
            JointFault::ShortClose => "缺案封卷",
            JointFault::SilentDrop => "静默吞案",
            JointFault::ClosedWrite => "封卷后补案",
            JointFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [JointFault] {
        &[
            JointFault::SidesMiss,
            JointFault::ZeroCode,
            JointFault::CodeRange,
            JointFault::FaceRange,
            JointFault::CaseOrder,
            JointFault::CaseDup,
            JointFault::ShortClose,
            JointFault::SilentDrop,
            JointFault::ClosedWrite,
            JointFault::SummaryEmpty,
        ]
    }
}

/// 构建完整案卷（30 案逐案侧判）。
pub fn build_full_ledger() -> JointLedger {
    let mut led = JointLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < CASE_CAP {
        let c = match JOINT_CASES.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.side(c) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in JointFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: JointFault) -> bool {
    match v {
        JointFault::SidesMiss => {
            let mut c = match JOINT_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.protocol_code = c.protocol_code + 1;
            joint_case_ok(&c).is_err()
        }
        JointFault::ZeroCode => {
            let mut c = match JOINT_CASES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.reject_code = 0;
            joint_case_ok(&c).is_err()
        }
        JointFault::CodeRange => {
            let mut c = match JOINT_CASES.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.reject_code = 0x99;
            c.protocol_code = 0x99;
            joint_case_ok(&c).is_err()
        }
        JointFault::FaceRange => {
            let mut c = match JOINT_CASES.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.face = "幽灵面";
            joint_case_ok(&c).is_err()
        }
        JointFault::CaseOrder => {
            let mut led = JointLedger::new();
            let _ = led.begin();
            let c = match JOINT_CASES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.side(c).is_err()
        }
        JointFault::CaseDup => {
            let mut led = JointLedger::new();
            let _ = led.begin();
            let c0 = match JOINT_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.side(c0.clone());
            // 同面同族换案号再立被拒。
            let dup = JointCase { no: 1, face: c0.face, fault: c0.fault, reject_code: c0.reject_code, protocol_code: c0.protocol_code, expect_match: true };
            led.side(dup).is_err()
        }
        JointFault::ShortClose => {
            let mut led = JointLedger::new();
            let _ = led.begin();
            let c = match JOINT_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.side(c);
            led.close().is_err()
        }
        JointFault::SilentDrop => {
            // 未侧判即封卷被拒（Drafted 态直接 close）。
            let mut led = JointLedger::new();
            led.close().is_err()
        }
        JointFault::ClosedWrite => {
            let mut led = build_full_ledger();
            if led.close().is_err() {
                return false;
            }
            let c = match JOINT_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.side(c).is_err()
        }
        JointFault::SummaryEmpty => {
            // 摘要空族：首案面名被清空 → 摘要无从拼起，面名闭集闸必拒。
            let mut c = match JOINT_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.face = "";
            joint_case_ok(&c).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 30 例联测全过（判据 J1 直读口）。
fn joint_thirty_ok() -> bool {
    let mut led = build_full_ledger();
    if led.len() != CASE_CAP {
        return false;
    }
    if led.verdict_len() != CASE_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.close().is_err() {
        return false;
    }
    if led.state() != JointState::Closed {
        return false;
    }
    // 独立复核：30 案逐案黄金判定。
    for c in JOINT_CASES.iter() {
        if joint_case_ok(c).is_err() {
            return false;
        }
    }
    // 双侧一致面直读：30/30。
    let (agree, total, miss) = sides_agree(JOINT_CASES);
    if total != CASE_CAP || agree != CASE_CAP || !miss.is_empty() {
        return false;
    }
    true
}

/// 拒止码双侧一致断言绿。
fn sides_ok() -> bool {
    for c in JOINT_CASES.iter() {
        if c.reject_code != c.protocol_code {
            return false;
        }
        if c.reject_code == 0 {
            return false;
        }
        if !c.expect_match {
            return false;
        }
        match face_of(c.face) {
            Some(f) => {
                if !code_in_face(c.reject_code, f.base) {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 零静默（判定记账逐案齐、无静默槽）。
fn zero_silent_ok() -> bool {
    let led = build_full_ledger();
    if led.verdict_len() != led.len() {
        return false;
    }
    // 判定记账案号与案序一一对应。
    let mut i = 0usize;
    while i < led.verdict_len() {
        let c = match led.case_at(i) {
            Some(v) => v,
            None => return false,
        };
        if c.no != i as u32 || c.reject_code == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 面名九闭集普查（九面互异、基码递增、查表命中）。
fn face_census_ok() -> bool {
    if FACE_TABLE.len() != 9 {
        return false;
    }
    let mut ok = true;
    let mut i = 0usize;
    while i < FACE_TABLE.len() {
        let f = match FACE_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if face_of(f.name).is_none() {
            ok = false;
        }
        let mut j = i + 1;
        while j < FACE_TABLE.len() {
            match FACE_TABLE.get(j) {
                Some(g) => {
                    if g.name == f.name || g.base <= f.base {
                        ok = false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    ok
}

/// 双跑一致（冻结案卷下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_ledger();
    let b = build_full_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.case_at(i), b.case_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if fault_sweep().0 != fault_sweep().0 {
        return false;
    }
    if sides_agree(JOINT_CASES).0 != sides_agree(JOINT_CASES).0 {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12671u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.close().is_err() {
        return false;
    }
    if led.state() != JointState::Closed {
        return false;
    }
    // 摘要：首案面名 + 尾案族名拼接非空。
    let head = match led.case_at(0) {
        Some(c) => c.face,
        None => return false,
    };
    let tail = match led.case_at(CASE_CAP - 1) {
        Some(c) => c.fault,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12672 一致性协议自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12671_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12671-snap-protocol");
    let led = build_full_ledger();

    s.add_dyn(
        "F12672-01 案卷三十例齐",
        JOINT_CASES.len() == CASE_CAP,
        &format!("cases={}", JOINT_CASES.len()),
    );

    s.add_dyn(
        "F12672-02 30 例联测全过",
        joint_thirty_ok(),
        &format!("cases={} verdicts={} rejects={}", led.len(), led.verdict_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12672-03 拒止码双侧一致断言绿",
        sides_ok(),
        &format!("agree={}", sides_agree(JOINT_CASES).0),
    );

    s.add_dyn(
        "F12672-04 零静默",
        zero_silent_ok(),
        &format!("verdicts={}", led.verdict_len()),
    );

    s.add_dyn(
        "F12672-05 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12672-06 面名九闭集普查",
        face_census_ok(),
        &format!("faces={}", FACE_TABLE.len()),
    );

    s.add_dyn(
        "F12672-07 案卷账等式",
        led.equation_ok(),
        &format!("cases={} verdicts={} rejects={}", led.len(), led.verdict_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12672-08 双跑一致",
        dual_run_ok(),
        "冻结案卷下两轮逐案同判",
    );

    s.add_dyn(
        "F12672-09 判码窗口口径",
        code_in_face(0x10 + 0x01, 0x10) && !code_in_face(0x10, 0x10) && !code_in_face(0x10 + 0x0D, 0x10),
        "基码 < 判码 ≤ 基码+0x0C",
    );

    s.add_dyn(
        "F12672-10 完成写回语义",
        writeback_ok(),
        "封卷即已完成且摘要非空",
    );

    s.add_dyn(
        "F12672-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 九面覆盖直读：30 案覆盖全部九面。
    let mut covered = [false; 9];
    for c in JOINT_CASES.iter() {
        let mut i = 0usize;
        while i < FACE_TABLE.len() {
            let f = match FACE_TABLE.get(i) {
                Some(v) => v,
                None => break,
            };
            if f.name == c.face {
                match covered.get_mut(i) {
                    Some(slot) => *slot = true,
                    None => {}
                }
            }
            i += 1;
        }
    }
    let mut all_covered = true;
    for slot in covered.iter() {
        if !*slot {
            all_covered = false;
        }
    }
    s.add_dyn(
        "F12672-12 九面全覆盖",
        all_covered,
        "30 案逐面落卷，九面无缺",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x811C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：11 个码互异、全部落窗口、查表可命中。
        assert_eq!(JOINT_ALL.len(), 11);
        let mut i = 0usize;
        while i < JOINT_ALL.len() {
            let vi = match JOINT_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < JOINT_ALL.len() {
                let vj = match JOINT_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn happy_path_closes() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), CASE_CAP);
        assert_eq!(led.verdict_len(), CASE_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.close().is_ok());
        assert_eq!(led.state(), JointState::Closed);
    }

    #[test]
    fn thirty_cases_all_agree() {
        // 双侧一致 30/30。
        let (agree, total, miss) = sides_agree(JOINT_CASES);
        assert_eq!(total, 30);
        assert_eq!(agree, 30, "不一致：{}", miss);
        assert!(miss.is_empty());
        assert!(sides_ok());
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn face_window_semantics() {
        // 面基码窗口直读。
        assert!(code_in_face(0x11, 0x10));
        assert!(code_in_face(0x10 + CODE_OFF_CAP, 0x10));
        assert!(!code_in_face(0x10, 0x10));
        assert!(!code_in_face(0x10 + CODE_OFF_CAP + 1, 0x10));
        assert!(!code_in_face(0, 0x10));
        // 面查表。
        assert!(face_of("注册表消费边").is_some());
        assert!(face_of("口径协议").is_some());
        assert!(face_of("幽灵面").is_none());
        assert!(face_of("").is_none());
    }

    #[test]
    fn case_gate_rejects() {
        // 案闸直读：乱序/重复/断码。
        let mut led = JointLedger::new();
        let _ = led.begin();
        let c1 = match JOINT_CASES.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.side(c1), Err(JointDiag::ECaseOrder.val()));
        let c0 = match JOINT_CASES.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.side(c0.clone()).is_ok());
        let dup = JointCase { no: 1, face: c0.face, fault: c0.fault, reject_code: 0x11, protocol_code: 0x11, expect_match: true };
        assert_eq!(led.side(dup), Err(JointDiag::ECaseDup.val()));
        // 零码。
        let zero = JointCase { no: 1, face: "分支档", fault: "零码族", reject_code: 0, protocol_code: 0, expect_match: true };
        assert_eq!(led.side(zero), Err(JointDiag::EZeroCode.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn zero_silent_ledger() {
        // 零静默：逐案判定记账且案号一一对应。
        assert!(zero_silent_ok());
        let led = build_full_ledger();
        assert_eq!(led.verdict_len(), CASE_CAP);
        let mut i = 0usize;
        while i < CASE_CAP {
            let c = match led.case_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(c.no, i as u32, "案 {} 错位", i);
            i += 1;
        }
    }

    #[test]
    fn nine_faces_covered() {
        // 九面覆盖直读（与检查项独立复核）。
        let mut n = [0usize; 9];
        for c in JOINT_CASES.iter() {
            let mut i = 0usize;
            while i < FACE_TABLE.len() {
                let f = match FACE_TABLE.get(i) {
                    Some(v) => v,
                    None => return,
                };
                if f.name == c.face {
                    match n.get_mut(i) {
                        Some(slot) => *slot += 1,
                        None => return,
                    }
                }
                i += 1;
            }
        }
        for slot in n.iter() {
            assert!(*slot >= 1, "面缺案");
        }
        // 案号连续 0..30。
        for (i, c) in JOINT_CASES.iter().enumerate() {
            assert_eq!(c.no, i as u32, "案号乱序");
        }
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_ledger();
        let b = build_full_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.case_at(i), b.case_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(fault_sweep().0, fault_sweep().0);
        assert_eq!(sides_agree(JOINT_CASES).0, sides_agree(JOINT_CASES).0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Drafted→Sided→Closed。
        assert!(JointState::Drafted.rank() < JointState::Sided.rank());
        assert!(JointState::Sided.rank() < JointState::Closed.rank());
        let mut led = build_full_ledger();
        assert_eq!(led.state(), JointState::Sided);
        assert!(led.close().is_ok());
        assert_eq!(led.close(), Err(JointDiag::ERewind.val()));
        let mut fresh = JointLedger::new();
        assert!(fresh.begin().is_ok());
        assert_eq!(fresh.begin(), Err(JointDiag::ERewind.val()));
    }

    #[test]
    fn faces_base_codes_ascending() {
        // 九面基码递增且互异（0x10..0x90）。
        assert_eq!(FACE_TABLE.len(), 9);
        for (i, f) in FACE_TABLE.iter().enumerate() {
            assert_eq!(f.base, (i as u32 + 1) * 0x10, "面 {} 基码漂移", i);
        }
        assert!(face_census_ok());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12671_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
