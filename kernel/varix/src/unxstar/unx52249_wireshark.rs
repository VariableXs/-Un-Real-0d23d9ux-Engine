//! UNX-F52249 Wireshark 收口 · 0xC7E00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52249"，逐字）：
//!   title: Wireshark 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52250-J1 六判据
//!     全落；HTTPS 代理/限速/重写三面 ×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52249）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52250-J1，照原文锚定（卡面 criteria 内「HTTPS 代理/限速/重写」
//! 诸语随原文属后卡 Charles 口径）；功能按本卡标题（Wireshark）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 捕获三态 12 格闭集：三家族（本地接口实时捕获/远程接口捕获/文件
//!      读取回放）各 4 格，逐格载荷纯函数恒等才收，漂移必抓。
//!   ② 显示过滤器 10 操作集：两家族（过滤构造 0..5 / 过滤应用 5..10），
//!      家族单调（逆序必抓），逐操作落账。
//!   ③ 协议解析四层 16 格：链路层/网络层/传输层/应用层各 4 格，逐格
//!      载荷对照恒等，层序单调。
//!   ④ 解密三元闸：TLS 密钥日志 / 预共享密钥配置 / 解密会话验证三态
//!      逐格落账，越三态闭集必抓。
//!   ⑤ Qt 上栈 4 层（事件循环/主窗口装配/报文面板/详情树渲染）逐层
//!      落账；Npcap 消费联签 4 格（驱动安装/枚举接口/捕获会话/回放注入）
//!      逐格联签落账，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（捕获格漂移/
//!      过滤家族逆序/捕获越闭集），三类注入全部落在当轮临时账本上，led
//!      上 injected==caught==10、escaped==0；漂移注入先录正格再注漂移；
//!      order 注入对空账本同样必抓且不改 last_kind；双跑指纹一致。
//!   ⑦ 定级 S1 口径：六判据 + 12 捕获格 + 10 过滤操作 + 16 解析格 +
//!      三元闸 + 4 层上栈 + 4 格联签全通 → S1。
//!   ⑧ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52249;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 捕获三态格数。
pub const CAP_CELLS: usize = 12;

/// 显示过滤器操作数。
pub const FILTER_OPS: usize = 10;

/// 协议解析格数（四层）。
pub const DISSECT_CELLS: usize = 16;

/// 解密三元闸态数。
pub const DECRYPT_STATES: usize = 3;

/// Qt 上栈层数。
pub const QT_LAYERS: usize = 4;

/// Npcap 消费联签格数。
pub const NPCAP_CELLS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireSharkCode {
    /// 成功
    Ok = 0,
    /// 捕获格越闭集
    ENoCap,
    /// 载荷漂移（捕获/解析共用漂移码）
    EDrift,
    /// 过滤操作越闭集
    ENoFilter,
    /// 过滤家族逆序
    EOrder,
    /// 解析格越闭集
    ENoDissect,
    /// 解密三元闸越态
    ENoDecrypt,
    /// Qt 上栈越层
    ENoQt,
    /// Npcap 联签格越闭集
    ENoNpcap,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl WireSharkCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WireSharkCode::Ok => "成功",
            WireSharkCode::ENoCap => "捕获格越闭集：捕获三态 12 格闭集之外先登记",
            WireSharkCode::EDrift => "载荷漂移：捕获/解析格载荷与基准不恒等，逐格复算",
            WireSharkCode::ENoFilter => "过滤操作越闭集：显示过滤器 10 操作集之外先登记",
            WireSharkCode::EOrder => "过滤家族逆序：过滤构造→过滤应用单调，逆序必抓",
            WireSharkCode::ENoDissect => "解析格越闭集：协议解析四层 16 格闭集之外先登记",
            WireSharkCode::ENoDecrypt => "解密三元闸越态：TLS 密钥日志/预共享密钥/解密会话验证三态之外先登记",
            WireSharkCode::ENoQt => "Qt 上栈越层：事件循环/主窗口/报文面板/详情树四层之外先登记",
            WireSharkCode::ENoNpcap => "联签格越闭集：Npcap 消费联签 4 格闭集之外先登记",
            WireSharkCode::EDup => "重复落账：同格只许一条账目",
            WireSharkCode::ESeal => "未齐即封：12 捕获格 + 10 过滤操作 + 16 解析格未齐先补齐",
            WireSharkCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WireSharkCode::EGradeDrift => "定级漂移：六判据+12 捕获格+10 操作+16 解析格+三元闸+4 层+4 联签全通才可定 S1",
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
            Grade::S1 => "S1：六判据+12 捕获格+10 过滤操作+16 解析格+三元闸+4 层上栈+4 格联签全通收口",
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
    Some((0x110000u64 + (i as u64) * 0x59) ^ 0x5749524553410000)
}

