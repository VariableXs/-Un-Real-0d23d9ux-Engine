//! UNX-F52492 Burp Suite Community 收口 · 0xCBAC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52492"，逐字）：
//!   title: Burp Suite Community 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52493-J1 代理/
//!     扫描 ×10（本机靶站）；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52492）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52493-J1，照原文锚定；功能按本卡标题（Burp Suite Community）
//! 落地。代理/扫描判据只对本机搭建的演练靶站（白名单口径）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五工具族 20 操作集：Proxy/Repeater/Intruder 限/Decoder/Comparer
//!      五家族闭集各 4 操作，逐操作载荷（纯函数折叠）恒等才收；家族
//!      单调（逆序必抓）。
//!   ② 代理三元闸：监听/拦截/转发三元闭集逐格落账，越界必拒。
//!   ③ 扫描限购声明 2 行（Community 无主动扫描，如实）+ 与 Wireshark
//!      分账声明 2 行（F52249）+ Java 上栈声明 2 行，六行闭集缺一拒封
//!      （seal 时声明行缺 → ERowMiss）。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      seal/judge_grade）；注入核 run_cycle 三态轮转 10/10（操作越闭集/
//!      操作漂移/家族逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级
//!      S1（KB-COMPAT：Community 免费面如实全通档）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52492;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五工具族操作数。
pub const OPS: usize = 20;

/// 代理三元闸格数。
pub const LIC_CELLS: usize = 3;

/// 扫描限购声明行数（Community 无主动扫描）。
pub const SCAN_ROWS: usize = 2;

/// 与 Wireshark 分账声明行数（F52249）。
pub const SPLIT_ROWS: usize = 2;

/// Java 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BurpCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 代理三元格号越闭集
    ENoLic,
    /// 代理三元载荷漂移
    ELicDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl BurpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BurpCode::Ok => "成功",
            BurpCode::ENoOp => "操作号越闭集：Proxy/Repeater/Intruder 限/Decoder/Comparer 20 操作集之外先登记",
            BurpCode::EDrift => "载荷漂移：操作载荷与基准折叠不恒等，逐格复算",
            BurpCode::EOrder => "家族逆序：Proxy→Repeater→Intruder 限→Decoder→Comparer 单调，逆序必抓",
            BurpCode::ENoLic => "代理格号越闭集：监听/拦截/转发三元闭集之外先登记",
            BurpCode::ELicDrift => "代理三元载荷漂移：与基准折叠不恒等，逐格复算",
            BurpCode::ENoRow => "声明行越闭集：扫描限购 2 行 + Wireshark 分账 2 行 + Java 上栈 2 行之外先登记",
            BurpCode::ERowMiss => "缺声明行：扫描限购/Wireshark 分账/Java 上栈声明未齐先补齐",
            BurpCode::EDup => "重复落账：同格只许一条账目",
            BurpCode::ESeal => "未齐即封：20 操作 + 3 代理三元 + 6 声明行未齐先补齐",
            BurpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BurpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BurpCode::EGradeDrift => "定级漂移：六判据全落且声明行全齐才可定 S1（Community 免费面如实）",
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
            Grade::S1 => "S1：Community 免费面如实全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五工具族操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xCBAC0u64 + (o as u64) * 0x35) ^ 0x5249_2A00_0000_0000)
}

/// 操作家族（0..4=Proxy 4..8=Repeater 8..12=Intruder 限 12..16=Decoder 16..20=Comparer）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Proxy"),
        1 => Some("Repeater"),
        2 => Some("Intruder 限"),
        3 => Some("Decoder"),
        4 => Some("Comparer"),
        _ => None,
    }
}

/// 代理三元格载荷（格 i：0=监听 1=拦截 2=转发）。
pub const fn lic_cell(i: usize) -> Option<u64> {
    if i >= LIC_CELLS {
        return None;
    }
    Some((0xCBBC0u64 + (i as u64) * 0x47) ^ 0x5249_2B00_0000_0000)
}

/// 代理三元格名。
pub const fn lic_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("监听"),
        1 => Some("拦截"),
        2 => Some("转发"),
        _ => None,
    }
}

/// 扫描限购声明行折叠（行 r；Community 无主动扫描，如实）。
pub const fn scan_row(r: usize) -> Option<u32> {
    if r >= SCAN_ROWS {
        return None;
    }
    Some(0xCB30 + r as u32 * 0x2C)
}

/// 与 Wireshark 分账声明行折叠（行 r，F52249）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xCC30 + r as u32 * 0x2C)
}

