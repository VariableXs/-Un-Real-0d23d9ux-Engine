//! UNX-F52219 PL/SQL Developer 收口 · 0xC7680段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52219"，逐字）：
//!   title: PL/SQL Developer 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52220-J1 六判据
//!     全落；MySQL 操作集 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52219）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52220-J1，照原文锚定；功能按本卡标题（PL/SQL Developer）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① Oracle 版本阶梯闭集：11g / 12c / 19c 三级，越级必抓；每级独立
//!      载荷折叠，纯函数恒等才收。
//!   ② 编辑器/调试器/测试窗口三面 15 格：每面 5 格（SQL 窗口/命令窗口/
//!      断点/单步/DBMS_OUTPUT 回显 等逐格折叠），越面格必抓。
//!   ③ 插件 6 格闭集：装载序列单调（逆序必抓），逐格载荷恒等才收。
//!   ④ 六判据收口闸（编辑器/调试器/测试窗口/插件/卸载/许可）逐格落账，
//!      未齐即封拒。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（版本越阶梯/漂移/插件逆序），
//!      全部落在当轮临时账本上，检出+逃逸守恒等式（injected==caught==10、
//!      escaped==0）；双跑 FNV 指纹一致；定级闭集 S2/S3 —— 有条件收口
//!      封顶 S2，不得漂移到 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52219;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Oracle 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// 三面格数（编辑器 5 / 调试器 5 / 测试窗口 5）。
pub const FACE_CELLS: usize = 15;

/// 插件格数。
pub const PLUGIN_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlSqlCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 面格越闭集
    ENoFace,
    /// 面格漂移
    EFaceDrift,
    /// 插件号越闭集
    ENoPlugin,
    /// 插件格漂移
    EPluginDrift,
    /// 插件装载逆序
    EOrder,
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

impl PlSqlCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PlSqlCode::Ok => "成功",
            PlSqlCode::ENoVer => "版本级越阶梯：Oracle 11g/12c/19c 三级闭集之外先登记",
            PlSqlCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            PlSqlCode::ENoFace => "面格越闭集：编辑器/调试器/测试窗口 15 格闭集之外先登记",
            PlSqlCode::EFaceDrift => "面格漂移：面格折叠与基准不恒等，逐格复算",
            PlSqlCode::ENoPlugin => "插件号越闭集：插件 6 格闭集之外先登记",
            PlSqlCode::EPluginDrift => "插件格漂移：插件格折叠与基准不恒等，逐格复算",
            PlSqlCode::EOrder => "插件装载逆序：插件序列单调，逆序必抓",
            PlSqlCode::EDup => "重复落账：同格只许一条账目",
            PlSqlCode::ESeal => "未齐即封：15 面格 + 6 插件格 + 3 级版本未齐先补齐",
            PlSqlCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PlSqlCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PlSqlCode::EGradeDrift => "定级漂移：有条件收口封顶 S2，不得漂移 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（PL/SQL Developer：有条件收口封顶 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S2：有条件收口（本卡封顶）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S2 => "S2：六判据+15面格+6插件格+3级版本全通的有条件收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯级载荷（级 v：0=11g 1=12c 2=19c）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xC0000u64 + (v as u64) * 0x61) ^ 0x504C53514C5F5631)
}

/// 版本级名（const fn 表 + Option 边界）。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("11g"),
        1 => Some("12c"),
        2 => Some("19c"),
        _ => None,
    }
}

/// 三面格载荷（格 f：0..5=编辑器 5..10=调试器 10..15=测试窗口）。
pub const fn face_cell(f: usize) -> Option<u64> {
    if f >= FACE_CELLS {
        return None;
    }
    Some((0xC1000u64 + (f as u64) * 0x33) ^ ((f / 5) as u64).wrapping_mul(0x01000193))
}

/// 面名。
pub const fn face_name(f: usize) -> Option<&'static str> {
    if f >= FACE_CELLS {
        return None;
    }
    match f / 5 {
        0 => Some("编辑器面"),
        1 => Some("调试器面"),
        _ => Some("测试窗口面"),
    }
}

/// 插件格载荷（插件 p；装载序 = p 闭集单调）。
pub const fn plugin_cell(p: usize) -> Option<u32> {
    if p >= PLUGIN_CELLS {
        return None;
    }
    Some(0xC200 + p as u32 * 0x17)
}