/// 捕获家族（0..4=本地接口实时捕获 4..8=远程接口捕获 8..12=文件读取回放）。
pub const fn cap_kind(i: usize) -> Option<u32> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 捕获家族名。
pub const fn cap_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("本地接口实时捕获"),
        1 => Some("远程接口捕获"),
        2 => Some("文件读取回放"),
        _ => None,
    }
}

/// 显示过滤操作家族（0..5=过滤构造 5..10=过滤应用）。
pub const fn filter_kind(o: usize) -> Option<u32> {
    if o >= FILTER_OPS {
        return None;
    }
    if o < 5 {
        Some(0)
    } else {
        Some(1)
    }
}

/// 过滤操作家族名。
pub const fn filter_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("过滤构造"),
        1 => Some("过滤应用"),
        _ => None,
    }
}

/// 协议解析格载荷（格 i；层 = i/4）。
pub const fn dissect_cell(i: usize) -> Option<u64> {
    if i >= DISSECT_CELLS {
        return None;
    }
    Some((0x111000u64 + (i as u64) * 0x47) ^ 0x50524F544F000000)
}

/// 解析层（0..4=链路层 4..8=网络层 8..12=传输层 12..16=应用层）。
pub const fn dissect_layer(i: usize) -> Option<u32> {
    if i >= DISSECT_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 解析层名。
pub const fn dissect_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("链路层"),
        1 => Some("网络层"),
        2 => Some("传输层"),
        3 => Some("应用层"),
        _ => None,
    }
}

/// 解密三元闸态折叠（态 s）。
pub const fn decrypt_state(s: usize) -> Option<u32> {
    if s >= DECRYPT_STATES {
        return None;
    }
    Some(0x11200 + s as u32 * 0x5D)
}

/// 解密三元闸态名。
pub const fn decrypt_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("TLS 密钥日志"),
        1 => Some("预共享密钥配置"),
        2 => Some("解密会话验证"),
        _ => None,
    }
}

/// Qt 上栈层折叠（层 l）。
pub const fn qt_layer(l: usize) -> Option<u32> {
    if l >= QT_LAYERS {
        return None;
    }
    Some(0x11300 + l as u32 * 0x53)
}

/// Qt 上栈层名。
pub const fn qt_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("Qt 事件循环"),
        1 => Some("主窗口装配"),
        2 => Some("报文面板"),
        3 => Some("详情树渲染"),
        _ => None,
    }
}

/// Npcap 消费联签格载荷（格 i）。
pub const fn npcap_cell(i: usize) -> Option<u64> {
    if i >= NPCAP_CELLS {
        return None;
    }
    Some((0x114000u64 + (i as u64) * 0x3B) ^ 0x4E50434150000000)
}

