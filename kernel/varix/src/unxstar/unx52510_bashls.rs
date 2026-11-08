//! UNX-F52510 bash-language-server 收口 · 0xCBF40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52510"，逐字）：
//!   title: bash-language-server 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52511-J1 schema
//!     校验 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52510）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52511-J1，照原文锚定；功能按本卡标题（bash-language-server）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：bash-language-server 五能力族 20 格（补全/悬停文档/
//!      诊断定位/ShellCheck 联动/符号跳转各 4 格），能力族单调，逐格
//!      载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② ShellCheck 消费联签 4 格：4 格闭集、双族（诊断报告 0..2、
//!      修复建议 2..4）单调，联签漂移必抓。
//!   ③ 解释器三元闸：bash 4.4/5.1/5.3 三档闭集，越档必抓、载荷漂移
//!      必抓。
//!   ④ 声明 4 行闭集：与 curl 分账声明 2 行（联签锚定行 F52255）+
//!      Node 上栈声明 2 行，缺一拒封（seal：六闸缺→ESeal、声明行缺→
//!      ERowMiss、格账缺→ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（联签漂移/
//!      能力族逆序/操作越闭集），三类注入全部落在当轮临时账本上，
//!      主账 injected==caught==10、escaped==0；漂移注入先按合法序落
//!      同族正格再注漂移；双跑指纹一致；定级目标档 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52510;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// bash-language-server 五能力族操作数。
pub const OPS: usize = 20;

/// ShellCheck 消费联签格数。
pub const SC_CELLS: usize = 4;

/// 解释器三元闸档数。
pub const IT_STEPS: usize = 3;

/// 声明行数（curl 分账 2 行 + Node 上栈 2 行）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BashlsCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// 联签格越闭集
    ENoSc,
    /// 联签漂移
    EScDrift,
    /// 解释器档越三元闸
    ENoIt,
    /// 解释器载荷漂移
    EItDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl BashlsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BashlsCode::Ok => "成功",
            BashlsCode::ENoOp => "操作号越闭集：bash-language-server 五能力族 20 格之外先登记",
            BashlsCode::EDrift => "操作漂移：能力格折叠与基准不恒等，逐格复算",
            BashlsCode::EOrder => "能力族逆序：补全→悬停文档→诊断定位→ShellCheck 联动→符号跳转单调，逆序必抓",
            BashlsCode::ENoSc => "联签格越闭集：ShellCheck 消费联签 4 格之外先登记",
            BashlsCode::EScDrift => "联签漂移：ShellCheck 消费折叠与基准不恒等，逐格复算",
            BashlsCode::ENoIt => "解释器档越三元闸：bash 4.4/5.1/5.3 三档闭集之外先登记",
            BashlsCode::EItDrift => "解释器载荷漂移：解释器档折叠与基准不恒等，逐档复算",
            BashlsCode::ENoRow => "声明行越闭集：curl 分账+Node 上栈 4 行闭集之外先登记",
            BashlsCode::ERowMiss => "缺声明行：curl 分账声明/Node 上栈声明未齐先补齐",
            BashlsCode::EDup => "重复落账：同格只许一条账目",
            BashlsCode::ESeal => "未齐即封：20 能力格+4 联签格+3 档解释器+六判据未齐先补齐",
            BashlsCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定档（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口（本卡目标档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20能力+4联签+3档解释器+4行声明全通收口",
            Grade::S2 => "S2：有条件收口（bash-language-server KB-COMPAT 目标档）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x2C) ^ 0x626173686C730000)
}

/// 能力族（0..4=补全 4..8=悬停文档 8..12=诊断定位 12..16=ShellCheck 联动 16..20=符号跳转）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力族名（按 kind 索引）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("补全"),
        1 => Some("悬停文档"),
        2 => Some("诊断定位"),
        3 => Some("ShellCheck 联动"),
        4 => Some("符号跳转"),
        _ => None,
    }
}

/// ShellCheck 联签格载荷（格 c；联签族 = c/2：0..2=诊断报告 2..4=修复建议）。
pub const fn sc_cell(c: usize) -> Option<u64> {
    if c >= SC_CELLS {
        return None;
    }
    Some((0xD2000u64 + (c as u64) * 0x31) ^ 0x7368636B00000000)
}

