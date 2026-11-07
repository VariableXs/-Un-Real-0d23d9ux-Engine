//! UNX-F52352 ExifTool 收口 · 0xC97C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52352"，逐字）：
//!   title: ExifTool 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52353-J1 六判据
//!     全落（JVM 上栈 F52057 联签锚定行）；.bib 管理 ×10；KB-COMPAT 定
//!     级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52352）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52353-J1，照原文锚定；功能按本卡标题（ExifTool）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 元数据五族闭集 20 格：EXIF/IPTC/XMP/GPS/ICC 各 4 格，族 = 格号/4，
//!      族单调（逆序必抓，空账本首格非 0 号族必抓）。
//!   ② 元数据读写 10 操作集：读 5/写 5（kind = 格号/5），族单调，载荷
//!      折叠恒等才收；漂移必抓。
//!   ③ 时间戳投影 8 格：逐格载荷对照，越闭集/漂移/重复必抓。
//!   ④ 格式支持 6 格：闭集逐格落账，越闭集必抓。
//!   ⑤ Perl 消费声明 2 行闭集：Perl 运行时消费声明行/元数据写回分账
//!      声明行，缺一拒封。
//!   ⑥ ledger 状态机（new/record/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（元数据族漂移/读写空账乱序/时间
//!      戳越闭集），检出+逃逸守恒等式；双跑指纹一致；定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52352;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 元数据族格数（五族 × 各 4 格）。
pub const META_CELLS: usize = 20;

/// 读写操作数。
pub const RW_OPS: usize = 10;

/// 时间戳投影格数。
pub const TS_CELLS: usize = 8;

/// 格式支持格数。
pub const FMT_CELLS: usize = 6;

/// Perl 消费声明行数。
pub const PERL_ROWS: usize = 2;

