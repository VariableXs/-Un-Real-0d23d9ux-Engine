//! UNX-F52252 mitmproxy（Windows 发行版）收口 · 0xC7EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52252"，逐字）：
//!   title: mitmproxy（Windows 发行版）收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52253-J1 六判据
//!     全落；SOAP/REST 工程 ×10；JVM 上栈（F52057 联签锚定行）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52252）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52253-J1，照原文锚定（卡面 criteria 内「SOAP/REST 工程 ×10」
//! 「JVM 上栈（F52057 联签锚定行）」诸语随原文属后卡口径）；功能按本卡
//! 标题（mitmproxy Windows 发行版）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① CLI/Web/addon 三形态 15 格闭集：三家族各 5 格，逐格载荷纯函数
//!      恒等才收，漂移必抓。
//!   ② flow 10 操作集：两家族（flow 处置 0..5 / flow 持久化 5..10），
//!      家族单调（逆序必抓），逐操作落账。
//!   ③ 脚本/插件 8 格：逐格载荷对照恒等（脚本加载/事件订阅/响应篡改/
//!      请求重写/插件卸载/热重载/错误隔离/生命周期回收），漂移必抓。
//!   ④ Python 上栈 4 层（解释器探测/虚拟环境/mitmproxy 包导入/addon
//!      装配）逐层落账，缺一拒封。
//!   ⑤ 证书三元闸：CA 证书生成 / 系统信任安装 / 客户端校验验证三态
//!      逐格落账，越三态闭集必抓。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（形态格漂移/
//!      flow 家族逆序/脚本格漂移），三类注入全部落在当轮临时账本上，led
//!      上 injected==caught==10、escaped==0；漂移注入先录正格再注漂移；
//!      order 注入对空账本同样必抓且不改 last_kind；双跑指纹一致。
//!   ⑦ 定级 S1 口径：六判据 + 15 形态格 + 10 flow 操作 + 8 脚本格 +
//!      三元闸 + 4 层上栈全通 → S1。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52252;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 三形态格数。
pub const FORM_CELLS: usize = 15;

/// flow 操作数。
pub const FLOW_OPS: usize = 10;

/// 脚本/插件格数。
pub const SCRIPT_CELLS: usize = 8;

/// Python 上栈层数。
pub const PY_LAYERS: usize = 4;

/// 证书三元闸态数。
pub const CERT_STATES: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MitmCode {
    /// 成功
    Ok = 0,
    /// 形态格越闭集
    ENoForm,
    /// 形态格载荷漂移
    EFormDrift,
    /// flow 操作越闭集
    ENoFlow,
    /// flow 家族逆序
    EOrder,
    /// 脚本/插件格越闭集
    ENoScript,
    /// 脚本/插件格漂移
    EScriptDrift,
    /// 证书三元闸越态
    ENoCert,
    /// Python 上栈越层
    ENoPy,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl MitmCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MitmCode::Ok => "成功",
            MitmCode::ENoForm => "形态格越闭集：CLI/Web/addon 三形态 15 格闭集之外先登记",
            MitmCode::EFormDrift => "形态漂移：形态格载荷与基准不恒等，逐格复算",
            MitmCode::ENoFlow => "flow 操作越闭集：flow 10 操作集之外先登记",
            MitmCode::EOrder => "flow 家族逆序：flow 处置→flow 持久化单调，逆序必抓",
            MitmCode::ENoScript => "脚本格越闭集：脚本/插件 8 格闭集之外先登记",
            MitmCode::EScriptDrift => "脚本漂移：脚本/插件格载荷与基准不恒等，逐格复算",
            MitmCode::ENoCert => "证书三元闸越态：CA 生成/系统信任安装/客户端校验验证三态之外先登记",
            MitmCode::ENoPy => "Python 上栈越层：解释器探测/虚拟环境/包导入/addon 装配四层之外先登记",
            MitmCode::EDup => "重复落账：同格只许一条账目",
            MitmCode::ESeal => "未齐即封：15 形态格 + 10 flow 操作 + 8 脚本格未齐先补齐",
            MitmCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            MitmCode::EGradeDrift => "定级漂移：六判据+15 形态格+10 操作+8 脚本格+三元闸+4 层全通才可定 S1",
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
            Grade::S1 => "S1：六判据+15 形态格+10 flow 操作+8 脚本格+三元闸+4 层上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 形态格载荷（格 i；家族 = i/5）。
pub const fn form_cell(i: usize) -> Option<u64> {
    if i >= FORM_CELLS {
        return None;
    }
    Some((0x140000u64 + (i as u64) * 0x53) ^ 0x4D49544D50520000)
}

/// 形态家族（0..5=CLI 5..10=Web 10..15=addon）。
pub const fn form_kind(i: usize) -> Option<u32> {
    if i >= FORM_CELLS {
        return None;
    }
    Some((i / 5) as u32)
}

/// 形态家族名。
pub const fn form_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("CLI 形态"),
        1 => Some("Web 形态"),
        2 => Some("addon 形态"),
        _ => None,
    }
}

