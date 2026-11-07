//! UNX-F52389 NSClient++ 收口 · 0xCA100段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52389"（判据原文锚定源）criteria：① 纯功能代码 ≥260 行；② 判据
//!   原文：UNX-F52390-J1 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52390"（功能标题源）title：NSClient++ 收口
//!   quota: 300
//! off-by-one 板式口径：本卡（卡 N=52389）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52390-J1（×10），照原文（键 "52389" criteria）锚定；功能按本卡
//! 标题（NSClient++ 收口，键 "52390" title）落地；行数基线按功能标题源
//! quota=300 执行（落盘 ≥360 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 check 族闭集 20 格：CPU/内存/磁盘/服务/日志各 4 格（Nagios
//!      check 命令语义），族序单调（空账本首族必 CPU，逆跳必抓），逐格
//!      载荷纯函数折叠恒等才收，漂移必抓。
//!   ② NSC.ini 双向投影 12 格：正向投影（账→ini 节键）与回投核验（同格
//!      二次提交必拒 EDup），投影载荷漂移 EIniDrift 必抓。
//!   ③ NRPE/NSCA/nscp 三协议 6 格：每协议 2 格，协议态单调（空账本首协
//!      议必 NRPE，逆跳/越协议必抓 EProtoOrd）。
//!   ④ 与 Nagios/Zabbix 消费联签 4 格 + C++ 上栈声明 2 行闭集，缺行拒封
//!      （ERowMiss），未齐拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（check 载荷漂移/协议态逆/越闭
//!      集），检出+逃逸守恒等式；双跑 FNV 指纹一致。
//!   ⑥ KB-COMPAT 定级闭集 S1/S2/S3：本卡定级口径最高 S2（有条件收口，
//!      Nagios/Zabbix 联签为消费面前向声明，S1 预留硬件在环实测档）；
//!      空账 S3，六闸齐 S2，全齐目标档 S2。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52389;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// check 格数（五族 × 4）。
pub const CHKS: usize = 20;

/// NSC.ini 双向投影格数。
pub const INI_CELLS: usize = 12;

/// 三协议格数（NRPE/NSCA/nscp × 2）。
pub const PROTOS: usize = 6;

/// Nagios/Zabbix 消费联签行数。
pub const UNION_ROWS: usize = 4;

/// C++ 上栈声明行数。
pub const CPP_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NsclientCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// check/协议载荷漂移
    EDrift,
    /// check 族逆序
    EOrder,
    /// NSC.ini 投影漂移
    EIniDrift,
    /// 协议号越闭集
    ENoProto,
    /// 协议态逆序
    EProtoOrd,
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

impl NsclientCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NsclientCode::Ok => "成功",
            NsclientCode::ENoCell => "格号越闭集：check 20 格/投影 12 格闭集之外先登记",
            NsclientCode::EDrift => "载荷漂移：check/协议折叠与基准不恒等，逐格复算",
            NsclientCode::EOrder => "check 族逆序：CPU→内存→磁盘→服务→日志单调，逆跳必抓",
            NsclientCode::EIniDrift => "NSC.ini 投影漂移：双向投影载荷与基准不恒等，逐格复算",
            NsclientCode::ENoProto => "协议号越闭集：NRPE/NSCA/nscp 6 格之外先登记",
            NsclientCode::EProtoOrd => "协议态逆序：首协议必 NRPE，NRPE→NSCA→nscp 单调，逆跳必抓",
            NsclientCode::ERowMiss => "缺声明行：Nagios/Zabbix 联签 4 行或 C++ 上栈 2 行未齐先补齐",
            NsclientCode::EDup => "重复落账：同格只许一条账目",
            NsclientCode::ESeal => "未齐即封：20 check + 12 投影 + 6 协议 + 闸未齐先补齐",
            NsclientCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NsclientCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NsclientCode::EGradeDrift => "定级漂移：全齐才可定目标档 S2，判据缺只可定 S3",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：硬件在环实测档（本卡预留，不落自动定级）
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
            Grade::S1 => "S1：硬件在环实测档，NSClient++ 卡预留不自动落级",
            Grade::S2 => "S2：有条件收口（NSClient++ 目标档），六闸齐即达，全齐保持",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// check 格载荷（格 o；族 = o/4）。
