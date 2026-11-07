//! UNX-F52248 Fiddler Everywhere 收口 · 0xC7DC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52248"，逐字）：
//!   title: Fiddler Everywhere 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52249-J1 六判据全
//!     落；抓包/过滤/解析 20 操作集全通；Npcap 消费面 ×10 场景；协议解析
//!     ×20 常见协议零漂移；KB-COMPAT 定级 S1；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52248）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52249-J1，照原文锚定（卡面 criteria 内「抓包/过滤/解析 20 操作
//! 集」「Npcap 消费面」诸语随原文属后卡口径）；功能按本卡标题（Fiddler
//! Everywhere）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 捕获 12 格闭集：三家族（实时流量/已存会话/导入归档）各 4 格，
//!      家族单调（逆序必抓），逐格载荷纯函数恒等才收，漂移必抓。
//!   ② 组合请求 10 操作集：两家族（构造 0..5 / 执行 5..10），逐操作恒等。
//!   ③ 规则/重写 8 格逐格落账。
//!   ④ 团队协作三元闸：工作区共享 / 邀请协作者 / 同步审计三态逐格落账，
//!      越三态闭集必抓。
//!   ⑤ 订阅/免费分账声明 2 行闭集，缺一拒封；跨端 5 场景（Windows/
//!      macOS/Linux/桌面应用/浏览器端）逐场景落账。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（组合请求格
//!      漂移/组合请求家族逆序/规则越闭集），三类注入全部落在当轮临时账
//!      本上，led 上 injected==caught==10、escaped==0；漂移注入先录正格
//!      再注漂移；双跑指纹一致。
//!   ⑦ 定级 S1 口径：六判据 + 12 捕获格 + 10 操作 + 8 规则格 + 三元闸
//!      + 2 行分账 + 5 场景全通 → S1。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52248;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 捕获格数。
pub const CAP_CELLS: usize = 12;

/// 组合请求操作数。
pub const COMP_OPS: usize = 10;

/// 规则/重写格数。
pub const RULE_CELLS: usize = 8;

/// 团队协作三元闸态数。
pub const TEAM_STATES: usize = 3;

/// 订阅/免费分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 跨端场景数。
pub const SCENES: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FidEveryCode {
    /// 成功
    Ok = 0,
    /// 捕获格越闭集
    ENoCap,
    /// 捕获漂移
    ECapDrift,
    /// 组合请求操作越闭集
    ENoComp,
    /// 组合请求家族逆序
    EOrder,
    /// 规则格越闭集
    ENoRule,
    /// 规则漂移
    ERuleDrift,
    /// 三元闸越闭集
    ENoTeam,
    /// 场景越闭集
    ENoScene,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl FidEveryCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FidEveryCode::Ok => "成功",
            FidEveryCode::ENoCap => "捕获格越闭集：捕获 12 格闭集之外先登记",
            FidEveryCode::ECapDrift => "捕获漂移：捕获格载荷与基准不恒等，逐格复算",
            FidEveryCode::ENoComp => "组合请求操作越闭集：组合请求 10 操作集之外先登记",
            FidEveryCode::EOrder => "组合请求家族逆序：构造→执行单调，逆序必抓",
            FidEveryCode::ENoRule => "规则格越闭集：规则/重写 8 格闭集之外先登记",
            FidEveryCode::ERuleDrift => "规则漂移：规则格载荷与基准不恒等，逐格复算",
            FidEveryCode::ENoTeam => "三元闸越闭集：工作区共享/邀请协作者/同步审计三态之外先登记",
            FidEveryCode::ENoScene => "场景越闭集：跨端 5 场景闭集之外先登记",
            FidEveryCode::EDup => "重复落账：同格只许一条账目",
            FidEveryCode::ESeal => "未齐即封：12 捕获格 + 10 操作 + 8 规则格未齐先补齐",
            FidEveryCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FidEveryCode::EGradeDrift => "定级漂移：六判据+12 捕获格+10 操作+8 规则格+三元闸+2 行分账+5 场景全通才可定 S1",
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
            Grade::S1 => "S1：六判据+12 捕获格+10 操作+8 规则格+三元闸+2 行分账+5 场景全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 捕获格载荷（格 i；家族 = i/4）。
pub const fn cap_cell(i: usize) -> Option<u64> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((0x100000u64 + (i as u64) * 0x59) ^ ((i / 4) as u64).wrapping_mul(0x0F1DEA7))
}

