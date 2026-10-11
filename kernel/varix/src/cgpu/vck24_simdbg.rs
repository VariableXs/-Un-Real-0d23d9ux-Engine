//! CGPU-F1705 · 异构模拟器调试支持
//!
//! 判据锚点（施工书原文摘录）：
//! 模拟调试：模拟 trace（模拟器产生同格式 trace（模拟=真机同格式——同格式声明
//! （F1612/F1661 复用；调试对齐（模拟调试面板可用——面板复用；CI（CI trace 产出
//! ——CI 集成；测试（同格式/面板/CI 三组）。判据：同格式、面板、CI、三组、判据。
//!
//! 诊断码段 0x8C01~0x8C05（独占）：
//!   0x8C01 同格式失恒（模拟事件落 schema v1 之外/与真机编码不一致）
//!   0x8C02 面板复用失接（模拟数据源未挂统一框架/口径漂移）
//!   0x8C03 CI 确定性破（同种子两次 run trace 指纹失配）
//!   0x8C04 段越界（判码落 0x8C00..0x8D00 区间外）
//!   0x8C05 保留段哨（本段扩位预留哨兵——不派发）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use crate::cgpu::vck21_tracefmt::{
    encode_event, import, roundtrip_check, EvKind, SCHEMA_VER_V1, TraceEvent,
    EV_REC_LEN,
};
use alloc::vec::Vec;

/// 模块版本。
pub const SIM_DBG_VERSION: &str = "K24-simdbg-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8C01~0x8C05）
// ---------------------------------------------------------------------------

pub const XC_SAMEFORMAT_BROKEN: u32 = 0x8C01;
pub const XC_PANEL_REUSE_BROKEN: u32 = 0x8C02;
pub const XC_CI_NONDET: u32 = 0x8C03;
pub const XC_SEG_OOR: u32 = 0x8C04;
pub const XC_SEG_RESERVED: u32 = 0x8C05;

// ---------------------------------------------------------------------------
// 一、同格式声明：模拟=真机 trace schema v1
// ---------------------------------------------------------------------------

/// 同格式声明（F1612/F1661 复用——源单号非空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SameFormatDecl {
    /// 复用源单号（模拟器 F1612 / 校准两态 F1661）。
    pub origin_sim: &'static str,
    pub origin_calib: &'static str,
    /// schema 版本（与真机 trace 同一 v1）。
    pub schema_ver: u16,
    /// 声明生效。
    pub declared: bool,
}

impl SameFormatDecl {
    pub fn build() -> Self {
        SameFormatDecl {
            origin_sim: "CGPU-F1612",
            origin_calib: "CGPU-F1661",
            schema_ver: SCHEMA_VER_V1,
            declared: true,
        }
    }

    /// 声明校验：生效 + 源单号非空 + 版本与真机一致（0x8C01）。
    pub fn verified(&self) -> Result<(), u32> {
        if !self.declared || self.origin_sim.is_empty() || self.origin_calib.is_empty() {
            return Err(XC_SAMEFORMAT_BROKEN);
        }
        if self.schema_ver != SCHEMA_VER_V1 {
            return Err(XC_SAMEFORMAT_BROKEN);
        }
        Ok(())
    }
}

/// LCG 线性同余（CI 确定性引擎——同种子同序列）。
pub struct Lcg(u64);

impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 16
    }
}