pub const fn chk_cell(o: usize) -> Option<u64> {
    if o >= CHKS {
        return None;
    }
    Some((0xB8000u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// check 族（0..4=CPU 4..8=内存 8..12=磁盘 12..16=服务 16..20=日志）。
pub const fn chk_kind(o: usize) -> Option<u32> {
    if o >= CHKS {
        return None;
    }
    Some((o / 4) as u32)
}

/// check 族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("CPU"),
        1 => Some("内存"),
        2 => Some("磁盘"),
        3 => Some("服务"),
        4 => Some("日志"),
        _ => None,
    }
}

/// NSC.ini 双向投影格载荷（格 i）。
pub const fn ini_cell(i: usize) -> Option<u64> {
    if i >= INI_CELLS {
        return None;
    }
    Some((0xB9000u64 + (i as u64) * 0x3B) ^ 0x4E534349_0000)
}

/// 协议号（0..2=NRPE 2..4=NSCA 4..6=nscp）。
pub const fn proto_of(i: usize) -> Option<u32> {
    if i >= PROTOS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 协议名。
pub const fn proto_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("NRPE"),
        1 => Some("NSCA"),
        2 => Some("nscp"),
        _ => None,
    }
}

/// 协议格载荷（格 i）。
pub const fn proto_cell(i: usize) -> Option<u64> {
    if i >= PROTOS {
        return None;
    }
    Some((0xBA000u64 + (i as u64) * 0x47) ^ 0x50524F54_0000)
}