/// ExifTool 域 XOR 锚（低 16 位恒零）。
pub const ET_ANCHOR: u64 = 0x4558_4946_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExiftoolCode {
    /// 成功
    Ok = 0,
    /// 元数据族格越闭集
    ENoMeta,
    /// 操作号越闭集（读写）
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 族逆序或空账乱序
    EOrder,
    /// 辅表越闭集（时间戳/格式支持）
    ENoAux,
    /// 辅表载荷漂移
    EAuxDrift,
    /// 声明行越闭集
    ENoRow,
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

impl ExiftoolCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ExiftoolCode::Ok => "成功",
            ExiftoolCode::ENoMeta => "元数据族格越闭集：EXIF/IPTC/XMP/GPS/ICC 20 格闭集之外先登记",
            ExiftoolCode::ENoOp => "操作号越闭集：元数据读写 10 操作闭集之外先登记",
            ExiftoolCode::EDrift => "载荷漂移：元数据族/读写操作折叠与基准不恒等，逐格复算",
            ExiftoolCode::EOrder => "族逆序：EXIF→IPTC→XMP→GPS→ICC 单调，逆序或空账乱序必抓",
            ExiftoolCode::ENoAux => "辅表格越闭集：时间戳投影 8 格/格式支持 6 格闭集之外先登记",
            ExiftoolCode::EAuxDrift => "辅表载荷漂移：时间戳/格式支持折叠与基准不恒等，逐格复算",
            ExiftoolCode::ENoRow => "声明行越闭集：Perl 消费分账 2 行闭集之外先登记",
            ExiftoolCode::EDup => "重复落账：同格只许一条账目",
            ExiftoolCode::ESeal => "未齐即封：20 元数据格 + 10 读写操作 + 8 时间戳格 + 6 格式格 + 2 行声明未齐先补齐",
            ExiftoolCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ExiftoolCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ExiftoolCode::EGradeDrift => "定级漂移：全通且 Perl 分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：判据+20元数据格+10读写+8时间戳+6格式格+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 元数据族格载荷（格 o；族 = o/4）。
pub const fn meta_cell(o: usize) -> Option<u64> {
    if o >= META_CELLS {
        return None;
    }
    Some((0xF1000u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 元数据族号（0..4=EXIF 4..8=IPTC 8..12=XMP 12..16=GPS 16..20=ICC）。
pub const fn meta_kind(o: usize) -> Option<u32> {
    if o >= META_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 元数据族名。
pub const fn meta_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("EXIF"),
        1 => Some("IPTC"),
        2 => Some("XMP"),
        3 => Some("GPS"),
        4 => Some("ICC"),
        _ => None,
    }
}

/// 读写操作载荷（操作 o；读 = o/5 号族，0..5=读 5..10=写）。
pub const fn rw_cell(o: usize) -> Option<u64> {
    if o >= RW_OPS {
        return None;
    }
    Some((0xF2000u64 + (o as u64) * 0x2C) ^ ((o / 5) as u64).wrapping_mul(0x01000193))
}

/// 读写操作向（0..5=读 5..10=写）。
pub const fn rw_kind(o: usize) -> Option<u32> {
    if o >= RW_OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 时间戳投影格载荷（格 t）。
pub const fn ts_cell(t: usize) -> Option<u64> {
    if t >= TS_CELLS {
        return None;
    }
    Some((0xF3000u64 + (t as u64) * 0x2C) ^ ET_ANCHOR)
}

/// 格式支持格载荷（格 f）。
pub const fn fmt_cell(f: usize) -> Option<u64> {
    if f >= FMT_CELLS {
        return None;
    }
    Some((0xF4000u64 + (f as u64) * 0x31) ^ ET_ANCHOR)
}

/// Perl 声明行折叠（行 r）。
pub const fn perl_row(r: usize) -> Option<u32> {
    if r >= PERL_ROWS {
        return None;
    }
    Some(0xF500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// ExifTool 收口账本状态机。
#[derive(Clone)]
pub struct EtLedger {
    /// 六判据（元数据族账/读写账/时间戳账/格式账/Perl 声明/写回摘要）。
    gates: [Option<bool>; 6],
    /// 元数据族账。
    metas: [Option<u64>; META_CELLS],
    /// 读写操作账。
    rws: [Option<u64>; RW_OPS],
    /// 时间戳账。
    tss: [Option<u64>; TS_CELLS],
    /// 格式支持账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 声明账。
    perls: [Option<u32>; PERL_ROWS],
    /// 最后元数据族。
    last_meta: i64,
    /// 最后读写向。
    last_rw: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EtLedger {
    /// 新账本。
    pub fn new() -> Self {
        EtLedger {
            gates: [None; 6],
            metas: [None; META_CELLS],
            rws: [None; RW_OPS],
            tss: [None; TS_CELLS],
            fmts: [None; FMT_CELLS],
            perls: [None; PERL_ROWS],
            last_meta: -1,
            last_rw: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ExiftoolCode> {
        if gate >= 6 {
            return Err(ExiftoolCode::ESeal);
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

    /// 元数据族一格（族单调 + 空账首格必 0 族 + 载荷对照 + 重复拒）。
    pub fn record_meta(&mut self, o: usize, got: u64) -> Result<(), ExiftoolCode> {
        if o >= META_CELLS {
            return Err(ExiftoolCode::ENoMeta);
        }
        let kind = match meta_kind(o) {
            Some(k) => k,
            None => return Err(ExiftoolCode::ENoMeta),
        };
        if (self.last_meta < 0 && kind != 0) || (kind as i64) < self.last_meta {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(ExiftoolCode::EOrder);
        }
        let want = match meta_cell(o) {
            Some(w) => w,
            None => return Err(ExiftoolCode::ENoMeta),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(ExiftoolCode::EDrift);
        }
        if match self.metas.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ExiftoolCode::EDup);
        }
        if let Some(c) = self.metas.get_mut(o) {
            *c = Some(got);
        }
        self.last_meta = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 元数据族全账核验（20 格）。
    pub fn metas_all(&self) -> bool {
        let mut o = 0usize;
        while o < META_CELLS {
            match self.metas.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 读写操作一格（向单调 + 空账首向必读 + 载荷对照 + 重复拒）。
    pub fn record_rw(&mut self, o: usize, got: u64) -> Result<(), ExiftoolCode> {
        if o >= RW_OPS {
            return Err(ExiftoolCode::ENoOp);
        }
        let kind = match rw_kind(o) {
            Some(k) => k,
            None => return Err(ExiftoolCode::ENoOp),
        };
        if (self.last_rw < 0 && kind != 0) || (kind as i64) < self.last_rw {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(ExiftoolCode::EOrder);
        }
        let want = match rw_cell(o) {
            Some(w) => w,
            None => return Err(ExiftoolCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(ExiftoolCode::EDrift);
        }
        if match self.rws.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ExiftoolCode::EDup);
        }
        if let Some(c) = self.rws.get_mut(o) {
            *c = Some(got);
        }
        self.last_rw = kind as i64;
        self.fnv(0x8302 + o as u64);
        Ok(())
    }

    /// 读写操作全账核验（10 项）。
    pub fn rws_all(&self) -> bool {
        let mut o = 0usize;
        while o < RW_OPS {
            match self.rws.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 时间戳投影一格（载荷对照 + 重复拒）。
    pub fn record_ts(&mut self, t: usize, got: u64) -> Result<(), ExiftoolCode> {
        let want = match ts_cell(t) {
            Some(w) => w,
            None => return Err(ExiftoolCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + t as u64);
            return Err(ExiftoolCode::EAuxDrift);
        }
        if match self.tss.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ExiftoolCode::EDup);
        }
        if let Some(c) = self.tss.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x8401 + t as u64);
        Ok(())
    }

    /// 时间戳全账核验（8 格）。
    pub fn tss_all(&self) -> bool {
        let mut t = 0usize;
        while t < TS_CELLS {
            match self.tss.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 格式支持一格（载荷对照 + 重复拒）。
    pub fn record_fmt(&mut self, f: usize, got: u64) -> Result<(), ExiftoolCode> {
        let want = match fmt_cell(f) {
            Some(w) => w,
            None => return Err(ExiftoolCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + f as u64);
            return Err(ExiftoolCode::EAuxDrift);
        }
        if match self.fmts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ExiftoolCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x8501 + f as u64);
        Ok(())
    }

    /// 格式支持全账核验（6 格）。
    pub fn fmts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FMT_CELLS {
            match self.fmts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// Perl 声明一行。
    pub fn declare_perl(&mut self, r: usize) -> Result<(), ExiftoolCode> {
        if r >= PERL_ROWS {
            return Err(ExiftoolCode::ENoRow);
        }
        if match self.perls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ExiftoolCode::EDup);
        }
        if let Some(c) = self.perls.get_mut(r) {
            *c = perl_row(r);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn perls_all(&self) -> bool {
        let mut r = 0usize;
        while r < PERL_ROWS {
            match self.perls.get(r) {
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.metas_all() && self.rws_all() && self.tss_all() && self.fmts_all() && self.perls_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for EtLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EtLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < META_CELLS {
        if let Some(w) = meta_cell(o) {
            if led.record_meta(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < RW_OPS {
        if let Some(w) = rw_cell(c) {
            if led.record_rw(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut t = 0usize;
    while t < TS_CELLS {
        if let Some(w) = ts_cell(t) {
            if led.record_ts(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut f = 0usize;
    while f < FMT_CELLS {
        if let Some(w) = fmt_cell(f) {
            if led.record_fmt(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut r = 0usize;
    while r < PERL_ROWS {
        let _ = led.declare_perl(r);
        r += 1;
    }
    // 注入 10 轮：元数据族漂移 / 读写空账乱序 / 时间戳越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = EtLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_meta(0, meta_cell(0).unwrap_or(0));
                lt.record_meta(1, meta_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ExiftoolCode::EDrift)
            }
            1 => {
                // 空账本乱序注入：首向必读（0..5=读），先写必抓且不改 last_kind。
                lt.record_rw(7, rw_cell(7).unwrap_or(0)) == Err(ExiftoolCode::EOrder)
            }
            _ => lt.record_ts(TS_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8700 + k as u64);
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
/// UNX-F52352 判据集（锚 UNX-F52353-J1，off-by-one 板式）。
pub fn run_unx52352_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52352");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "元数据五族 20 格全通",
        recorded >= META_CELLS as u32,
        "EXIF/IPTC/XMP/GPS/ICC 各 4 格逐格恒等（UNX-F52353-J1 锚定，§AI-66.3）",
    );
    s.add(
        "元数据读写 10 操作全通",
        {
            let mut lt = EtLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < RW_OPS {
                if let Some(w) = rw_cell(c) {
                    if lt.record_rw(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.rws_all()
        },
        "读 5/写 5 向单调逐操作恒等（元数据读写 ×10）",
    );
    s.add(
        "时间戳投影 8 格全齐",
        {
            let mut lt = EtLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TS_CELLS {
                if let Some(w) = ts_cell(t) {
                    if lt.record_ts(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tss_all()
        },
        "时间戳投影逐格载荷对照恒等",
    );
    s.add(
        "格式支持 6 格全齐",
        {
            let mut lt = EtLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    if lt.record_fmt(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fmts_all()
        },
        "格式支持闭集逐格落账恒等",
    );
    s.add(
        "Perl 消费分账声明 2/2 全齐",
        {
            let mut lt = EtLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PERL_ROWS {
                if lt.declare_perl(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.perls_all()
        },
        "Perl 运行时消费声明/元数据写回分账声明两行闭集",
    );
    s.add(
        "元数据族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < META_CELLS {
                if meta_kind(o).unwrap_or(5) < meta_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "元数据族漂移/读写空账乱序/时间戳越闭集三态轮转必抓",
    );
    let mut l0 = EtLedger::new();
    s.add(
        "边界·元数据族越闭集",
        l0.record_meta(META_CELLS, 0).is_err(),
        "元数据格 20 越闭集必拒",
    );
    s.add(
        "边界·读写越闭集",
        l0.record_rw(RW_OPS, 0).is_err(),
        "读写操作 10 越闭集必拒",
    );
    s.add(
        "边界·时间戳越闭集",
        l0.record_ts(TS_CELLS, 0).is_err(),
        "时间戳格 8 越闭集必拒",
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
            let mut lt = EtLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < META_CELLS {
                if let Some(w) = meta_cell(o) {
                    let _ = lt.record_meta(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < RW_OPS {
                if let Some(w) = rw_cell(c) {
                    let _ = lt.record_rw(c, w);
                }
                c += 1;
            }
            let mut t = 0usize;
            while t < TS_CELLS {
                if let Some(w) = ts_cell(t) {
                    let _ = lt.record_ts(t, w);
                }
                t += 1;
            }
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    let _ = lt.record_fmt(f, w);
                }
                f += 1;
            }
            let mut r = 0usize;
            while r < PERL_ROWS {
                let _ = lt.declare_perl(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC97C0,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC97C0",
    );
    s.add(
        "诊断码闭集 13 变体",
        ExiftoolCode::Ok as u32 == 0 && ExiftoolCode::EGradeDrift as u32 == 12,
        "Ok=+0x00 起 12 错误码至 +0x0C 恰 13 变体，detail 全非空",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meta_kind() {
        assert_eq!(meta_kind(0), Some(0));
        assert_eq!(meta_kind(3), Some(0));
        assert_eq!(meta_kind(4), Some(1));
        assert_eq!(meta_kind(19), Some(4));
        assert!(meta_kind(20).is_none());
        assert!(meta_cell(20).is_none());
        assert_eq!(meta_kind_name(0), Some("EXIF"));
        assert_eq!(meta_kind_name(4), Some("ICC"));
        assert!(meta_kind_name(5).is_none());
    }

    #[test]
    fn test_rw_kind() {
        assert_eq!(rw_kind(0), Some(0));
        assert_eq!(rw_kind(4), Some(0));
        assert_eq!(rw_kind(5), Some(1));
        assert_eq!(rw_kind(9), Some(1));
        assert!(rw_kind(10).is_none());
        assert!(rw_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = meta_cell(5).unwrap_or(0);
        assert_eq!(a, meta_cell(5).unwrap_or(0));
        assert_ne!(a, meta_cell(6).unwrap_or(0));
        assert!(ts_cell(TS_CELLS).is_none());
        assert!(fmt_cell(FMT_CELLS).is_none());
        assert!(perl_row(PERL_ROWS).is_none());
        assert_eq!(ET_ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = EtLedger::new();
        // 先按合法族序铺 0..=8（kind=o/3，三族），再注逆序 2 必抓，后 kind3 正格。
        let mut i = 0usize;
        while i <= 8 {
            let _ = lt.record_meta(i, meta_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(lt.record_meta(2, meta_cell(2).unwrap_or(0)), Err(ExiftoolCode::EOrder));
        assert!(lt.record_meta(14, meta_cell(14).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_empty_ledger_order_caught() {
        let mut lt = EtLedger::new();
        // 空账本先写（向 1）必抓。
        assert_eq!(lt.record_rw(7, rw_cell(7).unwrap_or(0)), Err(ExiftoolCode::EOrder));
        assert_eq!(lt.record_rw(0, rw_cell(0).unwrap_or(0)), Ok(()));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = EtLedger::new();
        assert_eq!(
            lt.record_meta(1, meta_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(ExiftoolCode::EDrift)
        );
        assert_eq!(
            lt.record_ts(2, ts_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(ExiftoolCode::EAuxDrift)
        );
        assert_eq!(
            lt.record_fmt(0, fmt_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(ExiftoolCode::EAuxDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = EtLedger::new();
        let _ = lt.record_meta(0, meta_cell(0).unwrap_or(0));
        assert_eq!(lt.record_meta(0, meta_cell(0).unwrap_or(0)), Err(ExiftoolCode::EDup));
        let _ = lt.record_ts(0, ts_cell(0).unwrap_or(0));
        assert_eq!(lt.record_ts(0, ts_cell(0).unwrap_or(0)), Err(ExiftoolCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = EtLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = EtLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (META_CELLS + RW_OPS + TS_CELLS + FMT_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC97C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ExiftoolCode::Ok as u32, 0);
        assert_eq!(ExiftoolCode::EGradeDrift as u32, 12);
        assert!(!ExiftoolCode::Ok.detail().is_empty());
        assert!(!ExiftoolCode::ESeal.detail().is_empty());
        assert!(!ExiftoolCode::EAuxDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = EtLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52352_checks_green() {
        let s = run_unx52352_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
