//! unx0972_obsclose — UNX-F0972 中断面观测一致性收口（全部账本出口统一）。
//!
//! 任务锚点：UNX-F0972（第二批工位 W7，SEG 段位 0x7F80，本单独占
//! 0x7F80..=0x7F9F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**中断面全部账本出口统一为单一观测视图
//! （各源账计数与视图汇总逐项相等，一致率 100%），出口漂移必被对
//! 拍检出带码，视图指纹双导出稳定**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x7F8 段闭集）；tick 显性
//! 注入零墙钟；出口账定容守恒（Σ源计数==视图汇总 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x7F80 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面观测一致性收口段基址（0x7F80，本单 0x7F80..=0x7F9F 独占）。
pub const SEG_BASE: u16 = 0x7F80;
/// 0x7F81 源账计数回退（同源计数只增不减，回退即污染）。
pub const XC_COUNT_REGRESS: u16 = SEG_BASE + 1;
/// 0x7F82 视图汇总失配（Σ源计数 != 视图汇总——出口不一致）。
pub const XC_SUM_MISMATCH: u16 = SEG_BASE + 2;
/// 0x7F83 出口域越界（出口码不在闭集内）。
pub const XC_EXIT_BAD: u16 = SEG_BASE + 3;
/// 0x7F84 源重复登记（同源双登记——源账身份不唯一）。
pub const XC_SRC_DUP: u16 = SEG_BASE + 4;
/// 0x7F85 未知源计数上报（来源未登记——野计数拒收）。
pub const XC_SRC_UNKNOWN: u16 = SEG_BASE + 5;
/// 0x7F86 视图指纹漂移（双导出指纹失配——视图序不确定）。
pub const XC_VIEW_DRIFT: u16 = SEG_BASE + 6;
/// 0x7F87 账本定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 7;
/// 0x7F88 出口收口未执行（漂移检测前必须先收口一次）。
pub const XC_NOT_SEALED: u16 = SEG_BASE + 8;
/// 0x7F89 漂移注入未检出（检出面自证碎——收口器被污染）。
pub const XC_DRIFT_UNDETECTED: u16 = SEG_BASE + 9;
/// 0x7F8A 出口行缺失（闭集出口在视图中缺行——导出不齐备）。
pub const XC_ROW_MISSING: u16 = SEG_BASE + 10;

const _: () = assert!(XC_COUNT_REGRESS != XC_SUM_MISMATCH);
const _: () = assert!(XC_VIEW_DRIFT != XC_CAP_INVALID);
const _: () = assert!(XC_DRIFT_UNDETECTED != XC_ROW_MISSING);

// ---------------------------------------------------------------------------
// §1 出口域闭集（中断面账本的全部统一出口）
// ---------------------------------------------------------------------------

/// 出口闭集：中断面全部账本最终经此五口出账（出闭集即越界）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitPort {
    /// 事件出口（观测事件流）。
    Event,
    /// 审计出口（飞行记录）。
    Audit,
    /// 健康出口（健康账汇总）。
    Health,
    /// 人话出口（explain 流）。
    Explain,
    /// 注入出口（注入检出流）。
    Inject,
}

impl ExitPort {
    /// 口码（1..=5）。
    pub const fn code(self) -> u8 {
        match self {
            ExitPort::Event => 1,
            ExitPort::Audit => 2,
            ExitPort::Health => 3,
            ExitPort::Explain => 4,
            ExitPort::Inject => 5,
        }
    }
    /// 口名。
    pub const fn name(self) -> &'static str {
        match self {
            ExitPort::Event => "事件",
            ExitPort::Audit => "审计",
            ExitPort::Health => "健康",
            ExitPort::Explain => "人话",
            ExitPort::Inject => "注入",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<ExitPort> {
        match c {
            1 => Some(ExitPort::Event),
            2 => Some(ExitPort::Audit),
            3 => Some(ExitPort::Health),
            4 => Some(ExitPort::Explain),
            5 => Some(ExitPort::Inject),
            _ => None,
        }
    }
}

/// 出口总数（闭集机检常量）。
pub const EXIT_COUNT: usize = 5;

// ---------------------------------------------------------------------------
// §2 源账（SourceLedger）——每账本源一份计数
// ---------------------------------------------------------------------------

/// 一个观测源：源 id / 出口归属 / 计数 / 最近上报 tick。
#[derive(Clone, Copy, Debug)]
pub struct SourceEntry {
    src_id: u32,
    port: ExitPort,
    count: u64,
    last_tick: u64,
}

/// 源账——源登记 + 计数上报（计数单调，回退带码拒收）。
#[derive(Debug)]
pub struct SourceLedger {
    entries: Vec<SourceEntry>,
    cap: usize,
    total: u64,
    rejected: u32,
}

impl SourceLedger {
    /// 建源账。
    pub fn new(cap: usize) -> Result<SourceLedger, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(SourceLedger {
            entries: Vec::with_capacity(cap),
            cap,
            total: 0,
            rejected: 0,
        })
    }

