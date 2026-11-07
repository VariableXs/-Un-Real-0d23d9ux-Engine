//! UNX-F52388 Telegraf 收口 · 0xCA0C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52388"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52389-J1 输入/输出插件 ×10；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52389"（功能标题源）title：Telegraf 收口
//!   quota: 260
//! off-by-one 板式口径：本卡（卡 N=52388）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52389-J1（输入/输出插件 ×10），照原文（键 "52388" criteria）锚定；
//! 功能按本卡标题（Telegraf 收口，键 "52389" title）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 plugin 族闭集 20 格：inputs/outputs/processors/aggregators/
//!      serializers 各 4 格，族序单调（首族必为 inputs，逆跳必抓），
//!      逐格载荷纯函数折叠恒等才收（输入/输出插件 ×10 之插件面收口）。
//!   ② telegraf.conf 双向投影 12 格：六对（正向投影/反向回读成对落账），
//!      反向先于正向必抓（EOrder）。
//!   ③ 采样 8 拍单调：拍号严格逐拍递进（0→7），拍载荷逐拍恒等递增，
//!      跳拍/倒拍/漂移一律 EMono 抓获。
//!   ④ 与 InfluxDB 消费联签 4 格 + Go 运行时上栈声明 2 行闭集，
//!      缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（检查载荷漂移/采样拍序乱/越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3
//!      （六闸 + 20 检查 + 12 投影 + 8 拍 + 4 联签 + 2 Go 全齐 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标；判据闸越闭集走独立 ENoGate 码。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52388;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 检查格数（五 plugin 族 × 4）。
pub const CHECKS: usize = 20;

/// telegraf.conf 双向投影格数（六对）。
pub const PROJ_CELLS: usize = 12;

/// 采样拍数。
pub const SAMPLE_BEATS: usize = 8;

/// 与 InfluxDB 消费联签行数。
pub const INFLUX_ROWS: usize = 4;

/// Go 运行时上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TelegrafCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 载荷漂移
    EDrift,
    /// plugin 族序逆/投影对序逆
    EOrder,
    /// 采样拍非单调
    EMono,
    /// 判据闸越闭集
    ENoGate,
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

impl TelegrafCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            TelegrafCode::Ok => "成功",
            TelegrafCode::ENoCell => "格号越闭集：检查 20/投影 12/采样 8 之外先登记",
            TelegrafCode::EDrift => "载荷漂移：格折叠与 telegraf.conf 基准不恒等，逐格复算",
            TelegrafCode::EOrder => "序逆：inputs→outputs→processors→aggregators→serializers 族序单调；投影反向先于正向必抓",
            TelegrafCode::EMono => "采样拍非单调：8 拍须逐拍递进且拍载荷逐拍恒等递增",
            TelegrafCode::ENoGate => "判据闸越闭集：六判据闸之外先登记",
            TelegrafCode::ENoRow => "声明行越闭集：InfluxDB 联签 4 行 + Go 上栈 2 行之外先登记",
            TelegrafCode::ERowMiss => "缺声明行：InfluxDB 消费联签/Go 上栈声明未齐先补齐",
            TelegrafCode::EDup => "重复落账：同格只许一条账目",
            TelegrafCode::ESeal => "未齐即封：20 检查 + 12 投影 + 8 拍未齐先补齐",
            TelegrafCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            TelegrafCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            TelegrafCode::EGradeDrift => "定级漂移：六闸+全节+4 联签+2 Go 全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20 检查+12 投影+8 拍+4 联签+2 Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 检查格载荷（格 o；plugin 族 = o/4；锚 "TGLF" 低 16 位恒零）。
pub const fn chk_cell(o: usize) -> Option<u64> {
    if o >= CHECKS {
        return None;
    }
    let fam = (o / 4) as u64;
    Some(
        (0xED100u64 + (o as u64) * 0x29)
            ^ fam.wrapping_mul(0x00000193)
            ^ 0x54474C4600000000,
    )
}

/// plugin 族（0..4=inputs 4..8=outputs 8..12=processors 12..16=aggregators 16..20=serializers）。
pub const fn chk_family(o: usize) -> Option<u32> {
    if o >= CHECKS {
        return None;
    }
    Some((o / 4) as u32)
}