/// 模拟器 trace 生成：LCG 种子驱动，产出与真机同 schema 的事件流
/// （事件字段全为运行时量——无用户数据，复用 vck21 隐私红线闭集）。
pub fn sim_trace_gen(seed: u64, n_events: usize) -> Result<Vec<TraceEvent>, u32> {
    if n_events == 0 || n_events > 4096 {
        return Err(XC_SAMEFORMAT_BROKEN);
    }
    let mut rng = Lcg::new(seed);
    let mut out = Vec::with_capacity(n_events);
    let mut i = 0;
    let mut ts: u64 = 1000;
    while i < n_events {
        let r = rng.next();
        let kind = match r % 3 {
            0 => EvKind::Task,
            1 => EvKind::Engine,
            _ => EvKind::Handoff,
        };
        let task = (r >> 8) % 64 as u64;
        let engine = (r >> 20) % 4 as u64;
        let peer = if kind == EvKind::Handoff { (r >> 32) % 64 as u64 + 1 } else { 0 };
        out.push(TraceEvent {
            kind,
            ts_us: ts,
            task: task as u32,
            engine: engine as u8,
            peer_task: peer as u32,
        });
        ts += 37 + (r >> 40) % 500;
        i += 1;
    }
    Ok(out)
}

/// 同格式对拍：模拟事件经真机编码器出流→真机导入器回读→逐字段恒等。
pub fn same_format_check(events: &[TraceEvent]) -> Result<(), u32> {
    // 字段级：模拟事件必须能过真机语义闸（非交接事件对端=0 由编码器往返保障）。
    for e in events {
        if e.kind != EvKind::Handoff && e.peer_task != 0 {
            return Err(XC_SAMEFORMAT_BROKEN);
        }
        if e.engine > 3 {
            return Err(XC_SAMEFORMAT_BROKEN);
        }
    }
    // 编码往返：模拟流走真机 export/import 恒等。
    let mut bytes = Vec::new();
    let header = crate::cgpu::vck21_tracefmt::SchemaHeader {
        magic: crate::cgpu::vck21_tracefmt::TRACE_MAGIC,
        schema_ver: SCHEMA_VER_V1,
        count: events.len() as u32,
    };
    bytes.extend_from_slice(&header.encode());
    for e in events {
        encode_event(e, &mut bytes);
    }
    let back = import(&bytes)?;
    roundtrip_check(events, &back)?;
    // 定长流长核对。
    if bytes.len() != 10 + events.len() * EV_REC_LEN {
        return Err(XC_SAMEFORMAT_BROKEN);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 二、面板复用：模拟数据源挂统一框架
// ---------------------------------------------------------------------------

/// 面板复用行（判据"模拟调试面板可用——面板复用"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelReuseRow {
    /// 模拟数据源名。
    pub sim_source: &'static str,
    /// 挂接的统一框架口径 id（与 vck20 RegCaliberView 同源）。
    pub caliber_id: &'static str,
    /// 复用在册。
    pub hooked: bool,
}

/// 面板复用账。
pub struct PanelReuseBook {
    rows: [PanelReuseRow; 2],
}

impl PanelReuseBook {
    pub const fn build() -> Self {
        PanelReuseBook {
            rows: [
                PanelReuseRow {
                    sim_source: "sim_npu_batch_utili",
                    caliber_id: "utili_permille",
                    hooked: true,
                },
                PanelReuseRow {
                    sim_source: "sim_mgpu_split_ratio",
                    caliber_id: "ratio_bp",
                    hooked: true,
                },
            ],
        }
    }

    /// 复用校验：全挂接 + 口径 id 非空（0x8C02）。
    pub fn verified(&self) -> Result<(), u32> {
        for r in &self.rows {
            if !r.hooked || r.caliber_id.is_empty() || r.sim_source.is_empty() {
                return Err(XC_PANEL_REUSE_BROKEN);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、CI trace 产出：确定性可回放
// ---------------------------------------------------------------------------

/// CI trace 记录：种子+指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CiTraceRec {
    pub seed: u64,
    pub n_events: u32,
    /// 事件流指纹（FNV over tag 串）。
    pub fp: u64,
}

/// FNV-1a 64。
pub const fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf_29ce_4842_22325;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

/// 事件流指纹（tag 串）。
pub fn stream_fp(events: &[TraceEvent]) -> u64 {
    let mut tags = Vec::with_capacity(events.len());
    for e in events {
        tags.push(e.kind.tag());
    }
    fnv64(&tags)
}

/// CI 产出器：同种子两次 run 指纹必须一致（0x8C03）。
pub fn ci_replay_check(seed: u64, n_events: usize) -> Result<CiTraceRec, u32> {
    let a = sim_trace_gen(seed, n_events)?;
    let b = sim_trace_gen(seed, n_events)?;
    if a.len() != b.len() {
        return Err(XC_CI_NONDET);
    }
    let fa = stream_fp(&a);
    let fb = stream_fp(&b);
    if fa != fb {
        return Err(XC_CI_NONDET);
    }
    // 逐事件恒等（双保险——tag 同但字段不同也应拦）。
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return Err(XC_CI_NONDET);
        }
        i += 1;
    }
    Ok(CiTraceRec { seed, n_events: n_events as u32, fp: fa })
}

// ---------------------------------------------------------------------------
// 自检（三组：同格式/面板/CI）
// ---------------------------------------------------------------------------

/// 模拟调试引擎。
pub struct SimDbgEngine;

impl SimDbgEngine {
    /// 组一：同格式——声明校验 + 生成流往返恒等 + 语义反例。
    pub fn selfcheck_sameformat() -> Result<(), u32> {
        SameFormatDecl::build().verified()?;
        // 生成 16 事件走真机编码往返。
        let ev = sim_trace_gen(42, 16)?;
        same_format_check(&ev)?;
        // 语义反例：非交接事件带对端（手工构造）拒。
        let bad = [TraceEvent {
            kind: EvKind::Task,
            ts_us: 1,
            task: 1,
            engine: 1,
            peer_task: 9,
        }];
        if same_format_check(&bad).is_ok() {
            return Err(XC_SAMEFORMAT_BROKEN);
        }
        // 引擎越界反例。
        let bad2 = [TraceEvent {
            kind: EvKind::Engine,
            ts_us: 1,
            task: 1,
            engine: 9,
            peer_task: 0,
        }];
        if same_format_check(&bad2).is_ok() {
            return Err(XC_SAMEFORMAT_BROKEN);
        }
        Ok(())
    }

    /// 组二：面板复用——账平 + 断接反例。
    pub fn selfcheck_panel() -> Result<(), u32> {
        PanelReuseBook::build().verified()?;
        let mut book = PanelReuseBook::build();
        book.rows[0].hooked = false;
        if book.verified().is_ok() {
            return Err(XC_PANEL_REUSE_BROKEN);
        }
        let mut book2 = PanelReuseBook::build();
        book2.rows[1].caliber_id = "";
        if book2.verified().is_ok() {
            return Err(XC_PANEL_REUSE_BROKEN);
        }
        Ok(())
    }

    /// 组三：CI——同种子确定性 + 换种子指纹必异 + 规模闸。
    pub fn selfcheck_ci() -> Result<(), u32> {
        let rec = ci_replay_check(0xC1_2026, 64)?;
        if rec.n_events != 64 {
            return Err(XC_CI_NONDET);
        }
        // 换种子：指纹应异（碰撞概率可忽略——64 tag 长序列）。
        let rec2 = ci_replay_check(0xC1_2027, 64)?;
        if rec2.fp == rec.fp {
            return Err(XC_CI_NONDET);
        }
        // 规模闸：0 事件拒。
        if sim_trace_gen(1, 0).is_ok() {
            return Err(XC_CI_NONDET);
        }
        Ok(())
    }

    /// 全量自检入口（三组+段对账）。
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        Self::selfcheck_sameformat()?;
        Self::selfcheck_panel()?;
        Self::selfcheck_ci()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8C00..0x8D00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SAMEFORMAT_BROKEN,
            XC_PANEL_REUSE_BROKEN,
            XC_CI_NONDET,
            XC_SEG_OOR,
            XC_SEG_RESERVED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8C00 || codes[i] >= 0x8D00 {
                return Err(XC_SAMEFORMAT_BROKEN);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_SAMEFORMAT_BROKEN);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