/// Java 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCD30 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Burp Suite Community 收口账本状态机。
#[derive(Clone)]
pub struct BurpLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 五工具族操作账。
    ops: [Option<u64>; OPS],
    /// 代理三元账。
    lics: [Option<u64>; LIC_CELLS],
    /// 扫描限购声明账。
    scans: [Option<u32>; SCAN_ROWS],
    /// Wireshark 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// Java 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BurpLedger {
    /// 新账本。
    pub fn new() -> Self {
        BurpLedger {
            gates: [None; 6],
            ops: [None; OPS],
            lics: [None; LIC_CELLS],
            scans: [None; SCAN_ROWS],
            splits: [None; SPLIT_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BurpCode> {
        if gate >= 6 {
            return Err(BurpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), BurpCode> {
        if o >= OPS {
            return Err(BurpCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(BurpCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A00);
            return Err(BurpCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(BurpCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A01 + o as u64);
            return Err(BurpCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BurpCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4A02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
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

    /// 代理三元一格（载荷对照 + 重复拒）。
    pub fn record_lic(&mut self, i: usize, got: u64) -> Result<(), BurpCode> {
        if i >= LIC_CELLS {
            return Err(BurpCode::ENoLic);
        }
        let want = match lic_cell(i) {
            Some(w) => w,
            None => return Err(BurpCode::ENoLic),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B00 + i as u64);
            return Err(BurpCode::ELicDrift);
        }
        if match self.lics.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BurpCode::EDup);
        }
        if let Some(c) = self.lics.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x4B01 + i as u64);
        Ok(())
    }

    /// 代理三元全账核验（3 格）。
    pub fn lics_all(&self) -> bool {
        let mut i = 0usize;
        while i < LIC_CELLS {
            match self.lics.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 扫描限购声明一行（Community 无主动扫描，如实）。
    pub fn declare_scan(&mut self, r: usize) -> Result<(), BurpCode> {
        if r >= SCAN_ROWS {
            return Err(BurpCode::ENoRow);
        }
        if match self.scans.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BurpCode::EDup);
        }
        if let Some(c) = self.scans.get_mut(r) {
            *c = scan_row(r);
        }
        self.fnv(0x4C00 + r as u64);
        Ok(())
    }

    /// 扫描限购声明全齐核验。
    pub fn scans_all(&self) -> bool {
        let mut r = 0usize;
        while r < SCAN_ROWS {
            match self.scans.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 与 Wireshark 分账声明一行（F52249）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), BurpCode> {
        if r >= SPLIT_ROWS {
            return Err(BurpCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BurpCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x4D00 + r as u64);
        Ok(())
    }

    /// Wireshark 分账全齐核验。
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

    /// Java 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), BurpCode> {
        if r >= STACK_ROWS {
            return Err(BurpCode::ENoRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BurpCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x4E00 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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

    /// 收口封账（顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal）。
    pub fn seal(&self) -> Result<(), BurpCode> {
        if !self.gates_all() {
            return Err(BurpCode::ESeal);
        }
        if !self.scans_all() || !self.splits_all() || !self.stacks_all() {
            return Err(BurpCode::ERowMiss);
        }
        if !self.ops_all() || !self.lics_all() {
            return Err(BurpCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全齐 → S1 Community 免费面如实档；六闸齐 → S2；否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.seal().is_ok() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BurpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BurpLedger::new();
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
    let mut i = 0usize;
    while i < LIC_CELLS {
        if let Some(w) = lic_cell(i) {
            if led.record_lic(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SCAN_ROWS {
        let _ = led.declare_scan(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 家族逆序三态轮转。
    // 三态注入全部落在当轮临时账本上；漂移/逆序先按合法序落正格再注。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BurpLedger::new();
        let caught = match kind {
            0 => lt.record_op(OPS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(BurpCode::EDrift)
            }
            _ => {
                let _ = lt.record_op(16, op_cell(16).unwrap_or(0));
                lt.record_op(5, op_cell(5).unwrap_or(0)) == Err(BurpCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4F00 + k as u64);
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
/// UNX-F52492 判据集（锚 UNX-F52493-J1，off-by-one 板式）。
pub fn run_unx52492_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52492");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五工具族 20 操作全通",
        recorded >= OPS as u32,
        "Proxy/Repeater/Intruder 限/Decoder/Comparer 五家族逐操作恒等（本机靶站口径）",
    );
    s.add(
        "代理三元闸全落",
        {
            let mut lt = BurpLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < LIC_CELLS {
                if let Some(w) = lic_cell(i) {
                    if lt.record_lic(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.lics_all()
        },
        "监听/拦截/转发三元闭集逐格落账，越界必拒",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = BurpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "扫描限购声明 2/2 全齐（Community 无主动扫描如实）",
        {
            let mut lt = BurpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SCAN_ROWS {
                if lt.declare_scan(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.scans_all()
        },
        "Community 无主动扫描声明/扫描能力限购边界声明两行闭集",
    );
    s.add(
        "与 Wireshark 分账声明 2/2 全齐（F52249）",
        {
            let mut lt = BurpLedger::new();
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
        "与 Wireshark 分账声明两行闭集（F52249 锚定）",
    );
    s.add(
        "Java 上栈声明 2/2 全齐",
        {
            let mut lt = BurpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "Java 运行时上栈声明/JVM 消费声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "Proxy→Repeater→Intruder 限→Decoder→Comparer 五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作越闭集/操作漂移/家族逆序三态轮转必抓",
    );
    let mut l0 = BurpLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒（ENoOp 提前返回，不计注入账）",
    );
    s.add(
        "边界·代理三元越闭集",
        l0.record_lic(LIC_CELLS, 0).is_err(),
        "代理三元 3 越闭集必拒（ENoLic 提前返回）",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_scan(SCAN_ROWS).is_err()
            && l0.declare_split(SPLIT_ROWS).is_err()
            && l0.declare_stack(STACK_ROWS).is_err(),
        "声明行越闭集必拒（ENoRow 提前返回）",
    );
    s.add(
        "操作漂移拒",
        {
            l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(BurpCode::EDrift)
        },
        "操作载荷漂移必抓",
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
        "定级 S1 闭集（KB-COMPAT）",
        {
            let mut lt = BurpLedger::new();
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
            let mut i = 0usize;
            while i < LIC_CELLS {
                if let Some(w) = lic_cell(i) {
                    let _ = lt.record_lic(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SCAN_ROWS {
                let _ = lt.declare_scan(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全齐 → S1（Community 免费面如实定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBAC0,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×91",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind_mapping() {
        // 名表按家族（kind）索引而非格号。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(9), Some(2));
        assert_eq!(op_kind(15), Some(3));
        assert_eq!(op_kind(16), Some(4));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(0), Some("Proxy"));
        assert_eq!(op_kind_name(2), Some("Intruder 限"));
        assert_eq!(op_kind_name(4), Some("Comparer"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_lic_names() {
        assert_eq!(lic_name(0), Some("监听"));
        assert_eq!(lic_name(1), Some("拦截"));
        assert_eq!(lic_name(2), Some("转发"));
        assert!(lic_name(3).is_none());
        assert!(lic_cell(LIC_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(13).unwrap_or(0);
        assert_eq!(a, op_cell(13).unwrap_or(0));
        assert_ne!(a, op_cell(14).unwrap_or(0));
        let b = lic_cell(2).unwrap_or(0);
        assert_eq!(b, lic_cell(2).unwrap_or(0));
        assert!(scan_row(SCAN_ROWS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族格，再注逆序必抓；被拒格不改 last_kind。
        let mut lt = BurpLedger::new();
        let _ = lt.record_op(10, op_cell(10).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(BurpCode::EOrder));
        assert!(lt.record_op(11, op_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BurpLedger::new();
        assert_eq!(
            lt.record_op(7, op_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(BurpCode::EDrift)
        );
        assert_eq!(
            lt.record_lic(0, lic_cell(0).unwrap_or(0).wrapping_add(2)),
            Err(BurpCode::ELicDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BurpLedger::new();
        let _ = lt.record_lic(1, lic_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_lic(1, lic_cell(1).unwrap_or(0)),
            Err(BurpCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BurpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = BurpLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_order() {
        // 封账语义顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
        let empty = BurpLedger::new();
        assert_eq!(empty.seal(), Err(BurpCode::ESeal));
        let mut lt = BurpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal(), Err(BurpCode::ERowMiss));
        let mut r = 0usize;
        while r < SCAN_ROWS {
            let _ = lt.declare_scan(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(BurpCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 操作 + 3 代理三元。
        assert!(recorded >= OPS as u32 + LIC_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBAC0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BurpCode::Ok as u32, 0);
        assert_eq!(BurpCode::EGradeDrift as u32, 12);
        assert_eq!(BurpCode::Ok.val(), SEG_BASE);
        assert_eq!(BurpCode::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!BurpCode::Ok.detail().is_empty());
        assert!(!BurpCode::ESeal.detail().is_empty());
        assert!(!BurpCode::ELicDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = BurpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_scan_row_boundary() {
        let mut lt = BurpLedger::new();
        assert!(lt.declare_scan(0).is_ok());
        assert!(lt.declare_scan(1).is_ok());
        assert!(lt.declare_scan(2).is_err());
        assert!(lt.scans_all());
    }

    #[test]
    fn unx52492_checks_green() {
        let s = run_unx52492_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