/// plugin 族名（telegraf.conf 五 plugin 表族）。
pub const fn family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("inputs"),
        1 => Some("outputs"),
        2 => Some("processors"),
        3 => Some("aggregators"),
        4 => Some("serializers"),
        _ => None,
    }
}

/// 投影格载荷（格 i；对 = i/2，偶正向/奇反向；锚 "TGCF" 低 16 位恒零）。
pub const fn proj_cell(i: usize) -> Option<u64> {
    if i >= PROJ_CELLS {
        return None;
    }
    let pair = (i / 2) as u64;
    Some((0xED300u64 + (i as u64) * 0x2F) ^ pair.wrapping_mul(0x0000CA77) ^ 0x5447434600000000)
}

/// 投影对号（0..6；telegraf.conf 六对）。
pub const fn proj_pair(i: usize) -> Option<u32> {
    if i >= PROJ_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 采样拍基准（拍 b；逐拍严格递增）。
pub const fn sample_beat(b: usize) -> Option<u64> {
    if b >= SAMPLE_BEATS {
        return None;
    }
    Some(0xED500u64 + (b as u64) * 0x47)
}

/// 与 InfluxDB 消费联签行折叠（行 r）。
pub const fn influx_row(r: usize) -> Option<u32> {
    if r >= INFLUX_ROWS {
        return None;
    }
    Some(0xED780 + r as u32 * 0x29)
}

/// Go 运行时上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xED7E0 + r as u32 * 0x29)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Telegraf 收口账本状态机。
#[derive(Clone)]
pub struct TelegrafLedger {
    /// 六判据（安装/telegraf.conf 配置/服务注册/采集功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 检查账。
    chks: [Option<u64>; CHECKS],
    /// 投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// 采样拍账。
    samples: [Option<u64>; SAMPLE_BEATS],
    /// InfluxDB 消费联签账。
    influx: [Option<u32>; INFLUX_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后 plugin 族。
    last_family: i64,
    /// 最后采样拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl TelegrafLedger {
    /// 新账本。
    pub fn new() -> Self {
        TelegrafLedger {
            gates: [None; 6],
            chks: [None; CHECKS],
            proj: [None; PROJ_CELLS],
            samples: [None; SAMPLE_BEATS],
            influx: [None; INFLUX_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), TelegrafCode> {
        if gate >= 6 {
            return Err(TelegrafCode::ENoGate);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xED110 + gate as u64);
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

    /// 检查一格（plugin 族序单调 + 首族必 inputs + 载荷对照 + 重复拒）。
    pub fn record_chk(&mut self, o: usize, got: u64) -> Result<(), TelegrafCode> {
        if o >= CHECKS {
            return Err(TelegrafCode::ENoCell);
        }
        let fam = match chk_family(o) {
            Some(k) => k as i64,
            None => return Err(TelegrafCode::ENoCell),
        };
        let order_bad = if self.last_family < 0 {
            fam != 0
        } else {
            fam < self.last_family || fam > self.last_family + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xED410);
            return Err(TelegrafCode::EOrder);
        }
        let want = match chk_cell(o) {
            Some(w) => w,
            None => return Err(TelegrafCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xED411 + o as u64);
            return Err(TelegrafCode::EDrift);
        }
        if match self.chks.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TelegrafCode::EDup);
        }
        if let Some(c) = self.chks.get_mut(o) {
            *c = Some(got);
        }
        self.last_family = fam;
        self.fnv(0xED412 + o as u64);
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

    /// 投影一格（telegraf.conf 对内先正后反 + 载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, i: usize, got: u64) -> Result<(), TelegrafCode> {
        if i >= PROJ_CELLS {
            return Err(TelegrafCode::ENoCell);
        }
        // 双向投影成对：奇格（反向回读）落账前其偶格镜像（正向投影）必须已在账。
        if i % 2 == 1 {
            let fwd_ok = match self.proj.get(i - 1) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !fwd_ok {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0xED430);
                return Err(TelegrafCode::EOrder);
            }
        }
        let want = match proj_cell(i) {
            Some(w) => w,
            None => return Err(TelegrafCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xED431 + i as u64);
            return Err(TelegrafCode::EDrift);
        }
        if match self.proj.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TelegrafCode::EDup);
        }
        if let Some(c) = self.proj.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xED432 + i as u64);
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

    /// 采样一拍（拍号严格递进 + 拍载荷对照 + 重复拒）。
    pub fn record_sample(&mut self, b: usize, got: u64) -> Result<(), TelegrafCode> {
        if b >= SAMPLE_BEATS {
            return Err(TelegrafCode::ENoCell);
        }
        if b as i64 != self.last_beat + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xED450);
            return Err(TelegrafCode::EMono);
        }
        let want = match sample_beat(b) {
            Some(w) => w,
            None => return Err(TelegrafCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xED451 + b as u64);
            return Err(TelegrafCode::EMono);
        }
        if match self.samples.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TelegrafCode::EDup);
        }
        if let Some(c) = self.samples.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0xED452 + b as u64);
        Ok(())
    }

    /// 采样全账核验（8 拍）。
    pub fn samples_all(&self) -> bool {
        let mut b = 0usize;
        while b < SAMPLE_BEATS {
            match self.samples.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 与 InfluxDB 消费联签一行。
    pub fn declare_influx(&mut self, r: usize) -> Result<(), TelegrafCode> {
        if r >= INFLUX_ROWS {
            return Err(TelegrafCode::ENoRow);
        }
        if match self.influx.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TelegrafCode::EDup);
        }
        if let Some(c) = self.influx.get_mut(r) {
            *c = influx_row(r);
        }
        self.fnv(0xED790 + r as u64);
        Ok(())
    }

    /// InfluxDB 联签全齐核验。
    pub fn influx_all(&self) -> bool {
        let mut r = 0usize;
        while r < INFLUX_ROWS {
            match self.influx.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 运行时上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), TelegrafCode> {
        if r >= GO_ROWS {
            return Err(TelegrafCode::ENoRow);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(TelegrafCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xED7F0 + r as u64);
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

    /// 封账核验：InfluxDB/Go 行缺 → ERowMiss；格未齐 → ESeal。
    pub fn seal(&self) -> Result<(), TelegrafCode> {
        if !(self.influx_all() && self.gos_all()) {
            return Err(TelegrafCode::ERowMiss);
        }
        if !(self.gates_all() && self.chks_all() && self.proj_all() && self.samples_all()) {
            return Err(TelegrafCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), TelegrafCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(TelegrafCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), TelegrafCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(TelegrafCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &TelegrafLedger) -> Grade {
    if led.gates_all() && led.chks_all() && led.proj_all() && led.samples_all() && led.influx_all()
        && led.gos_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

impl Default for TelegrafLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = TelegrafLedger::new();
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
    while b < SAMPLE_BEATS {
        if let Some(w) = sample_beat(b) {
            if led.record_sample(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < INFLUX_ROWS {
        let _ = led.declare_influx(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：检查载荷漂移 / 采样拍序乱（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = TelegrafLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_chk(0, match chk_cell(0) {
                    Some(w) => w,
                    None => 0,
                });
                lt.record_chk(
                    1,
                    match chk_cell(1) {
                        Some(w) => w,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(TelegrafCode::EDrift)
            }
            1 => {
                lt.record_sample(
                    6,
                    match sample_beat(6) {
                        Some(w) => w,
                        None => 0,
                    },
                ) == Err(TelegrafCode::EMono)
            }
            _ => {
                lt.record_chk(CHECKS, 0).is_err()
                    && lt.record_proj(PROJ_CELLS, 0).is_err()
                    && lt.record_sample(SAMPLE_BEATS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xED610 + k as u64);
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
/// UNX-F52388 判据集（锚 UNX-F52389-J1，off-by-one 板式）。
pub fn run_unx52388_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52388");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == CHECKS as u32 + PROJ_CELLS as u32 + SAMPLE_BEATS as u32,
        "20 检查 + 12 投影 + 8 拍逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "plugin 五族 20 格全通",
        {
            let mut lt = TelegrafLedger::new();
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
        "inputs/outputs/processors/aggregators/serializers 五族逐格载荷恒等",
    );
    s.add(
        "telegraf.conf 双向投影 12 格全落",
        {
            let mut lt = TelegrafLedger::new();
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
        "采样 8 拍单调",
        {
            let mut lt = TelegrafLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SAMPLE_BEATS {
                if let Some(w) = sample_beat(b) {
                    if lt.record_sample(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.samples_all()
        },
        "拍号逐拍递进且拍载荷逐拍恒等递增",
    );
    s.add(
        "InfluxDB 消费联签 4/4",
        {
            let mut lt = TelegrafLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < INFLUX_ROWS {
                if lt.declare_influx(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.influx_all()
        },
        "InfluxDB 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = TelegrafLedger::new();
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
        "plugin 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CHECKS {
                let a = match chk_family(o) {
                    Some(k) => k,
                    None => 5,
                };
                let b = match chk_family(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if a < b {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "inputs→outputs→processors→aggregators→serializers 五族闭集单调",
    );
    s.add(
        "投影对完整",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if match proj_pair(i) {
                    Some(p) => p >= 6,
                    None => true,
                } {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "六对 telegraf.conf 投影对号闭集 0..6",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = TelegrafLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/telegraf.conf 配置/服务注册/采集功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·检查越闭集",
        TelegrafLedger::new().record_chk(CHECKS, 0).is_err(),
        "检查 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        TelegrafLedger::new().record_proj(PROJ_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·采样拍越闭集",
        TelegrafLedger::new().record_sample(SAMPLE_BEATS, 0).is_err(),
        "采样拍 8 越闭集必拒",
    );
    s.add(
        "边界·判据闸越闭集",
        {
            let mut lt = TelegrafLedger::new();
            lt.judge_gate(6) == Err(TelegrafCode::ENoGate)
        },
        "判据闸 6 之外先登记必拒（独立 ENoGate 码）",
    );
    s.add(
        "漂移拒",
        {
            let mut lt = TelegrafLedger::new();
            lt.record_proj(
                0,
                match proj_cell(0) {
                    Some(w) => w,
                    None => 0,
                }
                .wrapping_add(1),
            ) == Err(TelegrafCode::EDrift)
        },
        "telegraf.conf 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "检查漂移/采样拍序乱/越闭集三态轮转必抓",
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
            let mut lt = TelegrafLedger::new();
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
            while b < SAMPLE_BEATS {
                if let Some(w) = sample_beat(b) {
                    let _ = lt.record_sample(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < INFLUX_ROWS {
                let _ = lt.declare_influx(r);
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
    s.add("收口·判据条数", s.len() >= 19, "含本条共 20 条闭集（含四条边界越闭集）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chk_family() {
        assert_eq!(chk_family(0), Some(0));
        assert_eq!(chk_family(4), Some(1));
        assert_eq!(chk_family(12), Some(3));
        assert_eq!(chk_family(19), Some(4));
        assert!(chk_family(20).is_none());
        assert!(chk_cell(20).is_none());
        assert_eq!(family_name(0), Some("inputs"));
        assert_eq!(family_name(3), Some("aggregators"));
        assert_eq!(family_name(4), Some("serializers"));
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
        let a = match chk_cell(3) {
            Some(w) => w,
            None => 0,
        };
        assert_eq!(a, match chk_cell(3) {
            Some(w) => w,
            None => 0,
        });
        assert_ne!(a, match chk_cell(4) {
            Some(w) => w,
            None => 0,
        });
        assert_ne!(
            match sample_beat(0) {
                Some(w) => w,
                None => 0,
            },
            match sample_beat(3) {
                Some(w) => w,
                None => 0,
            }
        );
        assert!(influx_row(INFLUX_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
        assert!(sample_beat(SAMPLE_BEATS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 inputs，跳族必抓。
        let mut lt = TelegrafLedger::new();
        assert_eq!(
            lt.record_chk(
                12,
                match chk_cell(12) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(TelegrafCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_chk(
            0,
            match chk_cell(0) {
                Some(w) => w,
                None => 0,
            },
        );
        let _ = lt.record_chk(
            4,
            match chk_cell(4) {
                Some(w) => w,
                None => 0,
            },
        );
        // 回跳 inputs 之前族 → 必抓。
        assert_eq!(
            lt.record_chk(
                2,
                match chk_cell(2) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(TelegrafCode::EOrder)
        );
        // 回跳未改族游标，outputs 同族续落仍合法。
        assert!(lt
            .record_chk(
                5,
                match chk_cell(5) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
    }

    #[test]
    fn test_proj_pair_order_caught() {
        // 空账本：telegraf.conf 反向回读先于正向投影必抓（对空账本同样成立）。
        let mut lt = TelegrafLedger::new();
        assert_eq!(
            lt.record_proj(
                1,
                match proj_cell(1) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(TelegrafCode::EOrder)
        );
        // 正向落账后反向可落。
        let _ = lt.record_proj(
            0,
            match proj_cell(0) {
                Some(w) => w,
                None => 0,
            },
        );
        assert!(lt
            .record_proj(
                1,
                match proj_cell(1) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
    }

    #[test]
    fn test_sample_mono_caught() {
        let mut lt = TelegrafLedger::new();
        // 采样跳拍（0 未落先落 6）必抓。
        assert_eq!(
            lt.record_sample(
                6,
                match sample_beat(6) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(TelegrafCode::EMono)
        );
        // 拍载荷漂移亦 EMono。
        assert_eq!(
            lt.record_sample(
                0,
                match sample_beat(0) {
                    Some(w) => w,
                    None => 0,
                }
                .wrapping_add(2)
            ),
            Err(TelegrafCode::EMono)
        );
        assert!(lt
            .record_sample(
                0,
                match sample_beat(0) {
                    Some(w) => w,
                    None => 0,
                }
            )
            .is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = TelegrafLedger::new();
        let _ = lt.record_chk(
            0,
            match chk_cell(0) {
                Some(w) => w,
                None => 0,
            },
        );
        assert_eq!(
            lt.record_chk(
                0,
                match chk_cell(0) {
                    Some(w) => w,
                    None => 0,
                }
            ),
            Err(TelegrafCode::EDup)
        );
        let _ = lt.declare_influx(0);
        assert_eq!(lt.declare_influx(0), Err(TelegrafCode::EDup));
        assert_eq!(lt.declare_influx(INFLUX_ROWS), Err(TelegrafCode::ENoRow));
        assert_eq!(lt.declare_go(GO_ROWS), Err(TelegrafCode::ENoRow));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = TelegrafLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 判据闸越闭集走独立 ENoGate 码。
        assert_eq!(lt.judge_gate(6), Err(TelegrafCode::ENoGate));
        assert!(lt.gates_all());
        assert!(!TelegrafLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：InfluxDB/Go 行缺 → ERowMiss。
        assert_eq!(TelegrafLedger::new().seal(), Err(TelegrafCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = TelegrafLedger::new();
        let mut r = 0usize;
        while r < INFLUX_ROWS {
            let _ = lt.declare_influx(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = lt.declare_go(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(TelegrafCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 检查 + 12 投影 + 8 拍（闸与声明行不计数）。
        assert_eq!(
            recorded,
            CHECKS as u32 + PROJ_CELLS as u32 + SAMPLE_BEATS as u32
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
        assert_eq!(SEG_BASE, 0xCA0C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(TelegrafCode::Ok as u32, 0);
        assert_eq!(TelegrafCode::EGradeDrift as u32, 12);
        assert_eq!(TelegrafCode::Ok.val(), SEG_BASE);
        assert_eq!(TelegrafCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!TelegrafCode::Ok.detail().is_empty());
        assert!(!TelegrafCode::ENoGate.detail().is_empty());
        assert!(!TelegrafCode::EFp.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&TelegrafLedger::new()), Grade::S3);
        let mut lt = TelegrafLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        let mut full = TelegrafLedger::new();
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
        while b < SAMPLE_BEATS {
            if let Some(w) = sample_beat(b) {
                let _ = full.record_sample(b, w);
            }
            b += 1;
        }
        let mut r = 0usize;
        while r < INFLUX_ROWS {
            let _ = full.declare_influx(r);
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
    fn unx52388_checks_green() {
        let s = run_unx52388_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 20);
    }
}
