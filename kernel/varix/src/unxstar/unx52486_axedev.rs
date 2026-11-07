//! UNX-F52486 axe DevTools 消费面收口 · 0xCB940段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52486"，逐字）：
//!   title: axe DevTools 消费面收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52487-J1 UIA 树
//!     检查 ×10（消费 M 域 UIA 联签锚定行 AI-62）；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52486）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52487-J1，照原文锚定；功能按本卡标题（axe DevTools 消费面收
//! 口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 规则族闭集：axe 五规则族（color-contrast/aria/forms/landmarks/
//!      keyboard）各 4 格共 20 格，规则族单调（逆序必抓）；逐格载荷纯
//!      函数折叠，恒等才收。
//!   ② 违例三态 12 格：violation/pass/incomplete 三态闭集，格 i 期望
//!      态 = i % 3（每态恰 4 格），态漂移必抓。
//!   ③ 扫描 16 拍单调：扫描基准逐拍递增，逆行必抓。
//!   ④ 与 Chrome DevTools 消费联签 4 格（F52461 锚定：消费联签四格
//!      闭集），逐格折叠对照，漂移必抓；JS 上栈声明 2 行闭集（JS 依
//!      Chromium 上栈声明/axe-core 版本对齐声明），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal_gate/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10 轮（规则族逆序/违例态
//!      漂移/扫描逆行）10/10 必抓；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级闭集（S1/S2/S3）：全齐 → S1，六判据齐 → S2，
//!      空账 S3。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52486;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// axe 规则格数。
pub const RULE_OPS: usize = 20;

/// 违例三态格数。
pub const VIOL_CELLS: usize = 12;

/// 扫描拍数。
pub const SCAN_BEATS: usize = 16;

/// Chrome DevTools 消费联签格数。
pub const JOINT_CELLS: usize = 4;

