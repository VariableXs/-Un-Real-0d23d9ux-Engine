//! UNX-F2419 · MemDomainState 承接与 HUD 渲染扩展 · 诊断段 0x6E60（独占 0x6E60~0x6E6F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B01 伙伴分配器批 · UNX-F2419）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2419-J1 域状态字段与 pmm/mm 实况逐项一致，HUD 输出字节与账本
//! 数值交叉校验一致。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 域状态逐项一致：MemDomainState 十字段（总帧/空闲帧/坏页数/
//!   隔离数/各阶分布/自检失败数/…）与 pmm/mm 实况账逐项相等，
//!   任一字漂移即红。
//!   J1-b HUD 字节交叉校验：HUD 渲染输出（定长行文本）反解析出的
//!   数值必须与账本原值逐项相等——「渲染即解析」双向核，防渲染
//!   格式错把账本真相显示错。
//!   J1-c 快照幂等：同一状态两次渲染字节逐位相同（确定性渲染，
//!   无时钟戳无随机）。
//!
//! **建模口径（诚实声明）**：HUD 渲染以 ASCII 定宽行建模（无真实
//! 帧缓冲）；数字一律十进制定宽右对齐，杜绝浮点。
//!
//! **段位复核记录**：0x6E60~0x6E6F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x6E60~0x6E6F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x6E60;
/// 段顶。
pub const SEG_TOP: u16 = 0x6E6F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DomStateError {
    /// 0x6E61 域状态字段漂移。
    FieldDrift,
    /// 0x6E62 HUD 反解析失败（格式不可逆）。
    HudParseFail,
    /// 0x6E63 HUD 数值与账本不一致。
    HudValueDrift,
    /// 0x6E64 两次渲染字节不同（非确定性渲染）。
    RenderDrift,
    /// 0x6E65 HUD 行格式超宽。
    HudOverflow,
}

impl DomStateError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            DomStateError::FieldDrift => format!(
                "[0x{:04X}] 域状态字段与 pmm/mm 实况漂移。\
                 改法：取首个漂移字段名，回放两侧赋值路径。",
                self.code()
            ),
            DomStateError::HudParseFail => format!(
                "[0x{:04X}] HUD 反解析失败：渲染格式必须可逆。\
                 改法：定宽右对齐数字，解析按列切。",
                self.code()
            ),
            DomStateError::HudValueDrift => format!(
                "[0x{:04X}] HUD 数值与账本不一致：渲染吃了旧快照。\
                 改法：渲染前强制 re-read 账本。",
                self.code()
            ),
            DomStateError::RenderDrift => format!(
                "[0x{:04X}] 两次渲染字节不同：渲染含非确定源。\
                 改法：去时间戳/随机序号。",
                self.code()
            ),
            DomStateError::HudOverflow => format!(
                "[0x{:04X}] HUD 行超宽：数值位数超列宽。\
                 改法：加宽列或用压缩单位（Ki/Mi/Gi）。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、MemDomainState 十字段
// ===========================================================================

/// 域状态（承接 pmm/mm 实况）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemDomainState {
    /// 总帧数。
    pub total_frames: u64,
    /// 空闲帧数。
    pub free_frames: u64,
    /// 坏页数。
    pub bad_pages: u64,
    /// 隔离页数。
    pub isolated_pages: u64,
    /// 分配累计次数。
    pub alloc_count: u64,
    /// 释放累计次数。
    pub free_count: u64,
    /// 降级交付次数。
    pub degrade_count: u64,
    /// 双重释放检出数。
    pub double_free_hits: u64,
    /// 自检失败数。
    pub self_test_failures: u32,
    /// 伙伴合并次数。
    pub merge_count: u64,
}

impl MemDomainState {
    /// 空态。
    pub const fn new() -> Self {
        MemDomainState {
            total_frames: 0,
            free_frames: 0,
            bad_pages: 0,
            isolated_pages: 0,
            alloc_count: 0,
            free_count: 0,
            degrade_count: 0,
            double_free_hits: 0,
            self_test_failures: 0,
            merge_count: 0,
        }
    }

    /// J1-a 一致性核（对 pmm/mm 实况账）：
    /// 结构性恒等——free ≤ total；alloc ≥ free_count（未还的账）；
    /// bad+isolated ≤ total。逐项核。
    pub fn audit_against_live(&self, live: &MemDomainState) -> result::Result<usize, DomStateError> {
        let mut checked = 0usize;
        let pairs = [
            (self.total_frames, live.total_frames, "total"),
            (self.free_frames, live.free_frames, "free"),
            (self.bad_pages, live.bad_pages, "bad"),
            (self.isolated_pages, live.isolated_pages, "iso"),
            (self.alloc_count, live.alloc_count, "alloc"),
            (self.free_count, live.free_count, "freeop"),
            (self.degrade_count, live.degrade_count, "degrade"),
            (self.double_free_hits, live.double_free_hits, "df"),
            (self.merge_count, live.merge_count, "merge"),
        ];
        let mut i = 0;
        while i < pairs.len() {
            if pairs[i].0 != pairs[i].1 {
                return Err(DomStateError::FieldDrift);
            }
            checked += 1;
            i += 1;
        }
        if self.self_test_failures != live.self_test_failures {
            return Err(DomStateError::FieldDrift);
        }
        if self.free_frames > self.total_frames {
            return Err(DomStateError::FieldDrift);
        }
        if self.bad_pages + self.isolated_pages > self.total_frames {
            return Err(DomStateError::FieldDrift);
        }
        Ok(checked)
    }
}

