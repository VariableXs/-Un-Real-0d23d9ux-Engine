//! CGPU-F1702 · 异构 trace 与开放格式
//!
//! 判据锚点（施工书原文摘录）：
//! trace：trace 格式（异构 trace 开放格式（任务/引擎/交接事件——格式 schema 版本
//! 化——开放承诺复用；导出导入（trace 导出/导入（互操作（Perfetto 兼容预留——
//! 互操作预留；隐私（trace 内容审查（无用户数据——红线复用；测试（schema/导出/
//! 兼容/隐私四组）。判据：开放格式、导出、互操作、隐私、四组、判据。
//!
//! 诊断码段 0x8901~0x8905（独占）：
//!   0x8901 schema 失格（魔数错/版本不支持/字段数与 schema 不符）
//!   0x8902 导出导入失恒（回读事件与源事件指纹失配/事件数不等）
//!   0x8903 互操作预留失信（承诺窗关闭/兼容层版本越界）
//!   0x8904 隐私红线破（trace 事件携带用户数据字段）
//!   0x8905 段越界（判码落 0x8900..0x8A00 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

use alloc::vec::Vec;

/// 模块版本。
pub const TRACE_FMT_VERSION: &str = "K21-tracefmt-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8901~0x8905）
// ---------------------------------------------------------------------------

pub const XC_SCHEMA_MALFORMED: u32 = 0x8901;
pub const XC_ROUNDTRIP_BROKEN: u32 = 0x8902;
pub const XC_COMPAT_PROMISE_BROKEN: u32 = 0x8903;
pub const XC_PRIVACY_VIOLATION: u32 = 0x8904;
pub const XC_SEG_OOR: u32 = 0x8905;

// ---------------------------------------------------------------------------
// 一、trace 事件闭集与 schema 版本化
// ---------------------------------------------------------------------------

/// 开放格式魔数（"VXTR"——Varix Trace）。
pub const TRACE_MAGIC: [u8; 4] = [b'V', b'X', b'T', b'R'];

/// schema 版本（v1 开放承诺：读侧永远向后兼容 v1）。
pub const SCHEMA_VER_V1: u16 = 1;
/// 开放承诺窗：承诺兼容的最低 schema 版本（承诺区间 [PROMISE_MIN, SCHEMA_VER]）。
pub const SCHEMA_PROMISE_MIN: u16 = 1;

/// 事件类三闭集（判据"任务/引擎/交接事件"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    /// 任务事件（提交/完成）。
    Task,
    /// 引擎事件（占用/空闲）。
    Engine,
    /// 交接事件（跨引擎 handoff）。
    Handoff,
}

impl EvKind {
    pub const ALL: [EvKind; 3] = [EvKind::Task, EvKind::Engine, EvKind::Handoff];
    pub const fn tag(self) -> u8 {
        match self {
            EvKind::Task => 1,
            EvKind::Engine => 2,
            EvKind::Handoff => 3,
        }
    }
    pub const fn from_tag(t: u8) -> Option<EvKind> {
        match t {
            1 => Some(EvKind::Task),
            2 => Some(EvKind::Engine),
            3 => Some(EvKind::Handoff),
            _ => None,
        }
    }
}

/// 单条 trace 事件（开放格式 v1 字段闭集——隐私审查按此闭集执行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TraceEvent {
    pub kind: EvKind,
    /// 时间戳 µs（F1442 单调钟）。
    pub ts_us: u64,
    /// 任务 id。
    pub task: u32,
    /// 引擎序（0..=3）。
    pub engine: u8,
    /// 交接对端任务（仅 Handoff 事件非零）。
    pub peer_task: u32,
}

/// trace 头（schema 版本化锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchemaHeader {
    pub magic: [u8; 4],
    pub schema_ver: u16,
    /// 事件记录数。
    pub count: u32,
}

impl SchemaHeader {
    /// 编码：4 魔数 + 2 版本 + 4 计数 = 10 字节定长头。
    pub fn encode(&self) -> [u8; 10] {
        let mut b = [0u8; 10];
        b[0..4].copy_from_slice(&self.magic);
        b[4..6].copy_from_slice(&self.schema_ver.to_le_bytes());
        b[6..10].copy_from_slice(&self.count.to_le_bytes());
        b
    }