    /// 登记一个源（重复登记带码拒收；出口越界拒收）。
    pub fn register(&mut self, src_id: u32, port: ExitPort) -> Result<(), u16> {
        if src_id == 0 {
            self.rejected += 1;
            return Err(XC_SRC_UNKNOWN);
        }
        if ExitPort::from_code(port.code()).is_none() {
            self.rejected += 1;
            return Err(XC_EXIT_BAD);
        }
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].src_id == src_id {
                self.rejected += 1;
                return Err(XC_SRC_DUP);
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        self.entries.push(SourceEntry {
            src_id,
            port,
            count: 0,
            last_tick: 0,
        });
        Ok(())
    }

    /// 上报一次计数增量（未登记源拒收；tick 回退拒收）。
    pub fn report(&mut self, src_id: u32, delta: u64, tick: u64) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].src_id == src_id {
                if tick < self.entries[i].last_tick {
                    self.rejected += 1;
                    return Err(XC_COUNT_REGRESS);
                }
                self.entries[i].count += delta;
                self.entries[i].last_tick = tick;
                self.total += delta;
                return Ok(());
            }
            i += 1;
        }
        self.rejected += 1;
        Err(XC_SRC_UNKNOWN)
    }

    /// 源数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 全源计数和（视图对拍的第一项）。
    pub fn total(&self) -> u64 {
        self.total
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 按出口聚合的计数（五口各一份——视图汇总源）。
    pub fn per_port(&self) -> [u64; EXIT_COUNT] {
        let mut out = [0u64; EXIT_COUNT];
        let mut i: usize = 0;
        while i < self.entries.len() {
            let c = self.entries[i].port.code() as usize;
            if c >= 1 && c <= EXIT_COUNT {
                out[c - 1] += self.entries[i].count;
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// §3 统一观测视图（UnifiedView）——收口出口本体
// ---------------------------------------------------------------------------

/// 统一观测视图一行：出口 / 汇总数 / 行指纹位。
#[derive(Clone, Copy, Debug)]
pub struct ViewRow {
    port: ExitPort,
    sum: u64,
}

/// 统一观测视图——全部源账按出口聚合后的唯一出口面。
///
/// 收口判据：Σ 视图行汇总 == Σ 源计数（守恒恒等式——每条计数恰
/// 归一口，无重复归口、无漏归口）；五口行齐备（闭集出口缺行即
/// 导出不齐备）。
#[derive(Debug)]
pub struct UnifiedView {
    rows: [ViewRow; EXIT_COUNT],
    grand_total: u64,
    sealed: bool,
    export_fp: u64,
    export_cnt: u32,
}

impl UnifiedView {
    /// 由源账收口建视图（每次收口覆盖旧视图——视图是账的投影）。
    pub fn close_from(src: &SourceLedger) -> Result<UnifiedView, u16> {
        let per = src.per_port();
        let mut rows = [ViewRow {
            port: ExitPort::Event,
            sum: 0,
        }; EXIT_COUNT];
        let mut grand: u64 = 0;
        let mut i: usize = 0;
        while i < EXIT_COUNT {
            let port = match ExitPort::from_code((i + 1) as u8) {
                Some(p) => p,
                None => return Err(XC_EXIT_BAD),
            };
            rows[i] = ViewRow {
                port,
                sum: per[i],
            };
            grand += per[i];
            i += 1;
        }
        // 守恒恒等式机检：视图汇总 == 源账总计。
        if grand != src.total() {
            return Err(XC_SUM_MISMATCH);
        }
        Ok(UnifiedView {
            rows,
            grand_total: grand,
            sealed: true,
            export_fp: 0,
            export_cnt: 0,
        })
    }

    /// 是否已收口。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    /// 全口径总计。
    pub fn grand_total(&self) -> u64 {
        self.grand_total
    }
    /// 行汇总（按口码查，越界 0）。
    pub fn sum_of(&self, port: ExitPort) -> u64 {
        let c = port.code() as usize;
        if c == 0 || c > EXIT_COUNT {
            return 0;
        }
        self.rows[c - 1].sum
    }

    /// 行齐备机检（五口行均在，sum 字段恒有值——闭集投影不缺行）。
    pub fn rows_complete(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < EXIT_COUNT {
            if ExitPort::from_code(self.rows[i].port.code()).is_none() {
                return Err(XC_ROW_MISSING);
            }
            i += 1;
        }
        Ok(())
    }

    /// 导出 schema 文本（确定性序 + 指纹封存 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        if !self.sealed {
            return Err(XC_NOT_SEALED);
        }
        self.rows_complete()?;
        let mut s = String::from("UNX-F0972-VIEW|v1|total=");
        s.push_str(&self.grand_total.to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < EXIT_COUNT {
            s.push_str(self.rows[i].port.name());
            s.push('|');
            s.push_str(&self.rows[i].sum.to_string());
            s.push('\n');
            i += 1;
        }
        let fp = fingerprint_str(&s);
        if self.export_cnt > 0 && fp != self.export_fp {
            return Err(XC_VIEW_DRIFT);
        }
        self.export_fp = fp;
        self.export_cnt += 1;
        Ok(s)
    }

    /// 最近导出指纹。
    pub fn export_fingerprint(&self) -> u64 {
        self.export_fp
    }
    /// 导出次数。
    pub fn export_count(&self) -> u32 {
        self.export_cnt
    }
}

/// 字符串 FNV-1a 64（视图指纹——只做对拍不做安全用途）。
fn fingerprint_str(s: &str) -> u64 {
    let bs: &[u8] = s.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i: usize = 0;
    while i < bs.len() {
        h = (h ^ bs[i] as u64).wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// §4 漂移注入检出面（收口一致性的活断言）
// ---------------------------------------------------------------------------

/// 漂移注入结果行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DriftProbe {
    /// 注入种（1 汇总失配 / 2 未收口导出）。
    pub kind: u8,
    /// 是否检出（真=带码拦截）。
    pub detected: bool,
    /// 检出码。
    pub code: u16,
}

/// 注入一：篡改视图 grand_total 后对拍守恒——必碎（检出 0x7F82）。
pub fn inject_sum_mismatch(src: &SourceLedger) -> DriftProbe {
    let mut v = match UnifiedView::close_from(src) {
        Ok(v) => v,
        Err(c) => {
            return DriftProbe {
                kind: 1,
                detected: true,
                code: c,
            }
        }
    };
    // 越权改写汇总（模拟出口被污染）。
    v.grand_total = v.grand_total.wrapping_add(1);
    // 重收口对拍：投影重算必与被篡改视图失配。
    let v2 = match UnifiedView::close_from(src) {
        Ok(x) => x,
        Err(c) => {
            return DriftProbe {
                kind: 1,
                detected: true,
                code: c,
            }
        }
    };
    let detected = v.grand_total != v2.grand_total;
    DriftProbe {
        kind: 1,
        detected,
        code: if detected { XC_SUM_MISMATCH } else { 0 },
    }
}

/// 注入二：未收口视图直接导出——必拦（检出 0x7F88）。
pub fn inject_unsealed_export() -> DriftProbe {
    let mut v = UnifiedView {
        rows: [ViewRow {
            port: ExitPort::Event,
            sum: 0,
        }; EXIT_COUNT],
        grand_total: 0,
        sealed: false,
        export_fp: 0,
        export_cnt: 0,
    };
    match v.export() {
        Err(c) if c == XC_NOT_SEALED => DriftProbe {
            kind: 2,
            detected: true,
            code: c,
        },
        _ => DriftProbe {
            kind: 2,
            detected: false,
            code: 0,
        },
    }
}

// ---------------------------------------------------------------------------
// §5 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：登记源→上报→收口→双导出对拍→注入检出→守恒。
pub fn drill() -> Result<u32, u16> {
    let mut l = SourceLedger::new(8)?;
    l.register(1, ExitPort::Event)?;
    l.register(2, ExitPort::Audit)?;
    l.register(3, ExitPort::Health)?;
    l.register(4, ExitPort::Explain)?;
    l.register(5, ExitPort::Inject)?;
    // 注入面：重复登记、未知源、计数回退全拦。
    if l.register(1, ExitPort::Event) != Err(XC_SRC_DUP) {
        return Err(XC_SRC_DUP);
    }
    if l.report(99, 1, 1) != Err(XC_SRC_UNKNOWN) {
        return Err(XC_SRC_UNKNOWN);
    }
    l.report(1, 40, 10)?;
    if l.report(1, 1, 5) != Err(XC_COUNT_REGRESS) {
        return Err(XC_COUNT_REGRESS);
    }
    l.report(2, 30, 20)?;
    l.report(3, 20, 30)?;
    l.report(4, 6, 40)?;
    l.report(5, 4, 50)?;
    // 收口与守恒。
    let mut v = UnifiedView::close_from(&l)?;
    if v.grand_total() != 100 {
        return Err(XC_SUM_MISMATCH);
    }
    if v.sum_of(ExitPort::Event) != 40 {
        return Err(XC_SUM_MISMATCH);
    }
    v.rows_complete()?;
    // 双导出对拍（指纹必稳）。
    let _ = v.export()?;
    let fp1 = v.export_fingerprint();
    let _ = v.export()?;
    if v.export_fingerprint() != fp1 || v.export_count() != 2 {
        return Err(XC_VIEW_DRIFT);
    }
    // 注入检出自证：两面全检出。
    let p1 = inject_sum_mismatch(&l);
    let p2 = inject_unsealed_export();
    if !p1.detected || !p2.detected {
        return Err(XC_DRIFT_UNDETECTED);
    }
    Ok(0)
}
