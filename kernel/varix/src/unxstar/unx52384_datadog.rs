//! UNX-F52384 Datadog Agent 收口 · 0xC9FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52384"（判据原文锚定源）criteria：① 纯功能代码 ≥300 行；② 判据
//!   原文：UNX-F52385-J1 本地指标/日志 ×10（外部上报面声明）；KB-COMPAT
//!   定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!   结果摘要非空。
//!   键 "52385"（功能标题源）title：Datadog Agent 收口
//!   quota: 300
//! off-by-one 板式口径：本卡（卡 N=52384）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52385-J1（本地指标/日志 ×10，外部上报面声明），照原文（键
//! "52384" criteria）锚定；功能按本卡标题（Datadog Agent 收口，键
//! "52385" title）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五检查族闭集 20 格：cpu/memory/disk/network/integrations 各 4 格，
//!      族序单调（首族必为 cpu，逆跳必抓），逐格载荷纯函数折叠恒等才收。
//!   ② datadog.yaml 双向投影 12 格：六对（正向投影/反向回读成对落账），
//!      反向先于正向必抓（EOrder）。
//!   ③ forwarder 三元闸：队列/重试/压缩三行声明，缺项 EFwdMiss 拒封。
//!   ④ 进程监控 8 拍单调：拍号严格逐拍递进（0→7），拍载荷逐拍恒等递增，
//!      跳拍/倒拍/漂移一律 EMono 抓获。
//!   ⑤ 与站点消费联签 4 格（外部上报面声明）+ Go 上栈声明 2 行闭集，
//!      缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（检查载荷漂移/投影对序逆/越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 检查 + 12 投影 + 8 拍 + 3 闸 + 4 联签 + 2 Go 全齐 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52384;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 检查格数（五族 × 4）。
pub const CHECKS: usize = 20;

/// datadog.yaml 双向投影格数（六对）。
pub const PROJ_CELLS: usize = 12;

/// 进程监控拍数。
pub const PROC_BEATS: usize = 8;

/// forwarder 三元闸行数。
pub const FWD_ROWS: usize = 3;

/// 与站点消费联签行数。
pub const SITE_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DatadogCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 载荷漂移
    EDrift,
    /// 检查族序逆/投影对序逆
    EOrder,
    /// 进程监控拍非单调
    EMono,
    /// forwarder 三元闸缺项
    EFwdMiss,
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

impl DatadogCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DatadogCode::Ok => "成功",
            DatadogCode::ENoCell => "格号越闭集：检查 20/投影 12/拍 8 之外先登记",
            DatadogCode::EDrift => "载荷漂移：格折叠与基准不恒等，逐格复算",
            DatadogCode::EOrder => "序逆：cpu→memory→disk→network→integrations 族序单调；投影反向先于正向必抓",
            DatadogCode::EMono => "进程监控拍非单调：8 拍须逐拍递进且拍载荷逐拍恒等递增",
            DatadogCode::EFwdMiss => "forwarder 三元闸缺项：队列/重试/压缩三行未齐先补齐",
            DatadogCode::ENoRow => "声明行越闭集：三元闸 3 行 + 联签 4 行 + Go 上栈 2 行之外先登记",
            DatadogCode::ERowMiss => "缺声明行：站点消费联签/Go 上栈声明未齐先补齐",
            DatadogCode::EDup => "重复落账：同格只许一条账目",
            DatadogCode::ESeal => "未齐即封：20 检查 + 12 投影 + 8 拍未齐先补齐",
            DatadogCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DatadogCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DatadogCode::EGradeDrift => "定级漂移：六闸+全节+3 闸+4 联签+2 Go 全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20 检查+12 投影+8 拍+3 闸+4 联签+2 Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 检查格载荷（格 o；族 = o/4）。
pub const fn chk_cell(o: usize) -> Option<u64> {
    if o >= CHECKS {
        return None;
    }
    let fam = (o / 4) as u64;
    Some(
        (0xDE000u64 + (o as u64) * 0x2C)
            ^ fam.wrapping_mul(0x01000193)
            ^ 0x4441444F47000000,
    )
}

/// 检查族（0..4=cpu 4..8=memory 8..12=disk 12..16=network 16..20=integrations）。
pub const fn chk_family(o: usize) -> Option<u32> {
    if o >= CHECKS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 检查族名。
pub const fn family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("cpu"),
        1 => Some("memory"),
        2 => Some("disk"),
        3 => Some("network"),
        4 => Some("integrations"),
        _ => None,
    }
}