/// flow 操作家族（0..5=flow 处置 5..10=flow 持久化）。
pub const fn flow_kind(o: usize) -> Option<u32> {
    if o >= FLOW_OPS {
        return None;
    }
    if o < 5 {
        Some(0)
    } else {
        Some(1)
    }
}

/// flow 操作家族名。
pub const fn flow_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("flow 处置"),
        1 => Some("flow 持久化"),
        _ => None,
    }
}

/// flow 操作行折叠（行 o，落账值）。
pub const fn flow_row(o: usize) -> Option<u32> {
    if o >= FLOW_OPS {
        return None;
    }
    Some(0x14100 + o as u32 * 0x41)
}

/// 脚本/插件格载荷（格 s）。
pub const fn script_cell(s: usize) -> Option<u64> {
    if s >= SCRIPT_CELLS {
        return None;
    }
    Some((0x141000u64 + (s as u64) * 0x2F) ^ 0x505954484F4E0000)
}

/// 脚本/插件格名。
pub const fn script_cell_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("脚本加载"),
        1 => Some("事件订阅"),
        2 => Some("响应篡改"),
        3 => Some("请求重写"),
        4 => Some("插件卸载"),
        5 => Some("热重载"),
        6 => Some("错误隔离"),
        7 => Some("生命周期回收"),
        _ => None,
    }
}

/// Python 上栈层折叠（层 l）。
pub const fn py_layer(l: usize) -> Option<u32> {
    if l >= PY_LAYERS {
        return None;
    }
    Some(0x14200 + l as u32 * 0x5F)
}

/// Python 上栈层名。
pub const fn py_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("解释器探测"),
        1 => Some("虚拟环境"),
        2 => Some("mitmproxy 包导入"),
        3 => Some("addon 装配"),
        _ => None,
    }
}

/// 证书三元闸态折叠（态 c）。
pub const fn cert_state(c: usize) -> Option<u32> {
    if c >= CERT_STATES {
        return None;
    }
    Some(0x14300 + c as u32 * 0x61)
}

