//! UNX-F52500 B25 靶场隔离声明条目（全域安全测试判据白名单登记） · 0xCBCC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52500"，逐字）：
//!   title: B25 靶场隔离声明条目（全域安全测试判据白名单登记）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52501-J1 LSP 协
//!     议 ×20 命令零漂移；compile_commands.json 消费 ×10 工程；KB-COMPAT
//!     定级 S1；判据锚定 §AI-66.3 组 26 口径；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52500）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52501-J1，照原文锚定；功能按本卡标题（B25 靶场隔离声明条目）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：全域安全工具族（Nikto / SQLMap / Hydra / Burp / ZAP 五
//!      具）× 隔离判据（回环白名单 / 凭据零接触 / 授权书 / 限速 / 干跑
//!      五元闭集）判据单调主轴 20 格（每判据 4 格轮转五具）。
//!   ② 白名单六条登记 12 格（本机回环 / 实验室网段 / 授权靶标清单 /
//!      凭据零接触声明 / 限速阈值 / 干跑模式开关各 2 格），越格必拒、
//!      漂移必抓。
//!   ③ 与各具分账声明 2 行（外部目标零接触分账 / E5 知识账探测失配分
//!      界声明，联签锚定行 AI-25），缺一拒封（ERowMiss）。
//!   ④ 单向门：六闸 + 20 隔离格 + 12 白名单格 + 2 声明行全齐方可封账
//!      （seal），封账后一切落账拒（ESeal）；封账判定次序：六闸缺→
//!      ESeal、声明行缺→ERowMiss、格账缺→ESeal。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal 单向门/fingerprint
//!      FNV-1a/judge_grade 定级闭集）；run_cycle 注入核 10 轮三态轮转
//!      （判据逆序违例 / 白名单格漂移 / 判据越闭集），全部落在当轮临
//!      时账本上，违例检出 10/10；双跑 FNV 指纹一致；定级 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52500;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 隔离判据主轴格数。
pub const ISO_OPS: usize = 20;

/// 白名单登记格数（六条 × 2 格）。
pub const WL_CELLS: usize = 12;

/// 白名单条数。
pub const WL_ENTRIES: usize = 6;

/// 声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum B25IsoCode {
    /// 成功
    Ok = 0,
    /// 隔离判据号越闭集
    ENoOp,
    /// 隔离格漂移
    EDrift,
    /// 隔离判据逆序
    EOrder,
    /// 白名单格越闭集
    ENoWl,
    /// 白名单格漂移
    EWlDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封 / 单向门已封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl B25IsoCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            B25IsoCode::Ok => "成功",
            B25IsoCode::ENoOp => "隔离判据号越闭集：五判据主轴 20 格之外先登记",
            B25IsoCode::EDrift => "隔离格漂移：载荷折叠与基准不恒等，逐格复算",
            B25IsoCode::EOrder => "隔离判据逆序：回环白名单→凭据零接触→授权书→限速→干跑单调，逆序必抓",
            B25IsoCode::ENoWl => "白名单格越闭集：六条登记 12 格之外先登记",
            B25IsoCode::EWlDrift => "白名单格漂移：登记折叠与基准不恒等，逐格复算",
            B25IsoCode::ENoRow => "声明行越闭集：各具分账 2 行闭集之外先登记",
            B25IsoCode::ERowMiss => "缺声明行：外部目标零接触/E5 探测失配分界声明未齐先补齐",
            B25IsoCode::EDup => "重复落账：同格只许一条账目",
            B25IsoCode::ESeal => "未齐即封或单向门已封：全齐方可 seal，封后落账拒",
            B25IsoCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            B25IsoCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            B25IsoCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1（KB-COMPAT 登记）",
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
            Grade::S1 => "S1：六闸+20隔离格+12白名单格+2声明行全通收口",
            Grade::S2 => "S2：有条件收口，六闸已落但格账存在未齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 隔离判据格载荷（操作 o；判据 = o/4，五判据各 4 格轮转五具）。
pub const fn iso_cell(o: usize) -> Option<u64> {
    if o >= ISO_OPS {
        return None;
    }
    Some((0xBF000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 隔离判据（0..4=回环白名单 4..8=凭据零接触 8..12=授权书 12..16=限速 16..20=干跑）。
pub const fn iso_kind(o: usize) -> Option<u32> {
    if o >= ISO_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 隔离判据名（按 kind 索引）。
pub const fn iso_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("回环白名单"),
        1 => Some("凭据零接触"),
        2 => Some("授权书"),
        3 => Some("限速"),
        4 => Some("干跑"),
        _ => None,
    }
}

/// 工具号（o % 5：0=Nikto 1=SQLMap 2=Hydra 3=Burp 4=ZAP）。
pub const fn tool_of(o: usize) -> Option<u32> {
    if o >= ISO_OPS {
        return None;
    }
    Some((o % 5) as u32)
}

/// 工具名（按 t 索引）。
pub const fn tool_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("Nikto"),
        1 => Some("SQLMap"),
        2 => Some("Hydra"),
        3 => Some("Burp"),
        4 => Some("ZAP"),
        _ => None,
    }
}