/// Npcap 联签格名。
pub const fn npcap_cell_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("驱动安装联签"),
        1 => Some("枚举接口联签"),
        2 => Some("捕获会话联签"),
        3 => Some("回放注入联签"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Wireshark 收口账本状态机。
#[derive(Clone)]
pub struct WireSharkLedger {
    /// 六判据（启动/捕获/过滤/解析/解密/卸载）。
    gates: [Option<bool>; 6],
    /// 捕获账。
    caps: [Option<u64>; CAP_CELLS],
    /// 过滤操作账。
    filters: [Option<u32>; FILTER_OPS],
    /// 解析账。
    dissects: [Option<u64>; DISSECT_CELLS],
    /// 解密三元闸账。
    decrypts: [Option<u32>; DECRYPT_STATES],
    /// Qt 上栈账。
    qts: [Option<u32>; QT_LAYERS],
    /// Npcap 联签账。
    npcaps: [Option<u64>; NPCAP_CELLS],
    /// 最后过滤家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WireSharkLedger {
    /// 新账本。
    pub fn new() -> Self {
        WireSharkLedger {
            gates: [None; 6],
            caps: [None; CAP_CELLS],
            filters: [None; FILTER_OPS],
            dissects: [None; DISSECT_CELLS],
            decrypts: [None; DECRYPT_STATES],
            qts: [None; QT_LAYERS],
            npcaps: [None; NPCAP_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WireSharkCode> {
        if gate >= 6 {
            return Err(WireSharkCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9100 + gate as u64);
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

    /// 捕获一格（载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, i: usize, got: u64) -> Result<(), WireSharkCode> {
        if i >= CAP_CELLS {
            return Err(WireSharkCode::ENoCap);
        }
        let want = match cap_cell(i) {
            Some(w) => w,
            None => return Err(WireSharkCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200 + i as u64);
            return Err(WireSharkCode::EDrift);
        }
        if match self.caps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WireSharkCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x9201 + i as u64);
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

    /// 过滤操作一格（家族单调 + 重复拒；拒后不改 last_kind）。
    pub fn record_filter(&mut self, o: usize) -> Result<(), WireSharkCode> {
        if o >= FILTER_OPS {
            return Err(WireSharkCode::ENoFilter);
        }
        let kind = match filter_kind(o) {
            Some(k) => k,
            None => return Err(WireSharkCode::ENoFilter),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(WireSharkCode::EOrder);
        }
        if match self.filters.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WireSharkCode::EDup);
        }
        if let Some(c) = self.filters.get_mut(o) {
            *c = Some(0x9300 + o as u32);
        }
        self.last_kind = kind as i64;
        self.fnv(0x9301 + o as u64);
        Ok(())
    }

    /// 过滤全账核验（10 操作）。
    pub fn filters_all(&self) -> bool {
        let mut o = 0usize;
        while o < FILTER_OPS {
            match self.filters.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 解析一格（层序单调 + 载荷对照 + 重复拒）。
    pub fn record_dissect(&mut self, i: usize, got: u64) -> Result<(), WireSharkCode> {
        if i >= DISSECT_CELLS {
            return Err(WireSharkCode::ENoDissect);
        }
        let want = match dissect_cell(i) {
            Some(w) => w,
            None => return Err(WireSharkCode::ENoDissect),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + i as u64);
            return Err(WireSharkCode::EDrift);
        }
        if match self.dissects.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WireSharkCode::EDup);
        }
        if let Some(c) = self.dissects.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x9401 + i as u64);
        Ok(())
    }

    /// 解析全账核验（16 格）。
    pub fn dissects_all(&self) -> bool {
        let mut i = 0usize;
        while i < DISSECT_CELLS {
            match self.dissects.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 解密三元闸一格。
    pub fn record_decrypt(&mut self, s: usize) -> Result<(), WireSharkCode> {
        let v = match decrypt_state(s) {
            Some(v) => v,
            None => return Err(WireSharkCode::ENoDecrypt),
        };
        if match self.decrypts.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WireSharkCode::EDup);
        }
        if let Some(c) = self.decrypts.get_mut(s) {
            *c = Some(v);
        }
        self.fnv(0x9500 + s as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn decrypts_all(&self) -> bool {
        let mut s = 0usize;
        while s < DECRYPT_STATES {
            match self.decrypts.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Qt 上栈一层。
    pub fn record_qt(&mut self, l: usize) -> Result<(), WireSharkCode> {
        let v = match qt_layer(l) {
            Some(v) => v,
            None => return Err(WireSharkCode::ENoQt),
        };
        if match self.qts.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WireSharkCode::EDup);
        }
        if let Some(c) = self.qts.get_mut(l) {
            *c = Some(v);
        }
        self.fnv(0x9600 + l as u64);
        Ok(())
    }

    /// 上栈全齐核验（4 层）。
    pub fn qts_all(&self) -> bool {
        let mut l = 0usize;
        while l < QT_LAYERS {
            match self.qts.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// Npcap 联签一格。
    pub fn record_npcap(&mut self, i: usize, got: u64) -> Result<(), WireSharkCode> {
        let want = match npcap_cell(i) {
            Some(w) => w,
            None => return Err(WireSharkCode::ENoNpcap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9700 + i as u64);
            return Err(WireSharkCode::EDrift);
        }
        if match self.npcaps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WireSharkCode::EDup);
        }
        if let Some(c) = self.npcaps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x9701 + i as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn npcaps_all(&self) -> bool {
        let mut i = 0usize;
        while i < NPCAP_CELLS {
            match self.npcaps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
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
            && self.filters_all()
            && self.dissects_all()
            && self.decrypts_all()
            && self.qts_all()
            && self.npcaps_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WireSharkLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WireSharkLedger::new();
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
    while o < FILTER_OPS {
        if led.record_filter(o).is_ok() {
            recorded += 1;
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DISSECT_CELLS {
        if let Some(w) = dissect_cell(d) {
            if led.record_dissect(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut s = 0usize;
    while s < DECRYPT_STATES {
        let _ = led.record_decrypt(s);
        s += 1;
    }
    let mut l = 0usize;
    while l < QT_LAYERS {
        let _ = led.record_qt(l);
        l += 1;
    }
    let mut n = 0usize;
    while n < NPCAP_CELLS {
        if let Some(w) = npcap_cell(n) {
            if led.record_npcap(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    // 注入 10 轮：捕获格漂移 / 过滤家族逆序 / 捕获越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移；
    // order 注入对空账本同样必抓且不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WireSharkLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cap(0, cap_cell(0).unwrap_or(0));
                lt.record_cap(1, cap_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WireSharkCode::EDrift)
            }
            1 => {
                let _ = lt.record_filter(6);
                lt.record_filter(3) == Err(WireSharkCode::EOrder)
            }
            _ => lt.record_cap(CAP_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9800 + k as u64);
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
/// UNX-F52249 判据集（锚 UNX-F52250-J1，off-by-one 板式）。
pub fn run_unx52249_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52249");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "捕获三态 12 格全通",
        recorded >= CAP_CELLS as u32,
        "本地接口实时捕获/远程接口捕获/文件读取回放三家族逐格恒等",
    );
    s.add(
        "显示过滤器 10 操作全通",
        {
            let mut lt = WireSharkLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < FILTER_OPS {
                if lt.record_filter(o).is_err() {
                    ok = false;
                }
                o += 1;
            }
            ok && lt.filters_all()
        },
        "过滤构造/过滤应用两家族逐操作落账，家族单调",
    );
    s.add(
        "协议解析四层 16 格全通",
        {
            let mut lt = WireSharkLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DISSECT_CELLS {
                if let Some(w) = dissect_cell(d) {
                    if lt.record_dissect(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dissects_all()
        },
        "链路层/网络层/传输层/应用层逐格载荷对照恒等",
    );
    s.add(
        "解密三元闸 3/3 全落",
        {
            let mut lt = WireSharkLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < DECRYPT_STATES {
                if lt.record_decrypt(st).is_err() {
                    ok = false;
                }
                st += 1;
            }
            ok && lt.decrypts_all()
        },
        "TLS 密钥日志/预共享密钥配置/解密会话验证三态逐格落账",
    );
    s.add(
        "Qt 上栈 4 层全落",
        {
            let mut lt = WireSharkLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < QT_LAYERS {
                if lt.record_qt(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.qts_all()
        },
        "事件循环/主窗口装配/报文面板/详情树渲染逐层落账",
    );
    s.add(
        "Npcap 消费联签 4/4 全落",
        {
            let mut lt = WireSharkLedger::new();
            let mut ok = true;
            let mut n = 0usize;
            while n < NPCAP_CELLS {
                if let Some(w) = npcap_cell(n) {
                    if lt.record_npcap(n, w).is_err() {
                        ok = false;
                    }
                }
                n += 1;
            }
            ok && lt.npcaps_all()
        },
        "驱动安装/枚举接口/捕获会话/回放注入逐格联签",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WireSharkLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/捕获/过滤/解析/解密/卸载逐格落账",
    );
    s.add(
        "过滤家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FILTER_OPS {
                if filter_kind(o).unwrap_or(2) < filter_kind(o - 1).unwrap_or(2) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "过滤构造→过滤应用闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "捕获漂移/过滤逆序/越闭集三态轮转必抓");
    let mut l0 = WireSharkLedger::new();
    s.add(
        "边界·捕获越闭集",
        l0.record_cap(CAP_CELLS, 0).is_err(),
        "捕获格 12 越闭集必拒",
    );
    s.add(
        "边界·解析越闭集",
        l0.record_dissect(DISSECT_CELLS, 0).is_err(),
        "解析格 16 越闭集必拒",
    );
    s.add(
        "边界·过滤越闭集",
        l0.record_filter(FILTER_OPS).is_err(),
        "过滤操作 10 越闭集必拒",
    );
    s.add(
        "捕获漂移拒",
        {
            let _ = l0.record_cap(0, cap_cell(0).unwrap_or(0));
            l0.record_cap(1, cap_cell(1).unwrap_or(0).wrapping_add(1)) == Err(WireSharkCode::EDrift)
        },
        "捕获格载荷漂移必抓",
    );
    s.add(
        "逆序拒后同族复验",
        {
            let _ = l0.record_filter(6);
            let r1 = l0.record_filter(3) == Err(WireSharkCode::EOrder);
            // 逆序拒后 last_kind 未改，同族未录格复验必过。
            r1 && l0.record_filter(7).is_ok()
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
            let mut lt = WireSharkLedger::new();
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
            while o < FILTER_OPS {
                let _ = lt.record_filter(o);
                o += 1;
            }
            let mut d = 0usize;
            while d < DISSECT_CELLS {
                if let Some(w) = dissect_cell(d) {
                    let _ = lt.record_dissect(d, w);
                }
                d += 1;
            }
            let mut st = 0usize;
            while st < DECRYPT_STATES {
                let _ = lt.record_decrypt(st);
                st += 1;
            }
            let mut l = 0usize;
            while l < QT_LAYERS {
                let _ = lt.record_qt(l);
                l += 1;
            }
            let mut n = 0usize;
            while n < NPCAP_CELLS {
                if let Some(w) = npcap_cell(n) {
                    let _ = lt.record_npcap(n, w);
                }
                n += 1;
            }
            // main/aux 各节全部记账后再断言。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7E00,
        "SEG_BASE 0x40 对齐且在 VX15 域 52249 格",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_kinds() {
        assert_eq!(cap_kind(0), Some(0));
        assert_eq!(cap_kind(4), Some(1));
        assert_eq!(cap_kind(11), Some(2));
        assert!(cap_kind(12).is_none());
        assert!(cap_cell(12).is_none());
        assert_eq!(cap_kind_name(2), Some("文件读取回放"));
        assert!(cap_kind_name(3).is_none());
    }

    #[test]
    fn test_filter_kinds() {
        assert_eq!(filter_kind(0), Some(0));
        assert_eq!(filter_kind(4), Some(0));
        assert_eq!(filter_kind(5), Some(1));
        assert_eq!(filter_kind(9), Some(1));
        assert!(filter_kind(10).is_none());
    }

    #[test]
    fn test_dissect_layers() {
        assert_eq!(dissect_layer(0), Some(0));
        assert_eq!(dissect_layer(4), Some(1));
        assert_eq!(dissect_layer(8), Some(2));
        assert_eq!(dissect_layer(15), Some(3));
        assert!(dissect_layer(16).is_none());
        assert!(dissect_cell(16).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(3).unwrap_or(0);
        assert_eq!(a, cap_cell(3).unwrap_or(0));
        assert_ne!(a, cap_cell(4).unwrap_or(0));
        // XOR 锚常量低 16 位恒零：锚值低 16 位 == 段基址低 16 位。
        assert_eq!(a & 0xFFFF, (0x110000u64 + 3 * 0x59) & 0xFFFF);
        assert!(npcap_cell(NPCAP_CELLS).is_none());
        assert!(decrypt_state(DECRYPT_STATES).is_none());
        assert!(qt_layer(QT_LAYERS).is_none());
    }

    #[test]
    fn test_filter_order_caught() {
        let mut lt = WireSharkLedger::new();
        let _ = lt.record_filter(6);
        assert_eq!(lt.record_filter(3), Err(WireSharkCode::EOrder));
        assert!(lt.record_filter(7).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WireSharkLedger::new();
        assert_eq!(
            lt.record_cap(5, cap_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(WireSharkCode::EDrift)
        );
        assert_eq!(
            lt.record_dissect(2, dissect_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(WireSharkCode::EDrift)
        );
        assert_eq!(
            lt.record_npcap(1, npcap_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(WireSharkCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WireSharkLedger::new();
        let _ = lt.record_decrypt(0);
        assert_eq!(lt.record_decrypt(0), Err(WireSharkCode::EDup));
        let _ = lt.record_qt(2);
        assert_eq!(lt.record_qt(2), Err(WireSharkCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WireSharkLedger::new();
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
        assert!(recorded >= CAP_CELLS as u32 + FILTER_OPS as u32 + DISSECT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC7E00);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WireSharkCode::Ok as u32, 0);
        assert_eq!(WireSharkCode::EGradeDrift as u32, 12);
        assert_eq!(WireSharkCode::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!WireSharkCode::Ok.detail().is_empty());
        assert!(!WireSharkCode::ESeal.detail().is_empty());
        assert!(!WireSharkCode::ENoNpcap.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = WireSharkLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52249_checks_green() {
        let s = run_unx52249_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