    /// 解码 + 校验：魔数错/版本不支持/计数与实际不符即 0x8901。
    pub fn decode(bytes: &[u8], actual_count: u32) -> Result<SchemaHeader, u32> {
        if bytes.len() < 10 {
            return Err(XC_SCHEMA_MALFORMED);
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&bytes[0..4]);
        if magic != TRACE_MAGIC {
            return Err(XC_SCHEMA_MALFORMED);
        }
        let mut vb = [0u8; 2];
        vb.copy_from_slice(&bytes[4..6]);
        let ver = u16::from_le_bytes(vb);
        if ver < SCHEMA_PROMISE_MIN || ver > SCHEMA_VER_V1 {
            return Err(XC_SCHEMA_MALFORMED);
        }
        let mut cb = [0u8; 4];
        cb.copy_from_slice(&bytes[6..10]);
        let count = u32::from_le_bytes(cb);
        if count != actual_count {
            return Err(XC_SCHEMA_MALFORMED);
        }
        Ok(SchemaHeader { magic, schema_ver: ver, count })
    }
}

/// 单事件编码：tag(1)+ts(8)+task(4)+engine(1)+peer(4) = 18 字节定长。
pub const EV_REC_LEN: usize = 18;

pub fn encode_event(e: &TraceEvent, out: &mut Vec<u8>) {
    out.push(e.kind.tag());
    let mut buf8 = [0u8; 8];
    buf8.copy_from_slice(&e.ts_us.to_le_bytes());
    out.extend_from_slice(&buf8);
    out.extend_from_slice(&e.task.to_le_bytes());
    out.push(e.engine);
    out.extend_from_slice(&e.peer_task.to_le_bytes());
}

pub fn decode_event(bytes: &[u8], off: usize) -> Result<TraceEvent, u32> {
    if off + EV_REC_LEN > bytes.len() {
        return Err(XC_SCHEMA_MALFORMED);
    }
    let kind = EvKind::from_tag(bytes[off]).ok_or(XC_SCHEMA_MALFORMED)?;
    let mut b8 = [0u8; 8];
    b8.copy_from_slice(&bytes[off + 1..off + 9]);
    let ts_us = u64::from_le_bytes(b8);
    let mut b4 = [0u8; 4];
    b4.copy_from_slice(&bytes[off + 9..off + 13]);
    let task = u32::from_le_bytes(b4);
    let engine = bytes[off + 13];
    if engine > 3 {
        return Err(XC_SCHEMA_MALFORMED);
    }
    b4.copy_from_slice(&bytes[off + 14..off + 18]);
    let peer = u32::from_le_bytes(b4);
    // 语义校验：非交接事件对端必须为 0。
    if kind != EvKind::Handoff && peer != 0 {
        return Err(XC_SCHEMA_MALFORMED);
    }
    Ok(TraceEvent { kind, ts_us, task, engine, peer_task: peer })
}

// ---------------------------------------------------------------------------
// 二、导出/导入（互操作预留 Perfetto）
// ---------------------------------------------------------------------------

/// 导出器：头+事件流。
pub fn export(events: &[TraceEvent]) -> Result<Vec<u8>, u32> {
    let mut out = Vec::with_capacity(10 + events.len() * EV_REC_LEN);
    let header = SchemaHeader {
        magic: TRACE_MAGIC,
        schema_ver: SCHEMA_VER_V1,
        count: events.len() as u32,
    };
    let hb = header.encode();
    out.extend_from_slice(&hb);
    for e in events {
        encode_event(e, &mut out);
    }
    Ok(out)
}