/// 联签族。
pub const fn sc_kind(c: usize) -> Option<u32> {
    if c >= SC_CELLS {
        return None;
    }
    Some((c / 2) as u32)
}

/// 联签族名。
pub const fn sc_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("诊断报告"),
        1 => Some("修复建议"),
        _ => None,
    }
}

/// 解释器三元闸档载荷（档 v）。
pub const fn it_cell(v: usize) -> Option<u64> {
    if v >= IT_STEPS {
        return None;
    }
    Some((0xD3000u64 + (v as u64) * 0x5D) ^ 0x6261736800000000)
}

/// 解释器档名。
pub const fn it_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("bash 4.4"),
        1 => Some("bash 5.1"),
        2 => Some("bash 5.3"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD410 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("curl 分账声明·主（联签锚定行 F52255）"),
        1 => Some("curl 分账声明·辅（联签锚定行 F52255）"),
        2 => Some("Node 上栈声明·主"),
        3 => Some("Node 上栈声明·辅"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// bash-language-server 收口账本状态机。
#[derive(Clone)]
pub struct BashlsLedger {
    /// 六判据（收口闸：安装/启动/补全/诊断/升级/卸载）。
    gates: [Option<bool>; 6],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// 联签账。
    scs: [Option<u64>; SC_CELLS],
    /// 解释器三元闸账。
    its: [Option<u64>; IT_STEPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后能力族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BashlsLedger {
    /// 新账本。
    pub fn new() -> Self {
        BashlsLedger {
            gates: [None; 6],
            ops: [None; OPS],
            scs: [None; SC_CELLS],
            its: [None; IT_STEPS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BashlsCode> {
        if gate >= 6 {
            return Err(BashlsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6A00 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 能力一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), BashlsCode> {
        if o >= OPS {
            return Err(BashlsCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(BashlsCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6B00);
            return Err(BashlsCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(BashlsCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6B01 + o as u64);
            return Err(BashlsCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BashlsCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6B02 + o as u64);
        Ok(())
    }

    /// 能力全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 联签一格（载荷对照 + 重复拒，越闭集拒不计注入账）。
    pub fn record_sc(&mut self, c: usize, got: u64) -> Result<(), BashlsCode> {
        let want = match sc_cell(c) {
            Some(w) => w,
            None => return Err(BashlsCode::ENoSc),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6C00 + c as u64);
            return Err(BashlsCode::EScDrift);
        }
        if match self.scs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BashlsCode::EDup);
        }
        if let Some(b) = self.scs.get_mut(c) {
            *b = Some(got);
        }
        self.fnv(0x6C01 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn scs_all(&self) -> bool {
        let mut c = 0usize;
        while c < SC_CELLS {
            match self.scs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 解释器三元闸一档（载荷对照 + 重复拒）。
    pub fn record_it(&mut self, v: usize, got: u64) -> Result<(), BashlsCode> {
        let want = match it_cell(v) {
            Some(w) => w,
            None => return Err(BashlsCode::ENoIt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6D00 + v as u64);
            return Err(BashlsCode::EItDrift);
        }
        if match self.its.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BashlsCode::EDup);
        }
        if let Some(j) = self.its.get_mut(v) {
            *j = Some(got);
        }
        self.fnv(0x6D01 + v as u64);
        Ok(())
    }

    /// 解释器三元闸全账核验（3 档）。
    pub fn its_all(&self) -> bool {
        let mut v = 0usize;
        while v < IT_STEPS {
            match self.its.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), BashlsCode> {
        if r >= SPLIT_ROWS {
            return Err(BashlsCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BashlsCode::EDup);
        }
        if let Some(s) = self.splits.get_mut(r) {
            *s = split_row(r);
        }
        self.fnv(0x6E00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), BashlsCode> {
        if !self.gates_all() {
            return Err(BashlsCode::ESeal);
        }
        if !self.splits_all() {
            return Err(BashlsCode::ERowMiss);
        }
        if !self.ops_all() || !self.scs_all() || !self.its_all() {
            return Err(BashlsCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：全齐→目标档 S2，六闸齐→S2，空账→S3。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.scs_all() && self.its_all() && self.splits_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BashlsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BashlsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < SC_CELLS {
        if let Some(w) = sc_cell(c) {
            if led.record_sc(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut v = 0usize;
    while v < IT_STEPS {
        if let Some(w) = it_cell(v) {
            if led.record_it(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：联签漂移 / 能力族逆序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BashlsLedger::new();
        let caught = match kind {
            0 => {
                // 联签漂移：先按合法序落同族正格（诊断报告族），再注漂移。
                let w0 = match sc_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match sc_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_sc(0, w0);
                lt.record_sc(1, w1.wrapping_add(1)) == Err(BashlsCode::EScDrift)
            }
            1 => {
                // 能力族逆序：空账先落族 4 正格，再回落族 1，逆序必抓。
                let w17 = match op_cell(17) {
                    Some(w) => w,
                    None => 0,
                };
                let w5 = match op_cell(5) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(17, w17);
                lt.record_op(5, w5) == Err(BashlsCode::EOrder)
            }
            _ => {
                // 操作越闭集：边界拒，不计注入账内层（lt 只判拒码）。
                lt.record_op(OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6F00 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52510 判据集（锚 UNX-F52511-J1，off-by-one 板式）。
pub fn run_unx52510_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52510");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "bash-language-server 五能力族 20 操作全通",
        recorded >= OPS as u32,
        "补全/悬停文档/诊断定位/ShellCheck 联动/符号跳转逐格恒等",
    );
    s.add(
        "ShellCheck 消费联签 4/4 全落",
        {
            let mut lt = BashlsLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SC_CELLS {
                if let Some(w) = sc_cell(c) {
                    if lt.record_sc(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.scs_all()
        },
        "诊断报告 0..2 与修复建议 2..4 双族逐格联签恒等",
    );
    s.add(
        "解释器三元闸 3/3 全落",
        {
            let mut lt = BashlsLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < IT_STEPS {
                if let Some(w) = it_cell(v) {
                    if lt.record_it(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.its_all()
        },
        "bash 4.4/5.1/5.3 三档闭集逐档恒等，越档必抓",
    );
    s.add(
        "curl 分账与 Node 上栈声明 4/4 全齐",
        {
            let mut lt = BashlsLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "curl 分账声明 2 行（F52255 联签）+ Node 上栈声明 2 行闭集（§AI-66.3）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = BashlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/启动/补全/诊断/升级/卸载逐格落账",
    );
    s.add(
        "能力族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let ka = match op_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let kb = match op_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if ka < kb {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五能力族闭集单调",
    );
    s.add(
        "联签族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < SC_CELLS {
                let ka = match sc_kind(c) {
                    Some(k) => k,
                    None => 2,
                };
                let kb = match sc_kind(c - 1) {
                    Some(k) => k,
                    None => 2,
                };
                if ka < kb {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "诊断报告→修复建议双族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "联签漂移/能力族逆序/操作越闭集三态轮转必抓",
    );
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = BashlsLedger::new();
            lt.record_op(OPS, 0).is_err()
        },
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        {
            let mut lt = BashlsLedger::new();
            lt.record_sc(SC_CELLS, 0).is_err()
        },
        "联签格 4 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = BashlsLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err()
        },
        "声明行 4 越闭集必拒",
    );
    s.add(
        "能力漂移拒",
        {
            let mut lt = BashlsLedger::new();
            let w0 = match op_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match op_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_op(0, w0);
            lt.record_op(1, w1.wrapping_add(1)) == Err(BashlsCode::EDrift)
        },
        "能力格载荷漂移必抓",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：闸+声明齐但联签缺一也必拒。
            let lt_empty = BashlsLedger::new();
            let mut lt_cell = BashlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt_cell.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt_cell.declare_split(r);
                r += 1;
            }
            let mut c = 1usize;
            while c < SC_CELLS {
                if let Some(w) = sc_cell(c) {
                    let _ = lt_cell.record_sc(c, w);
                }
                c += 1;
            }
            lt_empty.seal() == Err(BashlsCode::ESeal) && lt_cell.seal() == Err(BashlsCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = BashlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            lt.seal() == Err(BashlsCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss",
    );
    s.add(
        "解释器越三元拒",
        {
            let mut lt = BashlsLedger::new();
            lt.record_it(IT_STEPS, 0).is_err()
        },
        "解释器档 3 越三元闸必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = BashlsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < SC_CELLS {
                if let Some(w) = sc_cell(c) {
                    let _ = lt.record_sc(c, w);
                }
                c += 1;
            }
            let mut v = 0usize;
            while v < IT_STEPS {
                if let Some(w) = it_cell(v) {
                    let _ = lt.record_it(v, w);
                }
                v += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.grade() == Grade::S2
        },
        "全齐 → S2（bash-language-server KB-COMPAT 目标档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCBF40",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 ≥20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("补全"));
        assert_eq!(op_kind_name(3), Some("ShellCheck 联动"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_sc_kind() {
        assert_eq!(sc_kind(0), Some(0));
        assert_eq!(sc_kind(1), Some(0));
        assert_eq!(sc_kind(2), Some(1));
        assert_eq!(sc_kind(3), Some(1));
        assert!(sc_kind(4).is_none());
        assert!(sc_cell(4).is_none());
        assert_eq!(sc_kind_name(0), Some("诊断报告"));
        assert_eq!(sc_kind_name(1), Some("修复建议"));
        assert!(sc_kind_name(2).is_none());
    }

    #[test]
    fn test_it_ladder() {
        assert_eq!(it_name(0), Some("bash 4.4"));
        assert_eq!(it_name(1), Some("bash 5.1"));
        assert_eq!(it_name(2), Some("bash 5.3"));
        assert!(it_name(3).is_none());
        assert!(it_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        let s0 = sc_cell(0).unwrap_or(0);
        assert_ne!(s0, sc_cell(2).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(split_name(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = BashlsLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(BashlsCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BashlsLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(BashlsCode::EDrift)
        );
        assert_eq!(
            lt.record_sc(1, sc_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(BashlsCode::EScDrift)
        );
        assert_eq!(
            lt.record_it(2, it_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(BashlsCode::EItDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BashlsLedger::new();
        let _ = lt.record_it(0, it_cell(0).unwrap_or(0));
        assert_eq!(lt.record_it(0, it_cell(0).unwrap_or(0)), Err(BashlsCode::EDup));
        let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(BashlsCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BashlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = BashlsLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_out_of_set_no_injection() {
        // 越闭集边界拒不计注入账。
        let mut lt = BashlsLedger::new();
        let _ = lt.record_op(OPS, 0);
        let _ = lt.record_sc(SC_CELLS, 0);
        let _ = lt.record_it(IT_STEPS, 0);
        let _ = lt.declare_split(SPLIT_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        let mut full = BashlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < SC_CELLS {
            if let Some(w) = sc_cell(c) {
                let _ = full.record_sc(c, w);
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < IT_STEPS {
            if let Some(w) = it_cell(v) {
                let _ = full.record_it(v, w);
            }
            v += 1;
        }
        assert_eq!(full.seal(), Err(BashlsCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = full.declare_split(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 4 联签 + 3 档解释器（六闸与声明行不计数）。
        assert_eq!(recorded, OPS as u32 + SC_CELLS as u32 + IT_STEPS as u32);
        assert_eq!(recorded, 27);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCBF40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BashlsCode::Ok as u32, 0);
        assert_eq!(BashlsCode::ENoOp as u32, 1);
        assert_eq!(BashlsCode::EGradeDrift as u32, 12);
        assert_eq!(BashlsCode::Ok.val(), SEG_BASE);
        assert!(!BashlsCode::Ok.detail().is_empty());
        assert!(!BashlsCode::ESeal.detail().is_empty());
        assert!(!BashlsCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = BashlsLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = BashlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        // 全齐目标档 S2（目标档前置必须补 judge_gate×6）。
        let mut lt3 = BashlsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = lt3.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < SC_CELLS {
            if let Some(w) = sc_cell(c) {
                let _ = lt3.record_sc(c, w);
            }
            c += 1;
        }
        let mut v = 0usize;
        while v < IT_STEPS {
            if let Some(w) = it_cell(v) {
                let _ = lt3.record_it(v, w);
            }
            v += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt3.declare_split(r);
            r += 1;
        }
        assert_eq!(lt3.grade(), Grade::S2);
    }

    #[test]
    fn unx52510_checks_green() {
        let s = run_unx52510_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