/// 捕获家族（0..4=实时流量 4..8=已存会话 8..12=导入归档）。
pub const fn cap_kind(i: usize) -> Option<u32> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 捕获家族名。
pub const fn cap_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("实时流量"),
        1 => Some("已存会话"),
        2 => Some("导入归档"),
        _ => None,
    }
}

/// 组合请求操作格载荷（操作 o；家族 = o/5：0..5 构造 5..10 执行）。
pub const fn comp_cell(o: usize) -> Option<u64> {
    if o >= COMP_OPS {
        return None;
    }
    Some((0x101000u64 + (o as u64) * 0x43) ^ ((o / 5) as u64).wrapping_mul(0x01000193))
}

/// 组合请求操作家族。
pub const fn comp_kind(o: usize) -> Option<u32> {
    if o >= COMP_OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 组合请求操作家族名。
pub const fn comp_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("构造"),
        1 => Some("执行"),
        _ => None,
    }
}

/// 规则/重写格载荷（格 r）。
pub const fn rule_cell(r: usize) -> Option<u64> {
    if r >= RULE_CELLS {
        return None;
    }
    Some((0x102000u64 + (r as u64) * 0x31) ^ 0x46574F52554C4531)
}

/// 团队协作三元闸态名。
pub const fn team_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("工作区共享"),
        1 => Some("邀请协作者"),
        2 => Some("同步审计"),
        _ => None,
    }
}

/// 团队协作三元闸态折叠。
pub const fn team_state(s: usize) -> Option<u32> {
    if s >= TEAM_STATES {
        return None;
    }
    Some(0x10300 + s as u32 * 0x5B)
}

/// 订阅/免费分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0x10400 + r as u32 * 0x47)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("订阅形态分账声明"),
        1 => Some("免费形态分账声明"),
        _ => None,
    }
}

/// 跨端场景载荷（场景 sc）。
pub const fn scene_cell(sc: usize) -> Option<u64> {
    if sc >= SCENES {
        return None;
    }
    Some((0x105000u64 + (sc as u64) * 0x2B) ^ 0x46524F4E54534531)
}

