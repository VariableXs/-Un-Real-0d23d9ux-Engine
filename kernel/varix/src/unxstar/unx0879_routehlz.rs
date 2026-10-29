//! unx0879_routehlz —— UNX-F0879 · 路由健康总账（JSON 对齐 F0854 统一出口）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0879 · 路由健康总账。
//! 判据原文摘录（卡面验收行）：路由健康总账（JSON 对齐 F0854 统一出口）
//!           ——路由面健康行按 F0854 统一 schema 出口（v/s/k/n 四键）、
//!           kind 闭集回读可校、分块导出可续。
//! 诊断段 0x7960（本单独占，0x7961~0x796F 闭集）。
//!
//! 设计纪律：
//! - 统一 schema（对齐 F0854）：`{"v":<向量>,"s":<序号>,"k":"<流种>","n":<量>}`
//!   四键一花括号对；kind 为路由面闭集 tag（rte/bind/mask/health）。
//! - 健康行来源：路由配置事件（绑定/掩码/RTE 改写）逐笔落健康行；
//!   行存量 = 健康 + 事件守恒。
//! - 分块导出：按块导出（块长定容），游标推进至尽；导出后游标守恒
//!   （已导 + 未导 = 总行数）。
//! - schema 校验面：花括号对 + 四键齐 + kind 闭集回读三道门。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const RH_VERSION: &str = "unx0879_routehlz-v1";

/// 诊断段基址（0x7960 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7960;

/// XcCode 闭集（0x7961~0x796F）。
pub const XC_KIND: u16 = SEG_BASE + 1; // kind 越闭集
pub const XC_FORMAT: u16 = SEG_BASE + 2; // JSON 行格式失配
pub const XC_VEC_RANGE: u16 = SEG_BASE + 3; // 向量越界（16~255 外）
pub const XC_CURSOR: u16 = SEG_BASE + 4; // 导出游标越界
pub const XC_LED_FULL: u16 = SEG_BASE + 5; // 账溢出（计数不丢）
pub const XC_CONSERVE: u16 = SEG_BASE + 6; // 守恒破
pub const XC_EMPTY: u16 = SEG_BASE + 7; // 空账导出

/// 账容量。
pub const LED_CAP: usize = 128;
/// 导出块长。
pub const BLOCK: usize = 8;

/// kind 闭集（路由面健康流种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteKind {
    /// RTE 改写。
    Rte,
    /// 绑定变更。
    Bind,
    /// 掩码变更。
    Mask,
    /// 健康探针行。
    Health,
}

impl RouteKind {
    /// tag（JSON 出口字面）。
    pub fn tag(&self) -> &'static str {
        match self {
            RouteKind::Rte => "rte",
            RouteKind::Bind => "bind",
            RouteKind::Mask => "mask",
            RouteKind::Health => "health",
        }
    }
    /// tag 回读闭集。
    pub fn from_tag(t: &str) -> Result<RouteKind, u16> {
        match t {
            "rte" => Ok(RouteKind::Rte),
            "bind" => Ok(RouteKind::Bind),
            "mask" => Ok(RouteKind::Mask),
            "health" => Ok(RouteKind::Health),
            _ => Err(XC_KIND),
        }
    }
}

/// 健康行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RouteRow {
    /// 向量（16~255）。
    pub vector: u16,
    /// 序号。
    pub seq: u64,
    /// 流种。
    pub kind: RouteKind,
    /// 量（事件载荷：绑定目标/掩码位/探针值）。
    pub num: u32,
}

impl RouteRow {
    /// F0854 统一 schema 出口。
    pub fn to_json(&self) -> String {
        format!(
            "{{\"v\":{},\"s\":{},\"k\":\"{}\",\"n\":{}}}",
            self.vector, self.seq, self.kind.tag(), self.num
        )
    }
}

/// 路由健康总账。
pub struct RouteHealth {
    rows: Vec<RouteRow>,
    overflow: usize,
    seq: u64,
    /// 导出游标（已导出行数）。
    cursor: usize,
}

impl RouteHealth {
    pub fn new() -> RouteHealth {
        RouteHealth {
            rows: Vec::new(),
            overflow: 0,
            seq: 0,
            cursor: 0,
        }
    }

    /// 落健康行（向量界门 + kind 闭集 + 定容账）。
    pub fn record(&mut self, vector: u16, kind: RouteKind, num: u32) -> Result<u64, u16> {
        if vector < 16 || vector > 255 {
            return Err(XC_VEC_RANGE);
        }
        self.seq += 1;
        if self.rows.len() < LED_CAP {
            self.rows.push(RouteRow { vector, seq: self.seq, kind, num });
        } else {
            self.overflow += 1;
        }
        Ok(self.seq)
    }