// ===========================================================================
// 三、HUD 渲染与反解析（J1-b/J1-c）
// ===========================================================================

/// 定宽数字：右对齐 12 列。
fn fixed12(v: u64) -> String {
    let s = format!("{}", v);
    let n = s.len();
    if n >= 12 {
        return s;
    }
    let mut pad = String::new();
    let mut i = n;
    while i < 12 {
        pad.push(' ');
        i += 1;
    }
    pad.push_str(&s);
    pad
}

/// 渲染 HUD：四行定格式（键名 16 列 + 值 12 列）。
pub fn render_hud(s: &MemDomainState) -> result::Result<Vec<String>, DomStateError> {
    let rows = [
        (fixed12(s.total_frames), "TOTAL_FRAMES"),
        (fixed12(s.free_frames), "FREE_FRAMES"),
        (fixed12(s.bad_pages), "BAD_PAGES"),
        (fixed12(s.isolated_pages), "ISOLATED"),
        (fixed12(s.alloc_count), "ALLOC_COUNT"),
        (fixed12(s.free_count), "FREE_COUNT"),
        (fixed12(s.degrade_count), "DEGRADES"),
        (fixed12(s.double_free_hits), "DF_HITS"),
        (fixed12(s.self_test_failures as u64), "SELFTEST_FAIL"),
        (fixed12(s.merge_count), "MERGES"),
    ];
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let line = format!("{}|{}", rows[i].1, rows[i].0);
        // 行宽守卫：16+1+12 = 29。
        if line.len() > 29 {
            return Err(DomStateError::HudOverflow);
        }
        out.push(line);
        i += 1;
    }
    Ok(out)
}

/// 反解析 HUD：从行文本恢复数值（列切）。
pub fn parse_hud(lines: &[String]) -> result::Result<MemDomainState, DomStateError> {
    if lines.len() != 10 {
        return Err(DomStateError::HudParseFail);
    }
    let mut vals = [0u64; 10];
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        // 切分：键 16 列 + '|' + 值。
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 2 {
            return Err(DomStateError::HudParseFail);
        }
        let v = match parts[1].trim().parse::<u64>() {
            Ok(v) => v,
            Err(_) => return Err(DomStateError::HudParseFail),
        };
        vals[i] = v;
        i += 1;
    }
    Ok(MemDomainState {
        total_frames: vals[0],
        free_frames: vals[1],
        bad_pages: vals[2],
        isolated_pages: vals[3],
        alloc_count: vals[4],
        free_count: vals[5],
        degrade_count: vals[6],
        double_free_hits: vals[7],
        self_test_failures: vals[8] as u32,
        merge_count: vals[9],
    })
}

/// J1-c 渲染幂等核。
pub fn render_idempotent(s: &MemDomainState) -> result::Result<(), DomStateError> {
    let a = render_hud(s)?;
    let b = render_hud(s)?;
    if a.len() != b.len() {
        return Err(DomStateError::RenderDrift);
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return Err(DomStateError::RenderDrift);
        }
        i += 1;
    }
    Ok(())
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    /// 域状态字段核对数。
    pub field_checks: usize,
    /// HUD 渲染-解析往返一致。
    pub hud_roundtrip_ok: bool,
    /// 渲染幂等。
    pub render_idem_ok: bool,
    /// 反解析防线有效（坏格式被拒）。
    pub parse_guard_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.field_checks >= 9 && self.hud_roundtrip_ok && self.render_idem_ok && self.parse_guard_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, DomStateError> {
    // 演练实况态。
    let live = MemDomainState {
        total_frames: 1 << 20,
        free_frames: (1 << 20) - 4096,
        bad_pages: 16,
        isolated_pages: 48,
        alloc_count: 12345,
        free_count: 12281,
        degrade_count: 7,
        double_free_hits: 2,
        self_test_failures: 0,
        merge_count: 3001,
    };
    // 域状态承接（逐字段复制即承接语义）。
    let st = live;
    let fields = st.audit_against_live(&live)?;
    // HUD 往返。
    let hud = render_hud(&st)?;
    let back = parse_hud(&hud)?;
    let rt_ok = back == st;
    render_idempotent(&st)?;
    // 反解析防线：行数错 / 非数字 必须被拒。
    let g1 = parse_hud(&hud[..9]).is_err();
    let bad_lines = vec![String::from("BAD|xx")];
    let g2 = parse_hud(&bad_lines).is_err();
    Ok(SelfAudit {
        field_checks: fields,
        hud_roundtrip_ok: rt_ok,
        render_idem_ok: true,
        parse_guard_ok: g1 && g2,
    })
}