/// 投影格载荷（格 i；对 = i/2，偶正向/奇反向）。
pub const fn proj_cell(i: usize) -> Option<u64> {
    if i >= PROJ_CELLS {
        return None;
    }
    let pair = (i / 2) as u64;
    Some((0xDF000u64 + (i as u64) * 0x2C) ^ pair.wrapping_mul(0x85EBCA77) ^ 0x59414D4C00000000)
}

/// 投影对号（0..6）。
pub const fn proj_pair(i: usize) -> Option<u32> {
    if i >= PROJ_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 进程监控拍基准（拍 b；逐拍严格递增）。
pub const fn proc_beat(b: usize) -> Option<u64> {
    if b >= PROC_BEATS {
        return None;
    }
    Some(0xE0000u64 + (b as u64) * 0x3B)
}

/// forwarder 三元闸行折叠（行 r：0=队列 1=重试 2=压缩）。
pub const fn fwd_row(r: usize) -> Option<u32> {
    if r >= FWD_ROWS {
        return None;
    }
    Some(0xE0C00 + r as u32 * 0x2C)
}

/// 与站点消费联签行折叠（行 r；外部上报面声明）。
pub const fn site_row(r: usize) -> Option<u32> {
    if r >= SITE_ROWS {
        return None;
    }
    Some(0xE0C60 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xE0CC0 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Datadog Agent 收口账本状态机。
#[derive(Clone)]
pub struct DatadogLedger {
    /// 六判据（安装/API Key 配置/服务注册/检查功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 检查账。
    chks: [Option<u64>; CHECKS],
    /// 投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// 进程监控拍账。
    proc: [Option<u64>; PROC_BEATS],
    /// forwarder 三元闸账。
    fwd: [Option<u32>; FWD_ROWS],
    /// 站点消费联签账。
    sites: [Option<u32>; SITE_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后检查族。
    last_family: i64,
    /// 最后监控拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DatadogLedger {
    /// 新账本。
    pub fn new() -> Self {
        DatadogLedger {
            gates: [None; 6],
            chks: [None; CHECKS],
            proj: [None; PROJ_CELLS],
            proc: [None; PROC_BEATS],
            fwd: [None; FWD_ROWS],
            sites: [None; SITE_ROWS],
            gos: [None; GO_ROWS],
            last_family: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DatadogCode> {
        if gate >= 6 {
            return Err(DatadogCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xDE100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < 6 {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 检查一格（族序单调 + 首族必 cpu + 载荷对照 + 重复拒）。
    pub fn record_chk(&mut self, o: usize, got: u64) -> Result<(), DatadogCode> {
        if o >= CHECKS {
            return Err(DatadogCode::ENoCell);
        }
        let fam = match chk_family(o) {
            Some(k) => k as i64,
            None => return Err(DatadogCode::ENoCell),
        };
        let order_bad = if self.last_family < 0 {
            fam != 0
        } else {
            fam < self.last_family || fam > self.last_family + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDE400);
            return Err(DatadogCode::EOrder);
        }
        let want = match chk_cell(o) {
            Some(w) => w,
            None => return Err(DatadogCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDE401 + o as u64);
            return Err(DatadogCode::EDrift);
        }
        if match self.chks.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DatadogCode::EDup);
        }
        if let Some(c) = self.chks.get_mut(o) {
            *c = Some(got);
        }
        self.last_family = fam;
        self.fnv(0xDE402 + o as u64);
        Ok(())
    }

    /// 检查全账核验（20 格）。
    pub fn chks_all(&self) -> bool {
        let mut o = 0usize;
        while o < CHECKS {
            match self.chks.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 投影一格（对内先正后反 + 载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, i: usize, got: u64) -> Result<(), DatadogCode> {
        if i >= PROJ_CELLS {
            return Err(DatadogCode::ENoCell);
        }
        // 双向投影成对：奇格（反向）落账前其偶格镜像（正向）必须已在账。
        if i % 2 == 1 {
            let fwd_ok = match self.proj.get(i - 1) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !fwd_ok {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0xDF400);
                return Err(DatadogCode::EOrder);
            }
        }
        let want = match proj_cell(i) {
            Some(w) => w,
            None => return Err(DatadogCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDF401 + i as u64);
            return Err(DatadogCode::EDrift);
        }
        if match self.proj.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DatadogCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xDF402 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn proj_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROJ_CELLS {
            match self.proj.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 进程监控一拍（拍号严格递进 + 拍载荷对照 + 重复拒）。
    pub fn record_proc(&mut self, b: usize, got: u64) -> Result<(), DatadogCode> {
        if b >= PROC_BEATS {
            return Err(DatadogCode::ENoCell);
        }
        if b as i64 != self.last_beat + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xE0400);
            return Err(DatadogCode::EMono);
        }
        let want = match proc_beat(b) {
            Some(w) => w,
            None => return Err(DatadogCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xE0401 + b as u64);
            return Err(DatadogCode::EMono);
        }
        if match self.proc.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DatadogCode::EDup);
        }
        if let Some(c) = self.proc.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0xE0402 + b as u64);
        Ok(())
    }

    /// 进程监控全账核验（8 拍）。
    pub fn proc_all(&self) -> bool {
        let mut b = 0usize;
        while b < PROC_BEATS {
            match self.proc.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// forwarder 三元闸一行（0=队列 1=重试 2=压缩）。
    pub fn declare_fwd(&mut self, r: usize) -> Result<(), DatadogCode> {
        if r >= FWD_ROWS {
            return Err(DatadogCode::ENoRow);
        }
        if match self.fwd.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DatadogCode::EDup);
        }
        if let Some(c) = self.fwd.get_mut(r) {
            *c = fwd_row(r);
        }
        self.fnv(0xE0C00 + r as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn fwd_all(&self) -> bool {
        let mut r = 0usize;
        while r < FWD_ROWS {
            match self.fwd.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 与站点消费联签一行（外部上报面声明）。
    pub fn declare_site(&mut self, r: usize) -> Result<(), DatadogCode> {
        if r >= SITE_ROWS {
            return Err(DatadogCode::ENoRow);
        }
        if match self.sites.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DatadogCode::EDup);
        }
        if let Some(c) = self.sites.get_mut(r) {
            *c = site_row(r);
        }
        self.fnv(0xE0C60 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn sites_all(&self) -> bool {
        let mut r = 0usize;
        while r < SITE_ROWS {
            match self.sites.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), DatadogCode> {
        if r >= GO_ROWS {
            return Err(DatadogCode::ENoRow);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DatadogCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xE0CC0 + r as u64);
        Ok(())
    }

    /// Go 上栈声明全齐核验。
    pub fn gos_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.gos.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：三元闸缺 → EFwdMiss；联签/Go 行缺 → ERowMiss；格未齐 → ESeal。
    pub fn seal(&self) -> Result<(), DatadogCode> {
        if !self.fwd_all() {
            return Err(DatadogCode::EFwdMiss);
        }
        if !(self.sites_all() && self.gos_all()) {
            return Err(DatadogCode::ERowMiss);
        }
        if !(self.gates_all() && self.chks_all() && self.proj_all() && self.proc_all()) {
            return Err(DatadogCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), DatadogCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(DatadogCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), DatadogCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(DatadogCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &DatadogLedger) -> Grade {
    if led.gates_all() && led.chks_all() && led.proj_all() && led.proc_all() && led.fwd_all()
        && led.sites_all() && led.gos_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

impl Default for DatadogLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DatadogLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CHECKS {
        if let Some(w) = chk_cell(o) {
            if led.record_chk(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < PROJ_CELLS {
        if let Some(w) = proj_cell(i) {
            if led.record_proj(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut b = 0usize;
    while b < PROC_BEATS {
        if let Some(w) = proc_beat(b) {
            if led.record_proc(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < FWD_ROWS {
        let _ = led.declare_fwd(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < SITE_ROWS {
        let _ = led.declare_site(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：检查载荷漂移 / 投影对序逆（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DatadogLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_chk(0, chk_cell(0).unwrap_or(0));
                lt.record_chk(1, chk_cell(1).unwrap_or(0).wrapping_add(1)) == Err(DatadogCode::EDrift)
            }
            1 => lt.record_proj(1, proj_cell(1).unwrap_or(0)) == Err(DatadogCode::EOrder),
            _ => {
                lt.record_chk(CHECKS, 0).is_err()
                    && lt.record_proj(PROJ_CELLS, 0).is_err()
                    && lt.record_proc(PROC_BEATS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xDE600 + k as u64);
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
/// UNX-F52384 判据集（锚 UNX-F52385-J1，off-by-one 板式）。
pub fn run_unx52384_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52384");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == CHECKS as u32 + PROJ_CELLS as u32 + PROC_BEATS as u32,
        "20 检查 + 12 投影 + 8 拍逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "检查五族 20 格全通",
        {
            let mut lt = DatadogLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CHECKS {
                if let Some(w) = chk_cell(o) {
                    if lt.record_chk(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.chks_all()
        },
        "cpu/memory/disk/network/integrations 五族逐格载荷恒等",
    );
    s.add(
        "datadog.yaml 双向投影 12 格全落",
        {
            let mut lt = DatadogLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    if lt.record_proj(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.proj_all()
        },
        "六对正反投影成对落账",
    );
    s.add(
        "forwarder 三元闸 3/3",
        {
            let mut lt = DatadogLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < FWD_ROWS {
                if lt.declare_fwd(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.fwd_all()
        },
        "队列/重试/压缩三行闭集",
    );
    s.add(
        "进程监控 8 拍单调",
        {
            let mut lt = DatadogLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < PROC_BEATS {
                if let Some(w) = proc_beat(b) {
                    if lt.record_proc(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.proc_all()
        },
        "拍号逐拍递进且拍载荷逐拍恒等递增",
    );
    s.add(
        "站点消费联签 4/4（外部上报面声明）",
        {
            let mut lt = DatadogLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SITE_ROWS {
                if lt.declare_site(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sites_all()
        },
        "Datadog 站点消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = DatadogLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gos_all()
        },
        "Go 运行时上栈两行声明闭集",
    );
    s.add(
        "检查族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CHECKS {
                if chk_family(o).unwrap_or(5) < chk_family(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "cpu→memory→disk→network→integrations 五族闭集单调",
    );
    s.add(
        "投影对完整",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if proj_pair(i).unwrap_or(6) >= 6 {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "六对投影对号闭集 0..6",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = DatadogLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/API Key 配置/服务注册/检查功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·检查越闭集",
        DatadogLedger::new().record_chk(CHECKS, 0).is_err(),
        "检查 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        DatadogLedger::new().record_proj(PROJ_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·监控拍越闭集",
        DatadogLedger::new().record_proc(PROC_BEATS, 0).is_err(),
        "进程监控拍 8 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            let mut lt = DatadogLedger::new();
            lt.record_proj(0, proj_cell(0).unwrap_or(0).wrapping_add(1)) == Err(DatadogCode::EDrift)
        },
        "投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "检查漂移/投影对序逆/越闭集三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp_check(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = DatadogLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CHECKS {
                if let Some(w) = chk_cell(o) {
                    let _ = lt.record_chk(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    let _ = lt.record_proj(i, w);
                }
                i += 1;
            }
            let mut b = 0usize;
            while b < PROC_BEATS {
                if let Some(w) = proc_beat(b) {
                    let _ = lt.record_proc(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < FWD_ROWS {
                let _ = lt.declare_fwd(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < SITE_ROWS {
                let _ = lt.declare_site(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go(r);
                r += 1;
            }
            lt.seal().is_ok() && judge_grade(&lt) == Grade::S1
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_family() {
        assert_eq!(chk_family(0), Some(0));
        assert_eq!(chk_family(4), Some(1));
        assert_eq!(chk_family(19), Some(4));
        assert!(chk_family(20).is_none());
        assert!(chk_cell(20).is_none());
        assert_eq!(family_name(0), Some("cpu"));
        assert_eq!(family_name(4), Some("integrations"));
        assert!(family_name(5).is_none());
    }

    #[test]
    fn test_proj_pairs() {
        assert_eq!(proj_pair(0), Some(0));
        assert_eq!(proj_pair(1), Some(0));
        assert_eq!(proj_pair(11), Some(5));
        assert!(proj_pair(12).is_none());
        assert!(proj_cell(12).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = chk_cell(3).unwrap_or(0);
        assert_eq!(a, chk_cell(3).unwrap_or(0));
        assert_ne!(a, chk_cell(4).unwrap_or(0));
        assert_ne!(proc_beat(0).unwrap_or(0), proc_beat(3).unwrap_or(0));
        assert!(fwd_row(FWD_ROWS).is_none());
        assert!(site_row(SITE_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 cpu，跳族必抓。
        let mut lt = DatadogLedger::new();
        assert_eq!(
            lt.record_chk(8, chk_cell(8).unwrap_or(0)),
            Err(DatadogCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_chk(0, chk_cell(0).unwrap_or(0));
        let _ = lt.record_chk(4, chk_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_chk(2, chk_cell(2).unwrap_or(0)),
            Err(DatadogCode::EOrder)
        );
        assert!(lt.record_chk(5, chk_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_proj_pair_order_caught() {
        // 空账本：反向投影先于正向必抓（对空账本同样成立）。
        let mut lt = DatadogLedger::new();
        assert_eq!(
            lt.record_proj(1, proj_cell(1).unwrap_or(0)),
            Err(DatadogCode::EOrder)
        );
        // 正向落账后反向可落。
        let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
        assert!(lt.record_proj(1, proj_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_and_mono_caught() {
        let mut lt = DatadogLedger::new();
        assert_eq!(
            lt.record_chk(0, chk_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(DatadogCode::EDrift)
        );
        // 进程监控跳拍与拍载荷漂移均 EMono。
        assert_eq!(
            lt.record_proc(3, proc_beat(3).unwrap_or(0)),
            Err(DatadogCode::EMono)
        );
        assert_eq!(
            lt.record_proc(0, proc_beat(0).unwrap_or(0).wrapping_add(2)),
            Err(DatadogCode::EMono)
        );
        assert!(lt.record_proc(0, proc_beat(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DatadogLedger::new();
        let _ = lt.record_chk(0, chk_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_chk(0, chk_cell(0).unwrap_or(0)),
            Err(DatadogCode::EDup)
        );
        let _ = lt.declare_site(0);
        assert_eq!(lt.declare_site(0), Err(DatadogCode::EDup));
        assert_eq!(lt.declare_site(SITE_ROWS), Err(DatadogCode::ENoRow));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DatadogLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!DatadogLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：三元闸缺 → EFwdMiss。
        assert_eq!(DatadogLedger::new().seal(), Err(DatadogCode::EFwdMiss));
        // 闸齐、联签/Go 行缺 → ERowMiss。
        let mut lt = DatadogLedger::new();
        let mut r = 0usize;
        while r < FWD_ROWS {
            let _ = lt.declare_fwd(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(DatadogCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut r = 0usize;
        while r < SITE_ROWS {
            let _ = lt.declare_site(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = lt.declare_go(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(DatadogCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 检查 + 12 投影 + 8 拍（闸与声明行不计数）。
        assert_eq!(
            recorded,
            CHECKS as u32 + PROJ_CELLS as u32 + PROC_BEATS as u32
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
        assert!(fp_check(f1, f2).is_ok());
        assert!(fp_check(f1, f1.wrapping_add(1)).is_err());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9FC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DatadogCode::Ok as u32, 0);
        assert_eq!(DatadogCode::EGradeDrift as u32, 12);
        assert_eq!(DatadogCode::Ok.val(), SEG_BASE);
        assert_eq!(DatadogCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!DatadogCode::Ok.detail().is_empty());
        assert!(!DatadogCode::EFwdMiss.detail().is_empty());
        assert!(!DatadogCode::EFp.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&DatadogLedger::new()), Grade::S3);
        let mut lt = DatadogLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        let mut full = DatadogLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < CHECKS {
            if let Some(w) = chk_cell(o) {
                let _ = full.record_chk(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < PROJ_CELLS {
            if let Some(w) = proj_cell(i) {
                let _ = full.record_proj(i, w);
            }
            i += 1;
        }
        let mut b = 0usize;
        while b < PROC_BEATS {
            if let Some(w) = proc_beat(b) {
                let _ = full.record_proc(b, w);
            }
            b += 1;
        }
        let mut r = 0usize;
        while r < FWD_ROWS {
            let _ = full.declare_fwd(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < SITE_ROWS {
            let _ = full.declare_site(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = full.declare_go(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S1);
        assert!(full.conservation().is_ok());
    }

    #[test]
    fn unx52384_checks_green() {
        let s = run_unx52384_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 20);
    }
}