/// 导入器：回读全部事件（失恒=0x8902）。
pub fn import(bytes: &[u8]) -> Result<Vec<TraceEvent>, u32> {
    let header = SchemaHeader::decode(bytes, {
        // 实际事件数由定长记录数推得（(len-10)/18）。
        if bytes.len() < 10 || (bytes.len() - 10) % EV_REC_LEN != 0 {
            return Err(XC_SCHEMA_MALFORMED);
        }
        ((bytes.len() - 10) / EV_REC_LEN) as u32
    })?;
    let mut out = Vec::with_capacity(header.count as usize);
    let mut off = 10usize;
    let mut i = 0;
    while i < header.count {
        out.push(decode_event(bytes, off)?);
        off += EV_REC_LEN;
        i += 1;
    }
    Ok(out)
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

/// 回读对拍：导入事件逐字段与源一致 + 序列指纹一致。
pub fn roundtrip_check(src: &[TraceEvent], back: &[TraceEvent]) -> Result<(), u32> {
    if src.len() != back.len() {
        return Err(XC_ROUNDTRIP_BROKEN);
    }
    let mut i = 0;
    while i < src.len() {
        if src[i] != back[i] {
            return Err(XC_ROUNDTRIP_BROKEN);
        }
        i += 1;
    }
    // 序列级指纹：源序列事件 tag 串与回读 tag 串一致。
    let mut st = Vec::new();
    let mut bt = Vec::new();
    for e in src {
        st.push(e.kind.tag());
    }
    for e in back {
        bt.push(e.kind.tag());
    }
    if fnv64(&st) != fnv64(&bt) {
        return Err(XC_ROUNDTRIP_BROKEN);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、互操作预留（Perfetto 兼容承诺窗）
// ---------------------------------------------------------------------------

/// 互操作承诺：版本窗内承诺提供 Perfetto 兼容导出（预留位——不承诺完成时点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompatPromise {
    /// 兼容目标版本（Perfetto proto 版本号锚——预留登记）。
    pub target: &'static str,
    /// 承诺生效（窗开着）。
    pub promised: bool,
}

impl CompatPromise {
    pub fn build() -> Self {
        CompatPromise { target: "perfetto-proto-reserved", promised: true }
    }

    /// 预留校验：承诺在窗 + 目标锚非空（关闭=0x8903）。
    pub fn verified(&self) -> Result<(), u32> {
        if !self.promised || self.target.is_empty() {
            return Err(XC_COMPAT_PROMISE_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、隐私红线：trace 内容审查
// ---------------------------------------------------------------------------

/// 隐私审查：事件字段闭集审计——开放格式 v1 字段（kind/ts/task/engine/peer）
/// 全部为运行时标识与时间量，无用户数据字段（路径/窗口名/键击/网络地址）。
/// 审计=闭集声明+字段名单复核（白名单外字段出现即 0x8904）。
pub const ALLOWED_FIELDS: [&str; 5] = ["kind", "ts_us", "task", "engine", "peer_task"];

pub fn privacy_audit(event_fields: &[&str]) -> Result<(), u32> {
    for f in event_fields {
        let mut ok = false;
        let mut i = 0;
        while i < ALLOWED_FIELDS.len() {
            if ALLOWED_FIELDS[i] == *f {
                ok = true;
            }
            i += 1;
        }
        if !ok {
            return Err(XC_PRIVACY_VIOLATION);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检（四组：schema/导出导入/互操作/隐私）
// ---------------------------------------------------------------------------

/// trace 引擎。
pub struct TraceEngine {
    pub events: Vec<TraceEvent>,
}

impl TraceEngine {
    pub fn new() -> Self {
        TraceEngine { events: Vec::new() }
    }

    /// 样本：三类事件各一（含跨引擎交接）。
    pub fn seed(&mut self) -> Result<(), u32> {
        self.events.push(TraceEvent {
            kind: EvKind::Task,
            ts_us: 100,
            task: 1,
            engine: 0,
            peer_task: 0,
        });
        self.events.push(TraceEvent {
            kind: EvKind::Engine,
            ts_us: 200,
            task: 1,
            engine: 1,
            peer_task: 0,
        });
        self.events.push(TraceEvent {
            kind: EvKind::Handoff,
            ts_us: 300,
            task: 1,
            engine: 1,
            peer_task: 7,
        });
        Ok(())
    }

    /// 组一：schema——头编解码 + 魔数错拒 + 版本不支持拒 + 计数失配拒。
    pub fn selfcheck_schema(&mut self) -> Result<(), u32> {
        self.seed()?;
        let h = SchemaHeader {
            magic: TRACE_MAGIC,
            schema_ver: SCHEMA_VER_V1,
            count: 3,
        };
        let enc = h.encode();
        let back = SchemaHeader::decode(&enc, 3)?;
        if back.schema_ver != SCHEMA_VER_V1 || back.count != 3 {
            return Err(XC_SCHEMA_MALFORMED);
        }
        // 魔数错。
        let mut bad = enc;
        bad[0] = b'X';
        if SchemaHeader::decode(&bad, 3).is_ok() {
            return Err(XC_SCHEMA_MALFORMED);
        }
        // 版本不支持（0 < PROMISE_MIN）。
        let mut badv = enc;
        badv[4] = 0;
        badv[5] = 0;
        if SchemaHeader::decode(&badv, 3).is_ok() {
            return Err(XC_SCHEMA_MALFORMED);
        }
        // 计数失配。
        if SchemaHeader::decode(&enc, 4).is_ok() {
            return Err(XC_SCHEMA_MALFORMED);
        }
        // 事件语义：非交接事件带对端拒。
        let mut rec = [0u8; EV_REC_LEN];
        let mut tmp = Vec::new();
        encode_event(
            &TraceEvent { kind: EvKind::Task, ts_us: 1, task: 1, engine: 1, peer_task: 5 },
            &mut tmp,
        );
        rec.copy_from_slice(&tmp);
        if decode_event(&rec, 0).is_ok() {
            return Err(XC_SCHEMA_MALFORMED);
        }
        Ok(())
    }

    /// 组二：导出导入——回读恒等 + 指纹对拍 + 破损拒。
    pub fn selfcheck_roundtrip(&mut self) -> Result<(), u32> {
        self.seed()?;
        let bytes = export(&self.events)?;
        let back = import(&bytes)?;
        roundtrip_check(&self.events, &back)?;
        // 长度恒等：10+3*18。
        if bytes.len() != 10 + 3 * EV_REC_LEN {
            return Err(XC_ROUNDTRIP_BROKEN);
        }
        // 破损反例：截断流拒。
        if import(&bytes[..bytes.len() - 1]).is_ok() {
            return Err(XC_ROUNDTRIP_BROKEN);
        }
        Ok(())
    }

    /// 组三：互操作预留——承诺窗 + 关闭反例。
    pub fn selfcheck_compat(&mut self) -> Result<(), u32> {
        let p = CompatPromise::build();
        p.verified()?;
        let mut bad = CompatPromise::build();
        bad.promised = false;
        if bad.verified().is_ok() {
            return Err(XC_COMPAT_PROMISE_BROKEN);
        }
        let mut bad2 = CompatPromise::build();
        bad2.target = "";
        if bad2.verified().is_ok() {
            return Err(XC_COMPAT_PROMISE_BROKEN);
        }
        Ok(())
    }

    /// 组四：隐私——白名单全过 + 域外字段拒。
    pub fn selfcheck_privacy(&mut self) -> Result<(), u32> {
        privacy_audit(&ALLOWED_FIELDS)?;
        // 域外字段反例：用户路径字段混入即拒。
        let fields = ["kind", "ts_us", "window_title"];
        if privacy_audit(&fields).is_ok() {
            return Err(XC_PRIVACY_VIOLATION);
        }
        // 网络地址字段反例。
        let fields2 = ["kind", "net_addr"];
        if privacy_audit(&fields2).is_ok() {
            return Err(XC_PRIVACY_VIOLATION);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        Self::segment_guard()?;
        self.selfcheck_schema()?;
        self.selfcheck_roundtrip()?;
        self.selfcheck_compat()?;
        self.selfcheck_privacy()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8900..0x8A00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SCHEMA_MALFORMED,
            XC_ROUNDTRIP_BROKEN,
            XC_COMPAT_PROMISE_BROKEN,
            XC_PRIVACY_VIOLATION,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8900 || codes[i] >= 0x8A00 {
                return Err(XC_SCHEMA_MALFORMED);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_SCHEMA_MALFORMED);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