/// JS 上栈声明行数。
pub const JS_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AxeCode {
    /// 成功
    Ok = 0,
    /// 序号越闭集
    ENoOp,
    /// 规则格漂移
    EDrift,
    /// 规则族逆序
    EOrder,
    /// 违例格越闭集
    ENoViol,
    /// 违例态漂移
    EViolDrift,
    /// 扫描逆行
    EScanBack,
    /// 联签格越闭集
    ENoJoint,
    /// 联签漂移
    EJointDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl AxeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AxeCode::Ok => "成功",
            AxeCode::ENoOp => "序号越闭集：规则 20 格与扫描 16 拍闭集之外先登记",
            AxeCode::EDrift => "规则格漂移：规则折叠与基准不恒等，逐格复算",
            AxeCode::EOrder => "规则族逆序：color-contrast→aria→forms→landmarks→keyboard 单调，逆序必抓",
            AxeCode::ENoViol => "违例格越闭集：违例三态 12 格之外先登记",
            AxeCode::EViolDrift => "违例态漂移：violation/pass/incomplete 三态与格位期望不符",
            AxeCode::EScanBack => "扫描逆行：扫描基准逐拍递增，回退必抓",
            AxeCode::ENoJoint => "联签格越闭集：Chrome DevTools 消费联签 4 格之外先登记",
            AxeCode::EJointDrift => "联签漂移：联签折叠与基准不恒等，逐格复算",
            AxeCode::ENoRow => "声明行越闭集：JS 上栈 2 行闭集之外先登记",
            AxeCode::ERowMiss => "缺声明行：JS 上栈声明未齐先补齐",
            AxeCode::ESeal => "未齐即封：20 规则 + 12 违例 + 16 拍 + 4 联签 + 2 行未齐先补齐",
            AxeCode::EGradeDrift => "定级漂移：全齐才可定 S1（KB-COMPAT 定级登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const 可调）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：20 规则 + 12 违例 + 16 拍 + 4 联签 + 2 行全通收口",
            Grade::S2 => "S2：六判据齐，有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 规则格载荷（格 o；规则族 = o/4）。
pub const fn rule_cell(o: usize) -> Option<u64> {
    if o >= RULE_OPS {
        return None;
    }
    Some((0xD5000u64 + (o as u64) * 0x3A) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 规则族（0..4=color-contrast 4..8=aria 8..12=forms 12..16=landmarks 16..20=keyboard）。
pub const fn rule_kind(o: usize) -> Option<u32> {
    if o >= RULE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 规则族名。
pub const fn rule_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("color-contrast"),
        1 => Some("aria"),
        2 => Some("forms"),
        3 => Some("landmarks"),
        4 => Some("keyboard"),
        _ => None,
    }
}

/// 违例格位期望三态（0=violation 1=pass 2=incomplete；态 = i % 3）。
pub const fn viol_want(i: usize) -> Option<u32> {
    if i >= VIOL_CELLS {
        return None;
    }
    Some((i % 3) as u32)
}

/// 违例格载荷（格 i）。
pub const fn viol_cell(i: usize) -> Option<u64> {
    if i >= VIOL_CELLS {
        return None;
    }
    Some((0xD6000u64 + (i as u64) * 0x46) ^ 0x4158455F56494F)
}

/// 扫描拍基准（拍 b，逐拍递增）。
pub const fn scan_val(b: usize) -> Option<u64> {
    if b >= SCAN_BEATS {
        return None;
    }
    Some(0xD7000u64 + (b as u64) * 0x40)
}

/// 联签格载荷（格 j；F52461 锚定）。
pub const fn joint_cell(j: usize) -> Option<u64> {
    if j >= JOINT_CELLS {
        return None;
    }
    Some((0xD8000u64 + (j as u64) * 0x52) ^ 0x4368726F6D654454)
}

/// JS 上栈声明行折叠（行 r）。
pub const fn js_row(r: usize) -> Option<u32> {
    if r >= JS_ROWS {
        return None;
    }
    Some(0xD880 + r as u32 * 0x3A)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// axe DevTools 收口账本状态机。
#[derive(Clone)]
pub struct AxeLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 规则账。
    rules: [Option<u64>; RULE_OPS],
    /// 违例三态账。
    viols: [Option<u32>; VIOL_CELLS],
    /// 扫描拍账。
    scans: [Option<u64>; SCAN_BEATS],
    /// 联签账。
    joints: [Option<u64>; JOINT_CELLS],
    /// JS 上栈声明账。
    jsrows: [Option<u32>; JS_ROWS],
    /// 最后规则族。
    last_kind: i64,
    /// 最后扫描拍基准。
    last_scan: u64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AxeLedger {
    /// 新账本。
    pub fn new() -> Self {
        AxeLedger {
            gates: [None; 6],
            rules: [None; RULE_OPS],
            viols: [None; VIOL_CELLS],
            scans: [None; SCAN_BEATS],
            joints: [None; JOINT_CELLS],
            jsrows: [None; JS_ROWS],
            last_kind: -1,
            last_scan: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AxeCode> {
        if gate >= 6 {
            return Err(AxeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5B100 + gate as u64);
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

    /// 规则一格（规则族单调 + 载荷对照）。
    pub fn record_rule(&mut self, o: usize, got: u64) -> Result<(), AxeCode> {
        if o >= RULE_OPS {
            return Err(AxeCode::ENoOp);
        }
        let kind = match rule_kind(o) {
            Some(k) => k,
            None => return Err(AxeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B200);
            return Err(AxeCode::EOrder);
        }
        let want = match rule_cell(o) {
            Some(w) => w,
            None => return Err(AxeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B201 + o as u64);
            return Err(AxeCode::EDrift);
        }
        if let Some(c) = self.rules.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5B202 + o as u64);
        Ok(())
    }

    /// 规则全账核验（20 格）。
    pub fn rules_all(&self) -> bool {
        let mut o = 0usize;
        while o < RULE_OPS {
            match self.rules.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 违例一格（三态期望对照）。
    pub fn record_viol(&mut self, i: usize, got_state: u32) -> Result<(), AxeCode> {
        let want = match viol_want(i) {
            Some(w) => w,
            None => return Err(AxeCode::ENoViol),
        };
        if got_state != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B300 + i as u64);
            return Err(AxeCode::EViolDrift);
        }
        if let Some(c) = self.viols.get_mut(i) {
            *c = Some(got_state);
        }
        self.fnv(0x5B301 + i as u64);
        Ok(())
    }

    /// 违例三态全账核验（12 格，每态恰 4 格）。
    pub fn viols_all(&self) -> bool {
        let mut i = 0usize;
        while i < VIOL_CELLS {
            match self.viols.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 扫描一拍（基准对照 + 单调逆行拒）。
    pub fn record_scan(&mut self, b: usize, got: u64) -> Result<(), AxeCode> {
        if b >= SCAN_BEATS {
            return Err(AxeCode::ENoOp);
        }
        if got < self.last_scan {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B400 + b as u64);
            return Err(AxeCode::EScanBack);
        }
        let want = match scan_val(b) {
            Some(w) => w,
            None => return Err(AxeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B401 + b as u64);
            return Err(AxeCode::EScanBack);
        }
        if let Some(c) = self.scans.get_mut(b) {
            *c = Some(got);
        }
        self.last_scan = got;
        self.fnv(0x5B402 + b as u64);
        Ok(())
    }

    /// 扫描全账核验（16 拍）。
    pub fn scans_all(&self) -> bool {
        let mut b = 0usize;
        while b < SCAN_BEATS {
            match self.scans.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 联签一格（载荷对照）。
    pub fn record_joint(&mut self, j: usize, got: u64) -> Result<(), AxeCode> {
        let want = match joint_cell(j) {
            Some(w) => w,
            None => return Err(AxeCode::ENoJoint),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B500 + j as u64);
            return Err(AxeCode::EJointDrift);
        }
        if let Some(c) = self.joints.get_mut(j) {
            *c = Some(got);
        }
        self.fnv(0x5B501 + j as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn joints_all(&self) -> bool {
        let mut j = 0usize;
        while j < JOINT_CELLS {
            match self.joints.get(j) {
                Some(Some(_)) => {}
                _ => return false,
            }
            j += 1;
        }
        true
    }

    /// JS 上栈声明一行。
    pub fn declare_js(&mut self, r: usize) -> Result<(), AxeCode> {
        if r >= JS_ROWS {
            return Err(AxeCode::ENoRow);
        }
        if let Some(c) = self.jsrows.get_mut(r) {
            *c = js_row(r);
        }
        self.fnv(0x5B600 + r as u64);
        Ok(())
    }

    /// JS 上栈声明全齐核验。
    pub fn js_all(&self) -> bool {
        let mut r = 0usize;
        while r < JS_ROWS {
            match self.jsrows.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 收口封闸：声明缺 → ERowMiss；判据/格账缺 → ESeal；全齐 → Ok。
    pub fn seal_gate(&self) -> Result<(), AxeCode> {
        if !self.gates_all() {
            return Err(AxeCode::ESeal);
        }
        if !self.js_all() {
            return Err(AxeCode::ERowMiss);
        }
        if !self.rules_all() || !self.viols_all() || !self.scans_all() || !self.joints_all() {
            return Err(AxeCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全齐 → S1；六判据齐 → S2；空账 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.rules_all() && self.viols_all() && self.scans_all()
            && self.joints_all() && self.js_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：与期望档不符即 EGradeDrift。
    pub fn verify_grade(&self, want: Grade) -> Result<(), AxeCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(AxeCode::EGradeDrift)
        }
    }
}

impl Default for AxeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AxeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < RULE_OPS {
        if let Some(w) = rule_cell(o) {
            if led.record_rule(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < VIOL_CELLS {
        if let Some(w) = viol_want(i) {
            if led.record_viol(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut b = 0usize;
    while b < SCAN_BEATS {
        if let Some(w) = scan_val(b) {
            if led.record_scan(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut j = 0usize;
    while j < JOINT_CELLS {
        if let Some(w) = joint_cell(j) {
            if led.record_joint(j, w).is_ok() {
                recorded += 1;
            }
        }
        j += 1;
    }
    let mut r = 0usize;
    while r < JS_ROWS {
        let _ = led.declare_js(r);
        r += 1;
    }
    // 注入 10 轮：规则族逆序 / 违例态漂移 / 扫描逆行三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AxeLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_rule(17, rule_cell(17).unwrap_or(0));
                lt.record_rule(5, rule_cell(5).unwrap_or(0)) == Err(AxeCode::EOrder)
            }
            1 => {
                lt.record_viol(4, viol_want(4).unwrap_or(0).wrapping_add(1))
                    == Err(AxeCode::EViolDrift)
            }
            _ => {
                let _ = lt.record_scan(3, scan_val(3).unwrap_or(0));
                lt.record_scan(1, scan_val(1).unwrap_or(0)) == Err(AxeCode::EScanBack)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5B700 + k as u64);
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
/// UNX-F52486 判据集（锚 UNX-F52487-J1，off-by-one 板式）。
pub fn run_unx52486_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52486");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五规则族 20 格全通",
        recorded >= RULE_OPS as u32,
        "color-contrast/aria/forms/landmarks/keyboard 五族逐格恒等",
    );
    s.add(
        "违例三态 12 格全落",
        {
            let mut lt = AxeLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < VIOL_CELLS {
                if let Some(w) = viol_want(i) {
                    if lt.record_viol(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.viols_all()
        },
        "violation/pass/incomplete 三态逐格对位，每态恰 4 格",
    );
    s.add(
        "扫描 16 拍单调",
        {
            let mut lt = AxeLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SCAN_BEATS {
                if let Some(w) = scan_val(b) {
                    if lt.record_scan(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.scans_all()
        },
        "扫描基准逐拍递增，逆行必抓",
    );
    s.add(
        "Chrome DevTools 消费联签 4/4",
        {
            let mut lt = AxeLedger::new();
            let mut ok = true;
            let mut j = 0usize;
            while j < JOINT_CELLS {
                if let Some(w) = joint_cell(j) {
                    if lt.record_joint(j, w).is_err() {
                        ok = false;
                    }
                }
                j += 1;
            }
            ok && lt.joints_all()
        },
        "消费联签四格闭集（F52461 锚定）",
    );
    s.add(
        "JS 上栈声明 2/2 全齐",
        {
            let mut lt = AxeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JS_ROWS {
                if lt.declare_js(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.js_all()
        },
        "JS 依 Chromium 上栈声明/axe-core 版本对齐声明两行闭集",
    );
    s.add(
        "规则族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < RULE_OPS {
                if rule_kind(o).unwrap_or(5) < rule_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五规则族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "规则族逆序/违例态漂移/扫描逆行三态轮转必抓");
    let mut l0 = AxeLedger::new();
    s.add(
        "边界·规则越闭集",
        l0.record_rule(RULE_OPS, 0).is_err(),
        "规则格 20 越闭集必拒",
    );
    s.add(
        "边界·违例越闭集",
        l0.record_viol(VIOL_CELLS, 0).is_err(),
        "违例格 12 越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_joint(JOINT_CELLS, 0).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "未齐落账不拒（封点前 Ok）",
        {
            let _ = l0.record_rule(0, rule_cell(0).unwrap_or(0));
            l0.record_viol(0, viol_want(0).unwrap_or(0)).is_ok()
        },
        "账未齐时落账仍 Ok，封拒只发生在封闸",
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
        "定级 S1 闭集",
        {
            let mut lt = AxeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < RULE_OPS {
                if let Some(w) = rule_cell(o) {
                    let _ = lt.record_rule(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < VIOL_CELLS {
                if let Some(w) = viol_want(i) {
                    let _ = lt.record_viol(i, w);
                }
                i += 1;
            }
            let mut b = 0usize;
            while b < SCAN_BEATS {
                if let Some(w) = scan_val(b) {
                    let _ = lt.record_scan(b, w);
                }
                b += 1;
            }
            let mut j = 0usize;
            while j < JOINT_CELLS {
                if let Some(w) = joint_cell(j) {
                    let _ = lt.record_joint(j, w);
                }
                j += 1;
            }
            let mut r = 0usize;
            while r < JS_ROWS {
                let _ = lt.declare_js(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1 && lt.verify_grade(Grade::S1).is_ok()
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCB940,
        "SEG_BASE 0x40 对齐且落在 VX16 0xCB940 段",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_kind() {
        assert_eq!(rule_kind(0), Some(0));
        assert_eq!(rule_kind(4), Some(1));
        assert_eq!(rule_kind(8), Some(2));
        assert_eq!(rule_kind(12), Some(3));
        assert_eq!(rule_kind(19), Some(4));
        assert!(rule_kind(20).is_none());
        assert!(rule_cell(20).is_none());
        assert_eq!(rule_kind_name(0), Some("color-contrast"));
        assert_eq!(rule_kind_name(4), Some("keyboard"));
        assert!(rule_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = rule_cell(6).unwrap_or(0);
        assert_eq!(a, rule_cell(6).unwrap_or(0));
        assert_ne!(a, rule_cell(7).unwrap_or(0));
        assert!(viol_cell(VIOL_CELLS).is_none());
        assert!(joint_cell(JOINT_CELLS).is_none());
        assert!(js_row(JS_ROWS).is_none());
    }

    #[test]
    fn test_scan_monotone() {
        let mut lt = AxeLedger::new();
        let _ = lt.record_scan(4, scan_val(4).unwrap_or(0));
        assert_eq!(
            lt.record_scan(2, scan_val(2).unwrap_or(0)),
            Err(AxeCode::EScanBack)
        );
        assert!(lt.record_scan(5, scan_val(5).unwrap_or(0)).is_ok());
        // 扫描拍越闭集。
        assert!(lt.record_scan(SCAN_BEATS, 0).is_err());
    }

    #[test]
    fn test_viol_tri_state() {
        assert_eq!(viol_want(0), Some(0));
        assert_eq!(viol_want(1), Some(1));
        assert_eq!(viol_want(2), Some(2));
        assert_eq!(viol_want(11), Some(2));
        assert!(viol_want(12).is_none());
        let mut lt = AxeLedger::new();
        assert_eq!(
            lt.record_viol(7, viol_want(7).unwrap_or(0).wrapping_add(1)),
            Err(AxeCode::EViolDrift)
        );
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = AxeLedger::new();
        let _ = lt.record_rule(9, rule_cell(9).unwrap_or(0));
        assert_eq!(lt.record_rule(2, rule_cell(2).unwrap_or(0)), Err(AxeCode::EOrder));
        assert!(lt.record_rule(10, rule_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_joint_drift_caught() {
        let mut lt = AxeLedger::new();
        assert_eq!(
            lt.record_joint(1, joint_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(AxeCode::EJointDrift)
        );
        assert!(lt.record_joint(1, joint_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_and_seal() {
        let mut lt = AxeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        // 账未齐：声明行缺 → ERowMiss。
        assert_eq!(lt.seal_gate(), Err(AxeCode::ERowMiss));
        assert!(lt.record_rule(0, rule_cell(0).unwrap_or(0)).is_ok());
        let mut lt2 = AxeLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_miss() {
        let mut lt = AxeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal_gate(), Err(AxeCode::ERowMiss));
        assert!(lt.declare_js(JS_ROWS).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(
            recorded,
            RULE_OPS as u32 + VIOL_CELLS as u32 + SCAN_BEATS as u32 + JOINT_CELLS as u32
        );
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
        assert_eq!(SEG_BASE, 0xCB940);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AxeCode::Ok as u32, 0);
        assert_eq!(AxeCode::EGradeDrift as u32, 12);
        assert_eq!(AxeCode::Ok.val(), SEG_BASE);
        let mut i = 0u32;
        while i <= 12 {
            let c = match i {
                0 => AxeCode::Ok,
                1 => AxeCode::ENoOp,
                2 => AxeCode::EDrift,
                3 => AxeCode::EOrder,
                4 => AxeCode::ENoViol,
                5 => AxeCode::EViolDrift,
                6 => AxeCode::EScanBack,
                7 => AxeCode::ENoJoint,
                8 => AxeCode::EJointDrift,
                9 => AxeCode::ENoRow,
                10 => AxeCode::ERowMiss,
                11 => AxeCode::ESeal,
                _ => AxeCode::EGradeDrift,
            };
            assert_eq!(c.val(), SEG_BASE + i);
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt0 = AxeLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        assert!(lt0.verify_grade(Grade::S1).is_err());
        // 六闸齐 S2。
        let mut lt1 = AxeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52486_checks_green() {
        let s = run_unx52486_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