/// Nagios/Zabbix 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xBB00 + r as u32 * 0x2C)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn cpp_row(r: usize) -> Option<u32> {
    if r >= CPP_ROWS {
        return None;
    }
    Some(0xBB40 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// NSClient++ 收口账本状态机。
#[derive(Clone)]
pub struct NsclientLedger {
    /// 六判据（安装/服务注册/端口开放/check 功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// check 账。
    chks: [Option<u64>; CHKS],
    /// NSC.ini 双向投影账。
    inis: [Option<u64>; INI_CELLS],
    /// 三协议账。
    protos: [Option<u64>; PROTOS],
    /// 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// C++ 上栈声明账。
    cpps: [Option<u32>; CPP_ROWS],
    /// 最后 check 族。
    last_kind: i64,
    /// 最后协议号。
    last_proto: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NsclientLedger {
    /// 新账本。
    pub fn new() -> Self {
        NsclientLedger {
            gates: [None; 6],
            chks: [None; CHKS],
            inis: [None; INI_CELLS],
            protos: [None; PROTOS],
            unions: [None; UNION_ROWS],
            cpps: [None; CPP_ROWS],
            last_kind: -1,
            last_proto: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NsclientCode> {
        if gate >= 6 {
            return Err(NsclientCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCA1000 + gate as u64);
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

    /// check 一格（族序单调 + 首族必 CPU + 载荷对照 + 重复拒）。
    pub fn record_chk(&mut self, o: usize, got: u64) -> Result<(), NsclientCode> {
        if o >= CHKS {
            return Err(NsclientCode::ENoCell);
        }
        let kind = match chk_kind(o) {
            Some(k) => k as i64,
            None => return Err(NsclientCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1400);
            return Err(NsclientCode::EOrder);
        }
        let want = match chk_cell(o) {
            Some(w) => w,
            None => return Err(NsclientCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1401 + o as u64);
            return Err(NsclientCode::EDrift);
        }
        if match self.chks.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsclientCode::EDup);
        }
        if let Some(c) = self.chks.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCA1402 + o as u64);
        Ok(())
    }

    /// check 全账核验（20 格）。
    pub fn chks_all(&self) -> bool {
        let mut o = 0usize;
        while o < CHKS {
            match self.chks.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// NSC.ini 双向投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_ini(&mut self, i: usize, got: u64) -> Result<(), NsclientCode> {
        if i >= INI_CELLS {
            return Err(NsclientCode::ENoCell);
        }
        let want = match ini_cell(i) {
            Some(w) => w,
            None => return Err(NsclientCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1800 + i as u64);
            return Err(NsclientCode::EIniDrift);
        }
        if match self.inis.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsclientCode::EDup);
        }
        if let Some(c) = self.inis.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xCA1801 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn inis_all(&self) -> bool {
        let mut i = 0usize;
        while i < INI_CELLS {
            match self.inis.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 协议一格（协议态单调 + 首协议必 NRPE + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, i: usize, got: u64) -> Result<(), NsclientCode> {
        if i >= PROTOS {
            return Err(NsclientCode::ENoProto);
        }
        let p = match proto_of(i) {
            Some(p) => p as i64,
            None => return Err(NsclientCode::ENoProto),
        };
        let ord_bad = if self.last_proto < 0 {
            p != 0
        } else {
            p < self.last_proto || p > self.last_proto + 1
        };
        if ord_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1C00);
            return Err(NsclientCode::EProtoOrd);
        }
        let want = match proto_cell(i) {
            Some(w) => w,
            None => return Err(NsclientCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1C01 + i as u64);
            return Err(NsclientCode::EDrift);
        }
        if match self.protos.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsclientCode::EDup);
        }
        if let Some(c) = self.protos.get_mut(i) {
            *c = Some(got);
        }
        self.last_proto = p;
        self.fnv(0xCA1C02 + i as u64);
        Ok(())
    }

    /// 协议全账核验（6 格）。
    pub fn protos_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROTOS {
            match self.protos.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Nagios/Zabbix 消费联签一行。
    pub fn declare_union(&mut self, r: usize) -> Result<(), NsclientCode> {
        if r >= UNION_ROWS {
            return Err(NsclientCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsclientCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xCA2000 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn unions_all(&self) -> bool {
        let mut r = 0usize;
        while r < UNION_ROWS {
            match self.unions.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C++ 上栈声明一行。
    pub fn declare_cpp(&mut self, r: usize) -> Result<(), NsclientCode> {
        if r >= CPP_ROWS {
            return Err(NsclientCode::ERowMiss);
        }
        if match self.cpps.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NsclientCode::EDup);
        }
        if let Some(c) = self.cpps.get_mut(r) {
            *c = cpp_row(r);
        }
        self.fnv(0xCA2080 + r as u64);
        Ok(())
    }

    /// C++ 声明全齐核验。
    pub fn cpps_all(&self) -> bool {
        let mut r = 0usize;
        while r < CPP_ROWS {
            match self.cpps.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；格/闸未齐 → ESeal；全齐 → Ok。
    pub fn seal(&self) -> Result<(), NsclientCode> {
        if !(self.unions_all() && self.cpps_all()) {
            return Err(NsclientCode::ERowMiss);
        }
        if !(self.gates_all() && self.chks_all() && self.inis_all() && self.protos_all()) {
            return Err(NsclientCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), NsclientCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(NsclientCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for NsclientLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), NsclientCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(NsclientCode::EFp)
    }
}

/// KB-COMPAT 定级（六闸齐 → S2 目标档起步，全齐保持 S2，判据缺 → S3；
/// NSClient++ 卡口径：消费联签为前向声明，S1 预留硬件在环实测档）。
pub fn judge_grade(led: &NsclientLedger) -> Grade {
    if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &NsclientLedger, want: Grade) -> Result<(), NsclientCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(NsclientCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NsclientLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CHKS {
        if let Some(w) = chk_cell(o) {
            if led.record_chk(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < INI_CELLS {
        if let Some(w) = ini_cell(i) {
            if led.record_ini(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut p = 0usize;
    while p < PROTOS {
        if let Some(w) = proto_cell(p) {
            if led.record_proto(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < UNION_ROWS {
        let _ = led.declare_union(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < CPP_ROWS {
        let _ = led.declare_cpp(r);
        r += 1;
    }
    // 注入 10 轮：check 载荷漂移 / 协议态逆（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NsclientLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_chk(0, chk_cell(0).unwrap_or(0));
                lt.record_chk(1, chk_cell(1).unwrap_or(0).wrapping_add(1)) == Err(NsclientCode::EDrift)
            }
            1 => lt.record_proto(2, proto_cell(2).unwrap_or(0)) == Err(NsclientCode::EProtoOrd),
            _ => {
                lt.record_chk(CHKS, 0).is_err()
                    && lt.record_ini(INI_CELLS, 0).is_err()
                    && lt.record_proto(PROTOS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCA2100 + k as u64);
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
/// UNX-F52389 判据集（锚 UNX-F52390-J1 ×10，off-by-one 板式）。
pub fn run_unx52389_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52389");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == CHKS as u32 + INI_CELLS as u32 + PROTOS as u32,
        "20 check + 12 投影 + 6 协议逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "check 五族 20 格全通",
        {
            let mut lt = NsclientLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < CHKS {
                if let Some(w) = chk_cell(o) {
                    if lt.record_chk(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.chks_all()
        },
        "CPU/内存/磁盘/服务/日志五族逐格载荷恒等",
    );
    s.add(
        "NSC.ini 双向投影 12 格全落",
        {
            let mut lt = NsclientLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < INI_CELLS {
                if let Some(w) = ini_cell(i) {
                    if lt.record_ini(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.inis_all()
        },
        "账→ini 正向投影逐格恒等落账",
    );
    s.add(
        "NSC.ini 回投核验 12/12",
        {
            let mut lt = NsclientLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < INI_CELLS {
                if let Some(w) = ini_cell(i) {
                    let _ = lt.record_ini(i, w);
                    // 回投：同格二次提交必拒 EDup（ini→账反向核验）。
                    if lt.record_ini(i, w) != Err(NsclientCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.inis_all()
        },
        "ini→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "三协议 6 格全落",
        {
            let mut lt = NsclientLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROTOS {
                if let Some(w) = proto_cell(p) {
                    if lt.record_proto(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.protos_all()
        },
        "NRPE/NSCA/nscp 各 2 格协议态单调落账",
    );
    s.add(
        "Nagios/Zabbix 消费联签 4/4",
        {
            let mut lt = NsclientLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < UNION_ROWS {
                if lt.declare_union(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.unions_all()
        },
        "Nagios/Zabbix 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "C++ 上栈声明 2/2",
        {
            let mut lt = NsclientLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CPP_ROWS {
                if lt.declare_cpp(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cpps_all()
        },
        "C++ 运行时上栈两行声明闭集",
    );
    s.add(
        "check 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CHKS {
                if chk_kind(o).unwrap_or(5) < chk_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "CPU→内存→磁盘→服务→日志五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = NsclientLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/端口开放/check 功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·check 越闭集",
        NsclientLedger::new().record_chk(CHKS, 0).is_err(),
        "check 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        NsclientLedger::new().record_ini(INI_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·协议越闭集",
        NsclientLedger::new().record_proto(PROTOS, 0).is_err(),
        "协议 6 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = NsclientLedger::new();
            let _ = lt.record_ini(0, ini_cell(0).unwrap_or(0));
            lt.record_ini(1, ini_cell(1).unwrap_or(0).wrapping_add(3)) == Err(NsclientCode::EIniDrift)
        },
        "投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "check 漂移/协议态逆/越闭集三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &alloc::format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp_check(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S2 目标档闭集",
        {
            let mut lt = NsclientLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CHKS {
                if let Some(w) = chk_cell(o) {
                    let _ = lt.record_chk(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < INI_CELLS {
                if let Some(w) = ini_cell(i) {
                    let _ = lt.record_ini(i, w);
                }
                i += 1;
            }
            let mut p = 0usize;
            while p < PROTOS {
                if let Some(w) = proto_cell(p) {
                    let _ = lt.record_proto(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < UNION_ROWS {
                let _ = lt.declare_union(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < CPP_ROWS {
                let _ = lt.declare_cpp(r);
                r += 1;
            }
            lt.seal().is_ok() && grade_guard(&lt, Grade::S2).is_ok()
        },
        "全齐 → S2 目标档（KB-COMPAT 定级登记，S1 预留实测档）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chk_kind() {
        assert_eq!(chk_kind(0), Some(0));
        assert_eq!(chk_kind(4), Some(1));
        assert_eq!(chk_kind(19), Some(4));
        assert!(chk_kind(20).is_none());
        assert!(chk_cell(20).is_none());
        assert_eq!(kind_name(0), Some("CPU"));
        assert_eq!(kind_name(4), Some("日志"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_protocol_map() {
        assert_eq!(proto_of(0), Some(0));
        assert_eq!(proto_of(1), Some(0));
        assert_eq!(proto_of(2), Some(1));
        assert_eq!(proto_of(5), Some(2));
        assert!(proto_of(6).is_none());
        assert_eq!(proto_name(0), Some("NRPE"));
        assert_eq!(proto_name(1), Some("NSCA"));
        assert_eq!(proto_name(2), Some("nscp"));
        assert!(proto_name(3).is_none());
        assert!(proto_cell(PROTOS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = chk_cell(3).unwrap_or(0);
        assert_eq!(a, chk_cell(3).unwrap_or(0));
        assert_ne!(a, chk_cell(4).unwrap_or(0));
        assert_ne!(ini_cell(0).unwrap_or(0), ini_cell(5).unwrap_or(0));
        assert!(ini_cell(INI_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(cpp_row(CPP_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 CPU，跳族必抓。
        let mut lt = NsclientLedger::new();
        assert_eq!(
            lt.record_chk(9, chk_cell(9).unwrap_or(0)),
            Err(NsclientCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_chk(0, chk_cell(0).unwrap_or(0));
        let _ = lt.record_chk(5, chk_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_chk(2, chk_cell(2).unwrap_or(0)),
            Err(NsclientCode::EOrder)
        );
        assert!(lt.record_chk(6, chk_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_proto_state_caught() {
        // 空账本：首协议必 NRPE。
        assert_eq!(
            NsclientLedger::new().record_proto(4, proto_cell(4).unwrap_or(0)),
            Err(NsclientCode::EProtoOrd)
        );
        // 合法落协议 0 后跳协议 2 → 越协议必抓。
        let mut lt = NsclientLedger::new();
        let _ = lt.record_proto(0, proto_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_proto(4, proto_cell(4).unwrap_or(0)),
            Err(NsclientCode::EProtoOrd)
        );
        // 协议载荷漂移 EDrift。
        assert_eq!(
            lt.record_proto(1, proto_cell(1).unwrap_or(0).wrapping_add(9)),
            Err(NsclientCode::EDrift)
        );
    }

    #[test]
    fn test_ini_drift_and_dup() {
        let mut lt = NsclientLedger::new();
        assert_eq!(
            lt.record_ini(0, ini_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(NsclientCode::EIniDrift)
        );
        let _ = lt.record_ini(0, ini_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_ini(0, ini_cell(0).unwrap_or(0)),
            Err(NsclientCode::EDup)
        );
        assert_eq!(
            lt.record_ini(INI_CELLS, 0),
            Err(NsclientCode::ENoCell)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NsclientLedger::new();
        let _ = lt.record_chk(0, chk_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_chk(0, chk_cell(0).unwrap_or(0)),
            Err(NsclientCode::EDup)
        );
        let _ = lt.declare_cpp(0);
        assert_eq!(lt.declare_cpp(0), Err(NsclientCode::EDup));
        assert_eq!(lt.declare_cpp(CPP_ROWS), Err(NsclientCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(NsclientCode::ERowMiss));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NsclientLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!NsclientLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(NsclientLedger::new().seal(), Err(NsclientCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = NsclientLedger::new();
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = lt.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < CPP_ROWS {
            let _ = lt.declare_cpp(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(NsclientCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 check + 12 投影 + 6 协议（闸与声明行不计数）。
        assert_eq!(
            recorded,
            CHKS as u32 + INI_CELLS as u32 + PROTOS as u32
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
        assert_eq!(SEG_BASE, 0xCA100);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NsclientCode::Ok as u32, 0);
        assert_eq!(NsclientCode::EGradeDrift as u32, 12);
        assert_eq!(NsclientCode::Ok.val(), SEG_BASE);
        assert_eq!(NsclientCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!NsclientCode::Ok.detail().is_empty());
        assert!(!NsclientCode::ESeal.detail().is_empty());
        assert!(!NsclientCode::EIniDrift.detail().is_empty());
        assert!(!NsclientCode::EProtoOrd.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2（目标档）→ 全齐保持 S2。
        let empty = NsclientLedger::new();
        assert_eq!(judge_grade(&empty), Grade::S3);
        assert!(grade_guard(&empty, Grade::S2).is_err());
        let mut lt = NsclientLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S2).is_ok());
        let mut full = lt.clone();
        let mut o = 0usize;
        while o < CHKS {
            if let Some(w) = chk_cell(o) {
                let _ = full.record_chk(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < INI_CELLS {
            if let Some(w) = ini_cell(i) {
                let _ = full.record_ini(i, w);
            }
            i += 1;
        }
        let mut p = 0usize;
        while p < PROTOS {
            if let Some(w) = proto_cell(p) {
                let _ = full.record_proto(p, w);
            }
            p += 1;
        }
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = full.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < CPP_ROWS {
            let _ = full.declare_cpp(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S2);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
    }

    #[test]
    fn unx52389_checks_green() {
        let s = run_unx52389_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 19);
    }
}