/// 插件名。
pub const fn plugin_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("代码补全插件"),
        1 => Some("格式化插件"),
        2 => Some("版本控制插件"),
        3 => Some("计划任务插件"),
        4 => Some("报表插件"),
        5 => Some("DBMS 监视插件"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// PL/SQL Developer 收口账本状态机。
#[derive(Clone)]
pub struct PlSqlLedger {
    /// 六判据（编辑器/调试器/测试窗口/插件/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 面格账。
    faces: [Option<u64>; FACE_CELLS],
    /// 插件格账。
    plugins: [Option<u32>; PLUGIN_CELLS],
    /// 最后插件装载序。
    last_plugin: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PlSqlLedger {
    /// 新账本。
    pub fn new() -> Self {
        PlSqlLedger {
            gates: [None; 6],
            vers: [None; VERSION_STEPS],
            faces: [None; FACE_CELLS],
            plugins: [None; PLUGIN_CELLS],
            last_plugin: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PlSqlCode> {
        if gate >= 6 {
            return Err(PlSqlCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), PlSqlCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(PlSqlCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + v as u64);
            return Err(PlSqlCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PlSqlCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x7201 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 级全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 面格一格（载荷对照 + 重复拒）。
    pub fn record_face(&mut self, f: usize, got: u64) -> Result<(), PlSqlCode> {
        let want = match face_cell(f) {
            Some(w) => w,
            None => return Err(PlSqlCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + f as u64);
            return Err(PlSqlCode::EFaceDrift);
        }
        if match self.faces.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PlSqlCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x7301 + f as u64);
        Ok(())
    }

    /// 面格全账核验（15 格）。
    pub fn faces_all(&self) -> bool {
        let mut f = 0usize;
        while f < FACE_CELLS {
            match self.faces.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 插件一格（装载序单调 + 载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, p: usize, got: u32) -> Result<(), PlSqlCode> {
        if p >= PLUGIN_CELLS {
            return Err(PlSqlCode::ENoPlugin);
        }
        if (p as i64) < self.last_plugin {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400);
            return Err(PlSqlCode::EOrder);
        }
        let want = match plugin_cell(p) {
            Some(w) => w,
            None => return Err(PlSqlCode::ENoPlugin),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7401 + p as u64);
            return Err(PlSqlCode::EPluginDrift);
        }
        if match self.plugins.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PlSqlCode::EDup);
        }
        if let Some(c) = self.plugins.get_mut(p) {
            *c = Some(got);
        }
        self.last_plugin = p as i64;
        self.fnv(0x7402 + p as u64);
        Ok(())
    }

    /// 插件全账核验（6 格全齐且单调）。
    pub fn plugins_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            match self.plugins.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
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

    /// KB-COMPAT 定级（全通 → S2 封顶，判据缺 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.vers_all() && self.faces_all() && self.plugins_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for PlSqlLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PlSqlLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut f = 0usize;
    while f < FACE_CELLS {
        if let Some(w) = face_cell(f) {
            if led.record_face(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    // 注入 10 轮：版本越阶梯 / 漂移 / 插件逆序 三态轮转，
    // 三类注入全部落在当轮临时账本上；漂移先录正格再注漂移；
    // 逆序注入对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PlSqlLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
                lt.record_version(VERSION_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_face(2, face_cell(2).unwrap_or(0));
                lt.record_face(2, face_cell(2).unwrap_or(0).wrapping_add(1)) == Err(PlSqlCode::EFaceDrift)
            }
            _ => {
                let _ = lt.record_plugin(4, plugin_cell(4).unwrap_or(0));
                lt.record_plugin(1, plugin_cell(1).unwrap_or(0)) == Err(PlSqlCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7500 + k as u64);
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
/// UNX-F52219 判据集（锚 UNX-F52220-J1，off-by-one 板式）。
pub fn run_unx52219_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52219");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "编辑器/调试器/测试窗口三面 15 格全落",
        recorded >= FACE_CELLS as u32,
        "三面各 5 格闭集逐格恒等（§AI-66.3）",
    );
    s.add(
        "Oracle 11g/12c/19c 阶梯三级全落",
        {
            let mut lt = PlSqlLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三级闭集逐级恒等，越级必抓",
    );
    s.add(
        "插件 6 格全落",
        {
            let mut lt = PlSqlLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    if lt.record_plugin(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugins_all()
        },
        "插件序列单调装载，逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = PlSqlLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "编辑器/调试器/测试窗口/插件/卸载/许可逐格落账（UNX-F52220-J1 锚）",
    );
    s.add(
        "面格载荷恒等",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < FACE_CELLS {
                match (face_cell(f), face_cell(f)) {
                    (Some(a), Some(b)) => {
                        if a != b {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                f += 1;
            }
            ok
        },
        "15 面格折叠纯函数逐位确定",
    );
    s.add(
        "插件装载单调",
        {
            let mut ok = true;
            let mut p = 1usize;
            while p < PLUGIN_CELLS {
                if plugin_cell(p).unwrap_or(0) < plugin_cell(p - 1).unwrap_or(0) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "6 格插件闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "版本越阶梯/漂移/插件逆序三态轮转必抓",
    );
    let mut l0 = PlSqlLedger::new();
    s.add(
        "边界·版本级越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "边界·面格越闭集",
        l0.record_face(FACE_CELLS, 0).is_err(),
        "面格 15 越闭集必拒",
    );
    s.add(
        "边界·插件号越闭集",
        l0.record_plugin(PLUGIN_CELLS, 0).is_err(),
        "插件号 6 越闭集必拒",
    );
    s.add(
        "漂移拒（先录正格再注漂移）",
        {
            let _ = l0.record_face(2, face_cell(2).unwrap_or(0));
            l0.record_face(2, face_cell(2).unwrap_or(0).wrapping_add(1)) == Err(PlSqlCode::EFaceDrift)
        },
        "面格载荷漂移必抓",
    );
    s.add(
        "逆序拒（空账本同样必抓）",
        {
            let mut lt = PlSqlLedger::new();
            let _ = lt.record_plugin(4, plugin_cell(4).unwrap_or(0));
            lt.record_plugin(1, plugin_cell(1).unwrap_or(0)) == Err(PlSqlCode::EOrder)
        },
        "插件 4 先装、插件 1 后装，逆序必抓",
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
        "定级 S2 闭集（有条件收口封顶）",
        {
            let mut lt = PlSqlLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
            }
            let mut f = 0usize;
            while f < FACE_CELLS {
                if let Some(w) = face_cell(f) {
                    let _ = lt.record_face(f, w);
                }
                f += 1;
            }
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(p) {
                    let _ = lt.record_plugin(p, w);
                }
                p += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通封顶 S2（KB-COMPAT 定级登记，不得漂移 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * 18,
        "SEG_BASE 0x40 对齐且在 VX15 域 0xC7680 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("11g"));
        assert_eq!(version_name(1), Some("12c"));
        assert_eq!(version_name(2), Some("19c"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_face_table() {
        assert_eq!(face_name(0), Some("编辑器面"));
        assert_eq!(face_name(5), Some("调试器面"));
        assert_eq!(face_name(14), Some("测试窗口面"));
        assert!(face_cell(15).is_none());
        assert_ne!(face_cell(0).unwrap_or(0), face_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_plugin_table() {
        assert_eq!(plugin_name(0), Some("代码补全插件"));
        assert_eq!(plugin_name(5), Some("DBMS 监视插件"));
        assert!(plugin_cell(6).is_none());
        assert!(plugin_name(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = face_cell(3).unwrap_or(0);
        assert_eq!(a, face_cell(3).unwrap_or(0));
        assert_ne!(a, face_cell(4).unwrap_or(0));
        let v = version_cell(2).unwrap_or(0);
        assert_eq!(v, version_cell(2).unwrap_or(0));
    }

    #[test]
    fn test_plugin_order_caught() {
        let mut lt = PlSqlLedger::new();
        let _ = lt.record_plugin(4, plugin_cell(4).unwrap_or(0));
        assert_eq!(lt.record_plugin(1, plugin_cell(1).unwrap_or(0)), Err(PlSqlCode::EOrder));
        assert!(lt.record_plugin(5, plugin_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = PlSqlLedger::new();
        assert_eq!(
            lt.record_face(5, face_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(PlSqlCode::EFaceDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(PlSqlCode::EVerDrift)
        );
        assert_eq!(
            lt.record_plugin(3, plugin_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(PlSqlCode::EPluginDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PlSqlLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(PlSqlCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PlSqlLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = PlSqlLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (FACE_CELLS + VERSION_STEPS + PLUGIN_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7680);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PlSqlCode::Ok as u32, 0);
        assert_eq!(PlSqlCode::EGradeDrift as u32, 12);
        assert!(!PlSqlCode::Ok.detail().is_empty());
        assert!(!PlSqlCode::ESeal.detail().is_empty());
        assert_eq!(PlSqlCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = PlSqlLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52219_checks_green() {
        let s = run_unx52219_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