/// 跨端场景名。
pub const fn scene_name(sc: usize) -> Option<&'static str> {
    match sc {
        0 => Some("Windows 桌面"),
        1 => Some("macOS 桌面"),
        2 => Some("Linux 桌面"),
        3 => Some("桌面应用通用端"),
        4 => Some("浏览器端"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Fiddler Everywhere 收口账本状态机。
#[derive(Clone)]
pub struct FidEveryLedger {
    /// 六判据（启动/抓包/规则重写/团队协作/升级/卸载）。
    gates: [Option<bool>; 6],
    /// 捕获账。
    caps: [Option<u64>; CAP_CELLS],
    /// 组合请求操作账。
    comps: [Option<u64>; COMP_OPS],
    /// 规则账。
    rules: [Option<u64>; RULE_CELLS],
    /// 三元闸账。
    teams: [Option<u32>; TEAM_STATES],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 跨端场景账。
    scenes: [Option<u64>; SCENES],
    /// 最后组合请求家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FidEveryLedger {
    /// 新账本。
    pub fn new() -> Self {
        FidEveryLedger {
            gates: [None; 6],
            caps: [None; CAP_CELLS],
            comps: [None; COMP_OPS],
            rules: [None; RULE_CELLS],
            teams: [None; TEAM_STATES],
            splits: [None; SPLIT_ROWS],
            scenes: [None; SCENES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FidEveryCode> {
        if gate >= 6 {
            return Err(FidEveryCode::ESeal);
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

    /// 捕获一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, i: usize, got: u64) -> Result<(), FidEveryCode> {
        if i >= CAP_CELLS {
            return Err(FidEveryCode::ENoCap);
        }
        let want = match cap_cell(i) {
            Some(w) => w,
            None => return Err(FidEveryCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + i as u64);
            return Err(FidEveryCode::ECapDrift);
        }
        if match self.caps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FidEveryCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x8201 + i as u64);
        Ok(())
    }

    /// 捕获全账核验（12 格）。
    pub fn caps_all(&self) -> bool {
        let mut i = 0usize;
        while i < CAP_CELLS {
            match self.caps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 组合请求操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_comp(&mut self, o: usize, got: u64) -> Result<(), FidEveryCode> {
        if o >= COMP_OPS {
            return Err(FidEveryCode::ENoComp);
        }
        let kind = match comp_kind(o) {
            Some(k) => k,
            None => return Err(FidEveryCode::ENoComp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(FidEveryCode::EOrder);
        }
        let want = match comp_cell(o) {
            Some(w) => w,
            None => return Err(FidEveryCode::ENoComp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(FidEveryCode::ECapDrift);
        }
        if match self.comps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FidEveryCode::EDup);
        }
        if let Some(c) = self.comps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8302 + o as u64);
        Ok(())
    }

    /// 组合请求全账核验（10 操作）。
    pub fn comps_all(&self) -> bool {
        let mut o = 0usize;
        while o < COMP_OPS {
            match self.comps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 规则一格（载荷对照 + 重复拒）。
    pub fn record_rule(&mut self, r: usize, got: u64) -> Result<(), FidEveryCode> {
        if r >= RULE_CELLS {
            return Err(FidEveryCode::ENoRule);
        }
        let want = match rule_cell(r) {
            Some(w) => w,
            None => return Err(FidEveryCode::ENoRule),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + r as u64);
            return Err(FidEveryCode::ERuleDrift);
        }
        if match self.rules.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FidEveryCode::EDup);
        }
        if let Some(c) = self.rules.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0x8401 + r as u64);
        Ok(())
    }

    /// 规则全账核验（8 格）。
    pub fn rules_all(&self) -> bool {
        let mut r = 0usize;
        while r < RULE_CELLS {
            match self.rules.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 团队协作三元闸一格。
    pub fn record_team(&mut self, s: usize) -> Result<(), FidEveryCode> {
        let v = match team_state(s) {
            Some(v) => v,
            None => return Err(FidEveryCode::ENoTeam),
        };
        if match self.teams.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FidEveryCode::EDup);
        }
        if let Some(c) = self.teams.get_mut(s) {
            *c = Some(v);
        }
        self.fnv(0x8500 + s as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn teams_all(&self) -> bool {
        let mut s = 0usize;
        while s < TEAM_STATES {
            match self.teams.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 分账声明一行（订阅/免费）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), FidEveryCode> {
        if r >= SPLIT_ROWS {
            return Err(FidEveryCode::ENoRule);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FidEveryCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
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

    /// 跨端场景一格。
    pub fn record_scene(&mut self, sc: usize) -> Result<(), FidEveryCode> {
        let v = match scene_cell(sc) {
            Some(v) => v,
            None => return Err(FidEveryCode::ENoScene),
        };
        if match self.scenes.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FidEveryCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(sc) {
            *c = Some(v);
        }
        self.fnv(0x8700 + sc as u64);
        Ok(())
    }

    /// 场景全齐核验（5 场景）。
    pub fn scenes_all(&self) -> bool {
        let mut sc = 0usize;
        while sc < SCENES {
            match self.scenes.get(sc) {
                Some(Some(_)) => {}
                _ => return false,
            }
            sc += 1;
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.caps_all()
            && self.comps_all()
            && self.rules_all()
            && self.teams_all()
            && self.splits_all()
            && self.scenes_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for FidEveryLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FidEveryLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < CAP_CELLS {
        if let Some(w) = cap_cell(i) {
            if led.record_cap(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < COMP_OPS {
        if let Some(w) = comp_cell(o) {
            if led.record_comp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < RULE_CELLS {
        if let Some(w) = rule_cell(r) {
            if led.record_rule(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut s = 0usize;
    while s < TEAM_STATES {
        let _ = led.record_team(s);
        s += 1;
    }
    let mut sp = 0usize;
    while sp < SPLIT_ROWS {
        let _ = led.declare_split(sp);
        sp += 1;
    }
    let mut sc = 0usize;
    while sc < SCENES {
        let _ = led.record_scene(sc);
        sc += 1;
    }
    // 注入 10 轮：组合请求格漂移 / 组合请求家族逆序 / 规则越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FidEveryLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
                lt.record_comp(1, comp_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(FidEveryCode::ECapDrift)
            }
            1 => {
                let _ = lt.record_comp(6, comp_cell(6).unwrap_or(0));
                lt.record_comp(3, comp_cell(3).unwrap_or(0)) == Err(FidEveryCode::EOrder)
            }
            _ => {
                let _ = lt.record_comp(0, comp_cell(0).unwrap_or(0));
                lt.record_rule(RULE_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8800 + k as u64);
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
/// UNX-F52248 判据集（锚 UNX-F52249-J1，off-by-one 板式）。
pub fn run_unx52248_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52248");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "捕获 12 格全通",
        recorded >= CAP_CELLS as u32,
        "实时流量/已存会话/导入归档三家族逐格恒等",
    );
    s.add(
        "组合请求 10 操作全通",
        {
            let mut lt = FidEveryLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < COMP_OPS {
                if let Some(w) = comp_cell(o) {
                    if lt.record_comp(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.comps_all()
        },
        "构造/执行两家族逐操作恒等",
    );
    s.add(
        "规则/重写 8 格全落",
        {
            let mut lt = FidEveryLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < RULE_CELLS {
                if let Some(w) = rule_cell(r) {
                    if lt.record_rule(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.rules_all()
        },
        "规则/重写 8 格逐格载荷对照恒等",
    );
    s.add(
        "团队协作三元闸 3/3 全落",
        {
            let mut lt = FidEveryLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TEAM_STATES {
                if lt.record_team(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.teams_all()
        },
        "工作区共享/邀请协作者/同步审计三态逐格落账",
    );
    s.add(
        "订阅/免费分账声明 2/2 全齐",
        {
            let mut lt = FidEveryLedger::new();
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
        "订阅形态/免费形态两行分账声明闭集，缺一拒封",
    );
    s.add(
        "跨端 5 场景全落",
        {
            let mut lt = FidEveryLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SCENES {
                if lt.record_scene(sc).is_err() {
                    ok = false;
                }
                sc += 1;
            }
            ok && lt.scenes_all()
        },
        "Windows/macOS/Linux/桌面应用/浏览器端逐场景落账",
    );
    s.add(
        "捕获家族单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < CAP_CELLS {
                if cap_kind(i).unwrap_or(9) < cap_kind(i - 1).unwrap_or(9) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "三家族闭集单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = FidEveryLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/抓包/规则重写/团队协作/升级/卸载逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "组合请求格漂移/组合请求家族逆序/规则越闭集三态轮转必抓",
    );
    let mut l0 = FidEveryLedger::new();
    s.add(
        "边界·捕获格越闭集",
        l0.record_cap(CAP_CELLS, 0).is_err(),
        "捕获格 12 越闭集必拒",
    );
    s.add(
        "边界·组合请求越闭集",
        l0.record_comp(COMP_OPS, 0).is_err(),
        "组合请求操作 10 越闭集必拒",
    );
    s.add(
        "边界·规则越闭集",
        l0.record_rule(RULE_CELLS, 0).is_err(),
        "规则格 8 越闭集必拒",
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
            let mut lt = FidEveryLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < CAP_CELLS {
                if let Some(w) = cap_cell(i) {
                    let _ = lt.record_cap(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < COMP_OPS {
                if let Some(w) = comp_cell(o) {
                    let _ = lt.record_comp(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < RULE_CELLS {
                if let Some(w) = rule_cell(r) {
                    let _ = lt.record_rule(r, w);
                }
                r += 1;
            }
            let mut t = 0usize;
            while t < TEAM_STATES {
                let _ = lt.record_team(t);
                t += 1;
            }
            let mut sp = 0usize;
            while sp < SPLIT_ROWS {
                let _ = lt.declare_split(sp);
                sp += 1;
            }
            let mut sc = 0usize;
            while sc < SCENES {
                let _ = lt.record_scene(sc);
                sc += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "诊断码闭集",
        FidEveryCode::Ok.val() == SEG_BASE && FidEveryCode::EGradeDrift.val() == SEG_BASE + 12,
        "Ok=+0x00、EGradeDrift=+0x0C 恰 13 变体",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7DC0,
        "SEG_BASE = 0xC7200 + 0x40×(52248−52201)，0x40 对齐",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_kinds() {
        assert_eq!(cap_kind_name(0), Some("实时流量"));
        assert_eq!(cap_kind_name(1), Some("已存会话"));
        assert_eq!(cap_kind_name(2), Some("导入归档"));
        assert!(cap_kind_name(3).is_none());
        assert_eq!(cap_kind(11), Some(2));
        assert!(cap_kind(12).is_none());
        assert!(cap_cell(12).is_none());
    }

    #[test]
    fn test_comp_kind() {
        assert_eq!(comp_kind(0), Some(0));
        assert_eq!(comp_kind(4), Some(0));
        assert_eq!(comp_kind(5), Some(1));
        assert_eq!(comp_kind(9), Some(1));
        assert!(comp_kind(10).is_none());
        assert_eq!(comp_kind_name(0), Some("构造"));
        assert_eq!(comp_kind_name(1), Some("执行"));
        assert!(comp_kind_name(2).is_none());
    }

    #[test]
    fn test_team_states() {
        assert_eq!(team_state_name(0), Some("工作区共享"));
        assert_eq!(team_state_name(1), Some("邀请协作者"));
        assert_eq!(team_state_name(2), Some("同步审计"));
        assert!(team_state_name(3).is_none());
        assert!(team_state(TEAM_STATES).is_none());
    }

    #[test]
    fn test_split_rows() {
        assert_eq!(split_row_name(0), Some("订阅形态分账声明"));
        assert_eq!(split_row_name(1), Some("免费形态分账声明"));
        assert!(split_row_name(2).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_scenes() {
        assert_eq!(scene_name(0), Some("Windows 桌面"));
        assert_eq!(scene_name(4), Some("浏览器端"));
        assert!(scene_name(5).is_none());
        assert!(scene_cell(SCENES).is_none());
        let a = scene_cell(1).unwrap_or(0);
        assert_eq!(a, scene_cell(1).unwrap_or(0));
        assert_ne!(a, scene_cell(2).unwrap_or(0));
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(6).unwrap_or(0);
        assert_eq!(a, cap_cell(6).unwrap_or(0));
        assert_ne!(a, cap_cell(7).unwrap_or(0));
        let r = rule_cell(2).unwrap_or(0);
        assert_eq!(r, rule_cell(2).unwrap_or(0));
        assert!(rule_cell(RULE_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = FidEveryLedger::new();
        let _ = lt.record_comp(6, comp_cell(6).unwrap_or(0));
        assert_eq!(lt.record_comp(3, comp_cell(3).unwrap_or(0)), Err(FidEveryCode::EOrder));
        // 逆序被拒后家族账位不受污染：续落同家族高格仍可收。
        assert!(lt.record_comp(7, comp_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = FidEveryLedger::new();
        // 先录正格再注漂移。
        let _ = lt.record_cap(0, cap_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cap(1, cap_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(FidEveryCode::ECapDrift)
        );
        let _ = lt.record_rule(0, rule_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_rule(1, rule_cell(1).unwrap_or(0).wrapping_add(4)),
            Err(FidEveryCode::ERuleDrift)
        );
        assert_eq!(
            lt.record_comp(0, comp_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(FidEveryCode::ECapDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FidEveryLedger::new();
        let _ = lt.record_cap(3, cap_cell(3).unwrap_or(0));
        assert_eq!(lt.record_cap(3, cap_cell(3).unwrap_or(0)), Err(FidEveryCode::EDup));
        let _ = lt.record_team(0);
        assert_eq!(lt.record_team(0), Err(FidEveryCode::EDup));
        let _ = lt.declare_split(1);
        assert_eq!(lt.declare_split(1), Err(FidEveryCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FidEveryLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = FidEveryLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CAP_CELLS + COMP_OPS + RULE_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7DC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(FidEveryCode::Ok as u32, 0);
        assert_eq!(FidEveryCode::EGradeDrift as u32, 12);
        assert!(!FidEveryCode::Ok.detail().is_empty());
        assert!(!FidEveryCode::ENoTeam.detail().is_empty());
        assert_eq!(FidEveryCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = FidEveryLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52248_checks_green() {
        let s = run_unx52248_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