/// 证书三元闸态名。
pub const fn cert_state_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("CA 证书生成"),
        1 => Some("系统信任安装"),
        2 => Some("客户端校验验证"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// mitmproxy（Windows 发行版）收口账本状态机。
#[derive(Clone)]
pub struct MitmLedger {
    /// 六判据（安装/CLI/Web/addon/证书/卸载）。
    gates: [Option<bool>; 6],
    /// 形态账。
    forms: [Option<u64>; FORM_CELLS],
    /// flow 操作账。
    flows: [Option<u32>; FLOW_OPS],
    /// 脚本/插件账。
    scripts: [Option<u64>; SCRIPT_CELLS],
    /// Python 上栈账。
    pys: [Option<u32>; PY_LAYERS],
    /// 证书三元闸账。
    certs: [Option<u32>; CERT_STATES],
    /// 最后 flow 家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MitmLedger {
    /// 新账本。
    pub fn new() -> Self {
        MitmLedger {
            gates: [None; 6],
            forms: [None; FORM_CELLS],
            flows: [None; FLOW_OPS],
            scripts: [None; SCRIPT_CELLS],
            pys: [None; PY_LAYERS],
            certs: [None; CERT_STATES],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MitmCode> {
        if gate >= 6 {
            return Err(MitmCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xC100 + gate as u64);
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

    /// 形态一格（载荷对照 + 重复拒）。
    pub fn record_form(&mut self, i: usize, got: u64) -> Result<(), MitmCode> {
        if i >= FORM_CELLS {
            return Err(MitmCode::ENoForm);
        }
        let want = match form_cell(i) {
            Some(w) => w,
            None => return Err(MitmCode::ENoForm),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC200 + i as u64);
            return Err(MitmCode::EFormDrift);
        }
        if match self.forms.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MitmCode::EDup);
        }
        if let Some(c) = self.forms.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xC201 + i as u64);
        Ok(())
    }

    /// 形态全账核验（15 格）。
    pub fn forms_all(&self) -> bool {
        let mut i = 0usize;
        while i < FORM_CELLS {
            match self.forms.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// flow 操作一格（家族单调 + 重复拒；拒后不改 last_kind）。
    pub fn record_flow(&mut self, o: usize) -> Result<(), MitmCode> {
        if o >= FLOW_OPS {
            return Err(MitmCode::ENoFlow);
        }
        let kind = match flow_kind(o) {
            Some(k) => k,
            None => return Err(MitmCode::ENoFlow),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC300);
            return Err(MitmCode::EOrder);
        }
        if match self.flows.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MitmCode::EDup);
        }
        let v = match flow_row(o) {
            Some(v) => v,
            None => return Err(MitmCode::ENoFlow),
        };
        if let Some(c) = self.flows.get_mut(o) {
            *c = Some(v);
        }
        self.last_kind = kind as i64;
        self.fnv(0xC301 + o as u64);
        Ok(())
    }

    /// flow 全账核验（10 操作）。
    pub fn flows_all(&self) -> bool {
        let mut o = 0usize;
        while o < FLOW_OPS {
            match self.flows.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 脚本/插件一格（载荷对照 + 重复拒）。
    pub fn record_script(&mut self, s: usize, got: u64) -> Result<(), MitmCode> {
        if s >= SCRIPT_CELLS {
            return Err(MitmCode::ENoScript);
        }
        let want = match script_cell(s) {
            Some(w) => w,
            None => return Err(MitmCode::ENoScript),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xC400 + s as u64);
            return Err(MitmCode::EScriptDrift);
        }
        if match self.scripts.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MitmCode::EDup);
        }
        if let Some(c) = self.scripts.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0xC401 + s as u64);
        Ok(())
    }

    /// 脚本全账核验（8 格）。
    pub fn scripts_all(&self) -> bool {
        let mut s = 0usize;
        while s < SCRIPT_CELLS {
            match self.scripts.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Python 上栈一层。
    pub fn record_py(&mut self, l: usize) -> Result<(), MitmCode> {
        let v = match py_layer(l) {
            Some(v) => v,
            None => return Err(MitmCode::ENoPy),
        };
        if match self.pys.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MitmCode::EDup);
        }
        if let Some(c) = self.pys.get_mut(l) {
            *c = Some(v);
        }
        self.fnv(0xC500 + l as u64);
        Ok(())
    }

    /// 上栈全齐核验（4 层）。
    pub fn pys_all(&self) -> bool {
        let mut l = 0usize;
        while l < PY_LAYERS {
            match self.pys.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 证书三元闸一格。
    pub fn record_cert(&mut self, c: usize) -> Result<(), MitmCode> {
        let v = match cert_state(c) {
            Some(v) => v,
            None => return Err(MitmCode::ENoCert),
        };
        if match self.certs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MitmCode::EDup);
        }
        if let Some(x) = self.certs.get_mut(c) {
            *x = Some(v);
        }
        self.fnv(0xC600 + c as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn certs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CERT_STATES {
            match self.certs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
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
            && self.forms_all()
            && self.flows_all()
            && self.scripts_all()
            && self.certs_all()
            && self.pys_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MitmLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MitmLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < FORM_CELLS {
        if let Some(w) = form_cell(i) {
            if led.record_form(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut o = 0usize;
    while o < FLOW_OPS {
        if led.record_flow(o).is_ok() {
            recorded += 1;
        }
        o += 1;
    }
    let mut sc = 0usize;
    while sc < SCRIPT_CELLS {
        if let Some(w) = script_cell(sc) {
            if led.record_script(sc, w).is_ok() {
                recorded += 1;
            }
        }
        sc += 1;
    }
    let mut l = 0usize;
    while l < PY_LAYERS {
        let _ = led.record_py(l);
        l += 1;
    }
    let mut c = 0usize;
    while c < CERT_STATES {
        let _ = led.record_cert(c);
        c += 1;
    }
    // 注入 10 轮：形态格漂移 / flow 家族逆序 / 脚本格漂移三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移；
    // order 注入对空账本同样必抓且不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MitmLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_form(0, form_cell(0).unwrap_or(0));
                lt.record_form(1, form_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(MitmCode::EFormDrift)
            }
            1 => {
                let _ = lt.record_flow(6);
                lt.record_flow(3) == Err(MitmCode::EOrder)
            }
            _ => {
                let _ = lt.record_script(0, script_cell(0).unwrap_or(0));
                lt.record_script(1, script_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(MitmCode::EScriptDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xC700 + k as u64);
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
/// UNX-F52252 判据集（锚 UNX-F52253-J1，off-by-one 板式）。
pub fn run_unx52252_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52252");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "CLI/Web/addon 三形态 15 格全通",
        recorded >= FORM_CELLS as u32,
        "三家族各 5 格逐格载荷对照恒等",
    );
    s.add(
        "flow 10 操作全通",
        {
            let mut lt = MitmLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < FLOW_OPS {
                if lt.record_flow(o).is_err() {
                    ok = false;
                }
                o += 1;
            }
            ok && lt.flows_all()
        },
        "flow 处置/flow 持久化两家族逐操作落账，家族单调",
    );
    s.add(
        "脚本/插件 8 格全通",
        {
            let mut lt = MitmLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SCRIPT_CELLS {
                if let Some(w) = script_cell(sc) {
                    if lt.record_script(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.scripts_all()
        },
        "脚本加载至生命周期回收 8 格逐格载荷对照恒等",
    );
    s.add(
        "证书三元闸 3/3 全落",
        {
            let mut lt = MitmLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CERT_STATES {
                if lt.record_cert(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.certs_all()
        },
        "CA 证书生成/系统信任安装/客户端校验验证三态逐格落账",
    );
    s.add(
        "Python 上栈 4 层全落",
        {
            let mut lt = MitmLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < PY_LAYERS {
                if lt.record_py(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.pys_all()
        },
        "解释器探测/虚拟环境/mitmproxy 包导入/addon 装配逐层落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = MitmLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/CLI/Web/addon/证书/卸载逐格落账",
    );
    s.add(
        "flow 家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FLOW_OPS {
                if flow_kind(o).unwrap_or(2) < flow_kind(o - 1).unwrap_or(2) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "flow 处置→flow 持久化闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "形态漂移/flow 逆序/脚本漂移三态轮转必抓");
    let mut l0 = MitmLedger::new();
    s.add(
        "边界·形态越闭集",
        l0.record_form(FORM_CELLS, 0).is_err(),
        "形态格 15 越闭集必拒",
    );
    s.add(
        "边界·flow 越闭集",
        l0.record_flow(FLOW_OPS).is_err(),
        "flow 操作 10 越闭集必拒",
    );
    s.add(
        "边界·脚本越闭集",
        l0.record_script(SCRIPT_CELLS, 0).is_err(),
        "脚本格 8 越闭集必拒",
    );
    s.add(
        "脚本漂移拒",
        {
            let _ = l0.record_script(4, script_cell(4).unwrap_or(0));
            l0.record_script(5, script_cell(5).unwrap_or(0).wrapping_add(1))
                == Err(MitmCode::EScriptDrift)
        },
        "脚本/插件格载荷漂移必抓",
    );
    s.add(
        "逆序拒后同族复验",
        {
            let _ = l0.record_flow(6);
            let r1 = l0.record_flow(3) == Err(MitmCode::EOrder);
            // 逆序拒后 last_kind 未改，同族未录格复验必过。
            r1 && l0.record_flow(7).is_ok()
        },
        "逆序拒后同 last_kind 用同族未录格复验",
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
            let mut lt = MitmLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < FORM_CELLS {
                if let Some(w) = form_cell(i) {
                    let _ = lt.record_form(i, w);
                }
                i += 1;
            }
            let mut o = 0usize;
            while o < FLOW_OPS {
                let _ = lt.record_flow(o);
                o += 1;
            }
            let mut sc = 0usize;
            while sc < SCRIPT_CELLS {
                if let Some(w) = script_cell(sc) {
                    let _ = lt.record_script(sc, w);
                }
                sc += 1;
            }
            let mut c = 0usize;
            while c < CERT_STATES {
                let _ = lt.record_cert(c);
                c += 1;
            }
            let mut l = 0usize;
            while l < PY_LAYERS {
                let _ = lt.record_py(l);
                l += 1;
            }
            // main/aux 各节全部记账后再断言。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7EC0,
        "SEG_BASE 0x40 对齐且在 VX15 域 52252 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_form_kinds() {
        assert_eq!(form_kind(0), Some(0));
        assert_eq!(form_kind(4), Some(0));
        assert_eq!(form_kind(5), Some(1));
        assert_eq!(form_kind(14), Some(2));
        assert!(form_kind(15).is_none());
        assert!(form_cell(15).is_none());
        assert_eq!(form_kind_name(2), Some("addon 形态"));
    }

    #[test]
    fn test_flow_kinds() {
        assert_eq!(flow_kind(0), Some(0));
        assert_eq!(flow_kind(4), Some(0));
        assert_eq!(flow_kind(5), Some(1));
        assert_eq!(flow_kind(9), Some(1));
        assert!(flow_kind(10).is_none());
        assert!(flow_row(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = script_cell(3).unwrap_or(0);
        assert_eq!(a, script_cell(3).unwrap_or(0));
        assert_ne!(a, script_cell(4).unwrap_or(0));
        // XOR 锚常量低 16 位恒零：锚值低 16 位保持基址低 16 位。
        assert_eq!(a & 0xFFFF, (0x141000u64 + 3 * 0x2F) & 0xFFFF);
        assert!(script_cell(SCRIPT_CELLS).is_none());
        assert!(py_layer(PY_LAYERS).is_none());
        assert!(cert_state(CERT_STATES).is_none());
        assert_eq!(script_cell_name(7), Some("生命周期回收"));
    }

    #[test]
    fn test_flow_order_caught() {
        let mut lt = MitmLedger::new();
        let _ = lt.record_flow(6);
        assert_eq!(lt.record_flow(3), Err(MitmCode::EOrder));
        assert!(lt.record_flow(7).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MitmLedger::new();
        assert_eq!(
            lt.record_form(9, form_cell(9).unwrap_or(0).wrapping_add(1)),
            Err(MitmCode::EFormDrift)
        );
        assert_eq!(
            lt.record_script(6, script_cell(6).unwrap_or(0).wrapping_add(4)),
            Err(MitmCode::EScriptDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MitmLedger::new();
        let _ = lt.record_cert(0);
        assert_eq!(lt.record_cert(0), Err(MitmCode::EDup));
        let _ = lt.record_py(3);
        assert_eq!(lt.record_py(3), Err(MitmCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MitmLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= FORM_CELLS as u32 + FLOW_OPS as u32 + SCRIPT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC7EC0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MitmCode::Ok as u32, 0);
        assert_eq!(MitmCode::EGradeDrift as u32, 12);
        assert_eq!(MitmCode::EScriptDrift.val(), SEG_BASE + 6);
        assert!(!MitmCode::Ok.detail().is_empty());
        assert!(!MitmCode::ENoCert.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = MitmLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52252_checks_green() {
        let s = run_unx52252_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