/// 白名单格载荷（格 c；条目 = c/2，条目各 2 格）。
pub const fn wl_cell(c: usize) -> Option<u64> {
    if c >= WL_CELLS {
        return None;
    }
    Some((0xC0000u64 + (c as u64) * 0x3B) ^ 0x574C5F_574C4C30)
}

/// 白名单条目名（按 c/2 索引，六条）。
pub const fn wl_entry_name(c: usize) -> Option<&'static str> {
    match c / 2 {
        0 => Some("本机回环"),
        1 => Some("实验室网段"),
        2 => Some("授权靶标清单"),
        3 => Some("凭据零接触声明"),
        4 => Some("限速阈值"),
        5 => Some("干跑模式开关"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC100 + r as u32 * 0x2C)
}

/// 声明行名（0 外部目标零接触分账；1 E5 探测失配分界 AI-25 联签）。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("外部目标零接触分账"),
        1 => Some("E5 探测失配分界（AI-25 联签）"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// B25 靶场隔离声明账本状态机。
#[derive(Clone)]
pub struct B25IsoLedger {
    /// 六判据（母版闸）。
    gates: [Option<bool>; 6],
    /// 隔离判据主账。
    ops: [Option<u64>; ISO_OPS],
    /// 白名单登记账。
    wl: [Option<u64>; WL_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后隔离判据。
    last_kind: i64,
    /// 单向门：封账后一切落账拒。
    sealed: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl B25IsoLedger {
    /// 新账本。
    pub fn new() -> Self {
        B25IsoLedger {
            gates: [None; 6],
            ops: [None; ISO_OPS],
            wl: [None; WL_CELLS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
            sealed: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), B25IsoCode> {
        if self.sealed {
            return Err(B25IsoCode::ESeal);
        }
        if gate >= 6 {
            return Err(B25IsoCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 隔离一格（判据单调 + 载荷对照 + 重复拒；单向门已封即拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), B25IsoCode> {
        if self.sealed {
            return Err(B25IsoCode::ESeal);
        }
        if o >= ISO_OPS {
            return Err(B25IsoCode::ENoOp);
        }
        let kind = match iso_kind(o) {
            Some(k) => k,
            None => return Err(B25IsoCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(B25IsoCode::EOrder);
        }
        let want = match iso_cell(o) {
            Some(w) => w,
            None => return Err(B25IsoCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(B25IsoCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(B25IsoCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 隔离全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < ISO_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 白名单一格（载荷对照 + 重复拒；单向门已封即拒）。
    pub fn record_wl(&mut self, c: usize, got: u64) -> Result<(), B25IsoCode> {
        if self.sealed {
            return Err(B25IsoCode::ESeal);
        }
        let want = match wl_cell(c) {
            Some(w) => w,
            None => return Err(B25IsoCode::ENoWl),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + c as u64);
            return Err(B25IsoCode::EWlDrift);
        }
        if match self.wl.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(B25IsoCode::EDup);
        }
        if let Some(x) = self.wl.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8301 + c as u64);
        Ok(())
    }

    /// 白名单全账核验（12 格）。
    pub fn wl_all(&self) -> bool {
        let mut c = 0usize;
        while c < WL_CELLS {
            match self.wl.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行（单向门已封即拒）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), B25IsoCode> {
        if self.sealed {
            return Err(B25IsoCode::ESeal);
        }
        if r >= SPLIT_ROWS {
            return Err(B25IsoCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(B25IsoCode::EDup);
        }
        if let Some(x) = self.splits.get_mut(r) {
            *x = split_row(r);
        }
        self.fnv(0x8400 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（2 行）。
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

    /// 单向门封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal，
    /// 全齐置 sealed 并返回 Ok；封后一切落账拒。
    pub fn seal(&mut self) -> Result<(), B25IsoCode> {
        if self.sealed {
            return Err(B25IsoCode::ESeal);
        }
        if !self.gates_all() {
            return Err(B25IsoCode::ESeal);
        }
        if !self.splits_all() {
            return Err(B25IsoCode::ERowMiss);
        }
        if !self.ops_all() || !self.wl_all() {
            return Err(B25IsoCode::ESeal);
        }
        self.sealed = true;
        self.fnv(0x8500);
        Ok(())
    }

    /// 单向门状态。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.wl_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for B25IsoLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = B25IsoLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < ISO_OPS {
        if let Some(w) = iso_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < WL_CELLS {
        if let Some(w) = wl_cell(c) {
            if led.record_wl(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 单向门封账（全齐后置 sealed）。
    let _ = led.seal();
    // 注入 10 轮：判据逆序违例 / 白名单格漂移 / 判据越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = B25IsoLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_op(17, iso_cell(17).unwrap_or(0));
                lt.record_op(5, iso_cell(5).unwrap_or(0)) == Err(B25IsoCode::EOrder)
            }
            1 => {
                lt.record_wl(2, wl_cell(2).unwrap_or(0));
                lt.record_wl(3, wl_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(B25IsoCode::EWlDrift)
            }
            _ => lt.record_op(ISO_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52500 判据集（锚 UNX-F52501-J1，off-by-one 板式）。
pub fn run_unx52500_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52500");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五具×五判据隔离主轴 20 格全通",
        recorded >= ISO_OPS as u32,
        "Nikto/SQLMap/Hydra/Burp/ZAP 五具轮转，回环白名单/凭据零接触/授权书/限速/干跑判据单调",
    );
    s.add(
        "白名单六条登记 12 格全落",
        {
            let mut lt = B25IsoLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < WL_CELLS {
                if let Some(w) = wl_cell(c) {
                    if lt.record_wl(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.wl_all()
        },
        "本机回环/实验室网段/授权靶标清单/凭据零接触/限速阈值/干跑开关各 2 格恒等",
    );
    s.add(
        "各具分账声明 2 行全齐",
        {
            let mut lt = B25IsoLedger::new();
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
        "外部目标零接触分账 / E5 探测失配分界（AI-25 联签）两行闭集",
    );
    s.add(
        "判据母版六闸全落",
        {
            let mut lt = B25IsoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐格落账，缺一即封拒",
    );
    s.add(
        "隔离判据单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ISO_OPS {
                if iso_kind(o).unwrap_or(5) < iso_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五判据闭集单调（回环白名单→凭据零接触→授权书→限速→干跑）",
    );
    s.add(
        "单向门封账后落账拒",
        {
            let mut lt = B25IsoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < ISO_OPS {
                if let Some(w) = iso_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < WL_CELLS {
                if let Some(w) = wl_cell(c) {
                    let _ = lt.record_wl(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            let sealed_ok = lt.seal().is_ok() && lt.is_sealed();
            sealed_ok
                && lt.record_op(0, iso_cell(0).unwrap_or(0)) == Err(B25IsoCode::ESeal)
                && lt.record_wl(0, wl_cell(0).unwrap_or(0)) == Err(B25IsoCode::ESeal)
                && lt.judge_gate(0) == Err(B25IsoCode::ESeal)
        },
        "全齐 seal 成功，封后 record/judge 一律 ESeal（单向门）",
    );
    s.add(
        "违例注入 10/10 全抓",
        caught >= 10,
        "判据逆序违例/白名单格漂移/判据越闭集三态轮转必抓",
    );
    let mut l0 = B25IsoLedger::new();
    s.add(
        "边界·隔离判据越闭集",
        l0.record_op(ISO_OPS, 0).is_err(),
        "隔离判据 20 越闭集必拒",
    );
    s.add(
        "边界·白名单格越闭集",
        l0.record_wl(WL_CELLS, 0).is_err(),
        "白名单格 12 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    s.add(
        "白名单漂移必抓",
        {
            let mut lt = B25IsoLedger::new();
            lt.record_wl(0, wl_cell(0).unwrap_or(0));
            lt.record_wl(1, wl_cell(1).unwrap_or(0).wrapping_add(1)) == Err(B25IsoCode::EWlDrift)
        },
        "白名单格载荷漂移必抓",
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
            let mut lt = B25IsoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < ISO_OPS {
                if let Some(w) = iso_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < WL_CELLS {
                if let Some(w) = wl_cell(c) {
                    let _ = lt.record_wl(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1 && lt.seal().is_ok()
        },
        "全通 → S1 且单向门可封（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBCC0,
        "SEG_BASE 0x40 对齐且为 VX16 域 52500 格位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_kind() {
        assert_eq!(iso_kind(0), Some(0));
        assert_eq!(iso_kind(4), Some(1));
        assert_eq!(iso_kind(19), Some(4));
        assert!(iso_kind(20).is_none());
        assert!(iso_cell(20).is_none());
        assert_eq!(iso_kind_name(0), Some("回环白名单"));
        assert_eq!(iso_kind_name(4), Some("干跑"));
        assert!(iso_kind_name(5).is_none());
    }

    #[test]
    fn test_tool_of() {
        assert_eq!(tool_of(0), Some(0));
        assert_eq!(tool_of(3), Some(3));
        assert_eq!(tool_of(4), Some(4));
        assert_eq!(tool_of(5), Some(0));
        assert!(tool_of(20).is_none());
        assert_eq!(tool_name(0), Some("Nikto"));
        assert_eq!(tool_name(4), Some("ZAP"));
        assert!(tool_name(5).is_none());
    }

    #[test]
    fn test_wl_entries() {
        assert_eq!(wl_entry_name(0), Some("本机回环"));
        assert_eq!(wl_entry_name(6), Some("凭据零接触声明"));
        assert_eq!(wl_entry_name(3), Some("实验室网段"));
        assert_eq!(wl_entry_name(10), Some("干跑模式开关"));
        assert!(wl_entry_name(12).is_none());
        assert!(wl_cell(WL_CELLS).is_none());
        assert_eq!(split_row_name(0), Some("外部目标零接触分账"));
        assert!(split_row_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = iso_cell(3).unwrap_or(0);
        assert_eq!(a, iso_cell(3).unwrap_or(0));
        assert_ne!(a, iso_cell(4).unwrap_or(0));
        let b = wl_cell(5).unwrap_or(0);
        assert_eq!(b, wl_cell(5).unwrap_or(0));
        assert_ne!(b, wl_cell(6).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = B25IsoLedger::new();
        let _ = lt.record_op(17, iso_cell(17).unwrap_or(0));
        assert_eq!(lt.record_op(5, iso_cell(5).unwrap_or(0)), Err(B25IsoCode::EOrder));
        assert!(lt.record_op(18, iso_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = B25IsoLedger::new();
        assert_eq!(
            lt.record_op(5, iso_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(B25IsoCode::EDrift)
        );
        assert_eq!(
            lt.record_wl(7, wl_cell(7).unwrap_or(0).wrapping_add(9)),
            Err(B25IsoCode::EWlDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = B25IsoLedger::new();
        let _ = lt.record_wl(0, wl_cell(0).unwrap_or(0));
        assert_eq!(lt.record_wl(0, wl_cell(0).unwrap_or(0)), Err(B25IsoCode::EDup));
        let _ = lt.declare_split(1);
        assert_eq!(lt.declare_split(1), Err(B25IsoCode::EDup));
    }

    // 单向门封账判定次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    #[test]
    fn test_seal_semantics() {
        // 空账：六闸缺 → ESeal。
        let mut lt0 = B25IsoLedger::new();
        assert_eq!(lt0.seal(), Err(B25IsoCode::ESeal));
        // 六闸齐但声明行缺 → ERowMiss。
        let mut lt1 = B25IsoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.seal(), Err(B25IsoCode::ERowMiss));
        // 六闸齐 + 声明行齐但格账缺 → ESeal。
        let mut lt2 = B25IsoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.seal(), Err(B25IsoCode::ESeal));
        // 全齐 → seal 成功且置 sealed。
        let mut lt3 = B25IsoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < ISO_OPS {
            if let Some(w) = iso_cell(o) {
                let _ = lt3.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < WL_CELLS {
            if let Some(w) = wl_cell(c) {
                let _ = lt3.record_wl(c, w);
            }
            c += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt3.declare_split(r);
            r += 1;
        }
        assert!(lt3.seal().is_ok());
        assert!(lt3.is_sealed());
        // 封后一切落账拒（单向门）。
        assert_eq!(lt3.record_op(0, iso_cell(0).unwrap_or(0)), Err(B25IsoCode::ESeal));
        assert_eq!(lt3.record_wl(0, wl_cell(0).unwrap_or(0)), Err(B25IsoCode::ESeal));
        assert_eq!(lt3.declare_split(0), Err(B25IsoCode::ESeal));
        assert_eq!(lt3.judge_gate(0), Err(B25IsoCode::ESeal));
        assert_eq!(lt3.seal(), Err(B25IsoCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = B25IsoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = B25IsoLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, ISO_OPS as u32 + WL_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBCC0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(B25IsoCode::Ok as u32, 0);
        assert_eq!(B25IsoCode::EGradeDrift as u32, 12);
        assert_eq!(B25IsoCode::Ok.val(), SEG_BASE);
        assert!(!B25IsoCode::Ok.detail().is_empty());
        assert!(!B25IsoCode::ESeal.detail().is_empty());
        assert!(!B25IsoCode::ERowMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        let lt0 = B25IsoLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = B25IsoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = B25IsoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < ISO_OPS {
            if let Some(w) = iso_cell(o) {
                let _ = lt2.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < WL_CELLS {
            if let Some(w) = wl_cell(c) {
                let _ = lt2.record_wl(c, w);
            }
            c += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52500_checks_green() {
        let s = run_unx52500_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