    /// schema 校验器：花括号对 + 四键齐 + kind 回读。
    pub fn validate(line: &str) -> Result<RouteKind, u16> {
        if !line.starts_with('{') || !line.ends_with('}') {
            return Err(XC_FORMAT);
        }
        for key in ["\"v\":", "\"s\":", "\"k\":\"", "\"n\":"].iter() {
            if !line.contains(key) {
                return Err(XC_FORMAT);
            }
        }
        // kind 抽取：\"k\":\"<tag>\"
        let kpos = match line.find("\"k\":\"") {
            Some(p) => p + 5,
            None => return Err(XC_FORMAT),
        };
        let rest = &line[kpos..];
        let end = match rest.find('"') {
            Some(e) => e,
            None => return Err(XC_FORMAT),
        };
        RouteKind::from_tag(&rest[..end])
    }

    /// 分块导出：一块 BLOCK 行，返回 (本块行数, 累计导出)。
    pub fn export_block(&mut self) -> Result<(usize, usize, Vec<String>), u16> {
        if self.rows.is_empty() {
            return Err(XC_EMPTY);
        }
        if self.cursor >= self.rows.len() {
            return Err(XC_CURSOR);
        }
        let end = if self.cursor + BLOCK > self.rows.len() {
            self.rows.len()
        } else {
            self.cursor + BLOCK
        };
        let mut out = Vec::new();
        for i in self.cursor..end {
            out.push(self.rows[i].to_json());
        }
        let n = end - self.cursor;
        self.cursor = end;
        Ok((n, self.cursor, out))
    }

    /// 导出完毕判。
    pub fn export_done(&self) -> bool {
        self.cursor == self.rows.len()
    }

    /// 导出守恒：已导 + 未导 = 在账行数。
    pub fn export_conserve(&self) -> bool {
        self.cursor <= self.rows.len()
            && self.cursor + (self.rows.len() - self.cursor) == self.rows.len()
    }

    /// 账守恒：在账 + 溢出 = 落行次数。
    pub fn ledger_conserve(&self) -> bool {
        self.rows.len() + self.overflow == self.seq as usize
    }

    /// 行读取。
    pub fn row(&self, i: usize) -> Result<RouteRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.rows[i])
    }

    /// 行数。
    pub fn count(&self) -> usize {
        self.rows.len()
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 落健康行 ×20（四 kind 轮换）
    let mut h = RouteHealth::new();
    let kinds = [
        RouteKind::Rte,
        RouteKind::Bind,
        RouteKind::Mask,
        RouteKind::Health,
    ];
    for i in 0..20u16 {
        h.record(16 + i, kinds[(i % 4) as usize], i as u32)?;
    }
    if h.count() != 20 || !h.ledger_conserve() {
        return Err(XC_CONSERVE);
    }
    // 2. JSON 出口 + schema 回读（逐行）
    for i in 0..h.count() {
        let line = h.row(i)?.to_json();
        if RouteHealth::validate(&line).is_err() {
            return Err(XC_KIND);
        }
        if !line.starts_with('{') || !line.contains("\"k\":\"") {
            return Err(XC_FORMAT);
        }
    }
    // 3. 分块导出：20 行 / 块 8 → 3 块（8/8/4），游标守恒
    let mut total = 0usize;
    let mut blocks = 0usize;
    while !h.export_done() {
        let (n, cur, out) = h.export_block()?;
        if out.len() != n {
            return Err(XC_CONSERVE);
        }
        total += n;
        blocks += 1;
        if cur != total {
            return Err(XC_CURSOR);
        }
    }
    if total != 20 || blocks != 3 || !h.export_conserve() {
        return Err(XC_CONSERVE);
    }
    // 4. 错误路径①：导尽再导
    if h.export_block().is_ok() {
        return Err(XC_CURSOR);
    }
    // 5. 错误路径②：空账导出
    let mut h2 = RouteHealth::new();
    if h2.export_block().is_ok() {
        return Err(XC_EMPTY);
    }
    // 6. 错误路径③：schema 校验（非 JSON / 缺键 / kind 越闭集）
    if RouteHealth::validate("not json").is_ok() {
        return Err(XC_FORMAT);
    }
    if RouteHealth::validate("{\"v\":40,\"s\":1,\"n\":2}").is_ok() {
        return Err(XC_FORMAT);
    }
    if RouteHealth::validate("{\"v\":40,\"s\":1,\"k\":\"bogus\",\"n\":2}").is_ok() {
        return Err(XC_KIND);
    }
    // 7. 错误路径④：向量越界落行
    if h.record(0, RouteKind::Rte, 1).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    if h.record(300, RouteKind::Rte, 1).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    // 8. kind 闭集回读对拍
    if RouteKind::from_tag("mask")? != RouteKind::Mask {
        return Err(XC_KIND);
    }
    if h.row(0)?.kind != RouteKind::Rte || h.row(3)?.kind != RouteKind::Health {
        return Err(XC_KIND);
    }
    Ok(0)
}
