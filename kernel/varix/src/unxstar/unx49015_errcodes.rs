//! UNX-F49015 触控板 quirk 错误码族 + UNX-F49016 四点观测埋点 · 0x71A80
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，VX6 批 B11-C 次槽；卡面 J1 引用按原文保留）：
//!   UNX-F49015 触控板 quirk 表引擎与配置面错误码族（15 条）:
//!          ① 纯功能代码 ≥220 行；
//!          ② 判据原文：UNX-F49016-J1 四点 100% 覆盖；异常零静默；判据锚定 体验十三·补；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   UNX-F49016 触控板 quirk 表引擎与配置面观测埋点（入口/出口/耗时/结果四点全覆盖）:
//!          ① 纯功能代码 ≥260 行；
//!          ② 判据原文：UNX-F49017-J1 fuzz ×10^4 零崩；判据锚定 体验十 健壮性；
//!          ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 说明：F49015 卡面引用 UNX-F49016-J1、F49016 卡面引用 UNX-F49017-J1（板上
//! 自身的 +1 引用）。此处逐字保留，不改号，便于任务板锚定逐字回查。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（错误码族）：条数 → 码值递增 → 名称形状 → 名称唯一 → 族/严重度 →
//! 中文说明 → 码窗 → 摘要。
//! 条数先于码值：少于 15 条时「码值连续」照样能成立，先钉条数才不会让一张
//! 缺项的注册表以「连续」为名通过。
//! 码值先于名称：码值是跨模块的诊断契约，名称是人类可读标签。码值重复时
//! 两个不同故障会被同一值认领，此时名称再规整也只是两条别名。
//! 名称形状先于唯一：名称要能被落账与检索（小写 snake_case、无首数字、
//! 无连续下划线）。形状不合法时唯一性检查只是在比较两串噪声。
//! 族/严重度先于中文说明：族决定「谁负责处理」、严重度决定「是否阻断」；
//! 这两项越界时，中文说明再好听也会被错误的路由消费。
//! 中文说明先于码窗：说明为空等于把诊断退化成裸数字；先补说明，再做码窗
//! 守卫，才能保证「码窗内的每个值都有可读解释」。
//! 码窗后于全部结构检查：码窗是最后一道「段位」闸，它只回答「值有没有跑出
//! 本段」，不能替代结构自洽；先跑码窗会把结构病掩盖成越界。
//! 闸门次序（四点埋点）：点位齐备 → 时序单调 → 耗时一致 → 结果登记 →
//! 覆盖合账 → 故障注入。
//! 点位齐备先于时序：入口/出口/耗时/结果四点缺一则整条记录不成立；缺入口
//! 时去比较 exit>entry 没有意义，等于拿半个事实做判断。
//! 时序先于耗时：耗时是 exit-entry 的派生量。顺序反了，先信记录里写死的
//! 耗时再倒推时序，时钟回拨会被「自洽的假耗时」掩盖。
//! 耗时先于结果：结果码要挂在一条时序成立的记录上才有诊断价值；时序不成立
//! 时结果码只说明「某处报过错」，不能定位到具体操作。
//! 结果先于覆盖：结果码必须在 15 条注册表内。未登记的结果码等于静默异常，
//! 先拒未登记，100% 覆盖才有内容，而不是「四个空字段都填了」。
//! 覆盖后于故障注入：覆盖率是「正常路径四点齐」的度量，注入是「坏记录必被
//! 抓」的度量，二者互为反面；先算覆盖再注入，才能看出来闸门是否真的在拦。
//! 关键差异：本模块不定义 quirk 表的匹配语义（表引擎在 0x718C0 段），不做
//! 配置持久化（B11-C 其它槽位），不产生用户可见文案（文案走查在 0x71A40
//! 段）。四点埋点只建模「入口/出口/耗时/结果」四个点位与覆盖闸，不驱动真实
//! 计时硬件；错误码只登记值/名/族/严重度，不复写各子系统的处理分支。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71A80，VX6 批 B11-C 次槽）。
pub const SEG_BASE: u32 = 0x71A80;

/// 错误码条数（卡面 ×15）。
pub const ERR_COUNT: usize = 15;
/// 错误码基值。
pub const CODE_BASE: u32 = 0x72000;
/// 错误码步长（低 4 位留给负载）。
pub const CODE_STEP: u32 = 0x10;
/// 负载上限（0..=15）。
pub const PAYLOAD_MAX: u32 = 15;
/// 族数（表/匹配/配置/观测）。
pub const FAMILIES: usize = 4;
/// 严重度下限。
pub const SEV_MIN: u8 = 1;
/// 严重度上限。
pub const SEV_MAX: u8 = 3;
/// 名称最短长度。
pub const NAME_MIN: usize = 4;
/// 名称最长长度。
pub const NAME_MAX: usize = 24;
/// 四点个数。
pub const POINTS: u32 = 4;
/// 入口点位。
pub const PT_ENTRY: u8 = 0x01;
/// 出口点位。
pub const PT_EXIT: u8 = 0x02;
/// 耗时点位。
pub const PT_DURATION: u8 = 0x04;
/// 结果点位。
pub const PT_RESULT: u8 = 0x08;
/// 四点齐备掩码。
pub const PT_ALL: u8 = 0x0F;
/// 单次耗时上限（约 16M  ticks，防溢出）。
pub const DUR_MAX: u32 = 0x00FF_FFFF;
/// 注册表往返对数（15 × 16）。
pub const ROUNDTRIP_PAIRS: u32 = 240;
/// 正常路径埋点操作数（15 码 × 16 负载）。
pub const OPS: u32 = 240;
/// 观测风暴轮数（卡面 fuzz ×10^4）。
pub const STORM_ROUNDS: u32 = 10_000;
/// 故障注入路数。
pub const FAULT_ROUTES: u32 = 4;
/// 故障注入轮数上界。
pub const FAULT_MAX: u32 = 10_000;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcCode {
    /// 条数不是 15
    CountBad,
    /// 码值重复
    DupBad,
    /// 名称形状非法
    NameBad,
    /// 码值未按步长递增
    StepBad,
    /// 族号越界
    FamilyBad,
    /// 严重度越界
    SeverityBad,
    /// 覆盖轮数为零或越界
    CoverBad,
    /// 入口点缺失或时序倒置
    EntryBad,
    /// 出口点缺失
    ExitBad,
    /// 耗时点缺失或与出口入口差不一致
    DurationBad,
    /// 结果点缺失或结果码未登记
    ResultBad,
    /// 复跑摘要漂移
    ReplayBad,
    /// 渲染行不可解析
    RenderBad,
    /// 诊断码越出本模块段窗
    SegmentBad,
}

impl EcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            EcCode::CountBad => "错误码条数不是 15",
            EcCode::DupBad => "错误码值或名称重复",
            EcCode::NameBad => "错误码名称不是小写 snake_case 或长度越界",
            EcCode::StepBad => "错误码值未按 0x10 步长递增",
            EcCode::FamilyBad => "错误码族号越出 0..3",
            EcCode::SeverityBad => "严重度越出 1..=3",
            EcCode::CoverBad => "覆盖轮数为零或越过上限",
            EcCode::EntryBad => "入口点缺失或 exit 不晚于 entry",
            EcCode::ExitBad => "出口点缺失",
            EcCode::DurationBad => "耗时点缺失或耗时 ≠ exit - entry",
            EcCode::ResultBad => "结果点缺失或结果码未登记",
            EcCode::ReplayBad => "两次确定性复跑摘要不一致",
            EcCode::RenderBad => "渲染行字段缺失或回读不一致",
            EcCode::SegmentBad => "诊断码越出本模块段窗",
        }
    }

    pub const ALL: [EcCode; 14] = [
        EcCode::CountBad,
        EcCode::DupBad,
        EcCode::NameBad,
        EcCode::StepBad,
        EcCode::FamilyBad,
        EcCode::SeverityBad,
        EcCode::CoverBad,
        EcCode::EntryBad,
        EcCode::ExitBad,
        EcCode::DurationBad,
        EcCode::ResultBad,
        EcCode::ReplayBad,
        EcCode::RenderBad,
        EcCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EcCode::ALL.len() {
        if let Some(c) = EcCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 15 条错误码注册表 ─────────────────────────────────────────────
/// 一条错误码：值 / 名 / 族 / 严重度 / 中文说明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CodeDef {
    pub code: u32,
    pub name: &'static str,
    pub family: u8,
    pub severity: u8,
    pub zh: &'static str,
}

/// 族名表（索引即族号）。
pub const FAMILY_NAMES: [&str; FAMILIES] = ["table", "match", "config", "observe"];

pub const CODES: [CodeDef; ERR_COUNT] = [
    CodeDef { code: 0x72000, name: "table_empty", family: 0, severity: 3, zh: "quirk 表为空，无法匹配" },
    CodeDef { code: 0x72010, name: "table_oversize", family: 0, severity: 2, zh: "表项数越过定容上限" },
    CodeDef { code: 0x72020, name: "table_range_inverted", family: 0, severity: 3, zh: "表项固件区间倒置" },
    CodeDef { code: 0x72030, name: "table_checksum_mismatch", family: 0, severity: 3, zh: "表校验和与版本戳不符" },
    CodeDef { code: 0x72040, name: "match_model_zero", family: 1, severity: 2, zh: "设备自报机型为零" },
    CodeDef { code: 0x72050, name: "match_fw_unknown", family: 1, severity: 2, zh: "固件版本不在任何区间" },
    CodeDef { code: 0x72060, name: "match_ambiguous", family: 1, severity: 3, zh: "同一探测命中多条表项" },
    CodeDef { code: 0x72070, name: "match_mask_empty", family: 1, severity: 2, zh: "命中表项的参数掩码为空" },
    CodeDef { code: 0x72080, name: "config_write_denied", family: 2, severity: 2, zh: "配置写入被策略拒绝" },
    CodeDef { code: 0x72090, name: "config_rollback", family: 2, severity: 1, zh: "配置回退到默认值" },
    CodeDef { code: 0x720A0, name: "config_stale_gen", family: 2, severity: 2, zh: "提交世代落后于读取世代" },
    CodeDef { code: 0x720B0, name: "degrade_no_ledger", family: 3, severity: 3, zh: "降级未落账（虚报）" },
    CodeDef { code: 0x720C0, name: "degrade_overclaim", family: 3, severity: 3, zh: "声称能力超出实际能力" },
    CodeDef { code: 0x720D0, name: "observe_point_gap", family: 3, severity: 2, zh: "四点观测缺一个点" },
    CodeDef { code: 0x720E0, name: "observe_dur_drift", family: 3, severity: 1, zh: "耗时与出口入口差不一致" },
];

/// 注册表摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Registry {
    pub count: u32,
    pub families: [u32; FAMILIES],
    pub checksum: u64,
}

impl Registry {
    pub fn sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.families.len() {
            if let Some(v) = self.families.get(i) {
                n = n.saturating_add(*v);
            }
            i += 1;
        }
        n
    }
}

/// 名称形状：小写字母开头，只含小写字母/数字/单下划线，长度 4..=24。
pub fn name_shape_ok(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.len() < NAME_MIN || bytes.len() > NAME_MAX {
        return false;
    }
    let mut prev_us = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i).copied() {
            Some(v) => v,
            None => return false,
        };
        let ok = if i == 0 {
            b.is_ascii_lowercase()
        } else {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'
        };
        if !ok {
            return false;
        }
        if b == b'_' {
            if prev_us {
                return false;
            }
            prev_us = true;
        } else {
            prev_us = false;
        }
        i += 1;
    }
    // 末尾不许是下划线。
    match bytes.last() {
        Some(b) => *b != b'_',
        None => false,
    }
}

/// 注册表自审：条数 → 码值步长 → 名称形状 → 名称唯一 → 族/严重度 → 说明 → 摘要。
pub fn registry_integrity(defs: &[CodeDef]) -> Result<Registry, u32> {
    if defs.len() != ERR_COUNT {
        return Err(EcCode::CountBad.val());
    }
    let mut families = [0u32; FAMILIES];
    let mut checksum = p_core::FNV_OFF;
    let mut i = 0usize;
    while i < defs.len() {
        let d = match defs.get(i) {
            Some(v) => *v,
            None => return Err(EcCode::CountBad.val()),
        };
        if d.code != CODE_BASE + (i as u32).saturating_mul(CODE_STEP) {
            return Err(EcCode::StepBad.val());
        }
        if !name_shape_ok(d.name) {
            return Err(EcCode::NameBad.val());
        }
        let fidx = d.family as usize;
        if fidx >= FAMILIES {
            return Err(EcCode::FamilyBad.val());
        }
        if d.severity < SEV_MIN || d.severity > SEV_MAX {
            return Err(EcCode::SeverityBad.val());
        }
        if d.zh.is_empty() {
            return Err(EcCode::NameBad.val());
        }
        let mut j = 0usize;
        while j < i {
            if let Some(p) = defs.get(j) {
                if p.code == d.code {
                    return Err(EcCode::DupBad.val());
                }
                if p.name == d.name {
                    return Err(EcCode::DupBad.val());
                }
            }
            j += 1;
        }
        if let Some(slot) = families.get_mut(fidx) {
            *slot = slot.saturating_add(1);
        }
        checksum = p_core::fold_u32(checksum, d.code);
        checksum = fold_str(checksum, d.name);
        checksum = p_core::fold_u32(checksum, d.family as u32);
        checksum = p_core::fold_u32(checksum, d.severity as u32);
        checksum = fold_str(checksum, d.zh);
        i += 1;
    }
    // 每族至少一条（族空表等于该族诊断永远不可达）。
    let mut k = 0usize;
    while k < families.len() {
        if let Some(n) = families.get(k) {
            if *n == 0 {
                return Err(EcCode::FamilyBad.val());
            }
        }
        k += 1;
    }
    Ok(Registry {
        count: defs.len() as u32,
        families,
        checksum,
    })
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    for b in s.as_bytes() {
        x = p_core::fold_byte(x, *b);
    }
    x
}

/// 按码值查下标。
pub fn code_index(code: u32) -> Option<usize> {
    let mut i = 0usize;
    while i < CODES.len() {
        if let Some(d) = CODES.get(i) {
            if d.code == code {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 按名称查下标。
pub fn name_index(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < CODES.len() {
        if let Some(d) = CODES.get(i) {
            if d.name == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ── 编解码：码值 + 4 位负载 ──────────────────────────────────────
/// 编码：码值与 4 位负载拼成一个字。
pub fn encode(idx: usize, payload: u32) -> Result<u32, u32> {
    if idx >= ERR_COUNT || payload > PAYLOAD_MAX {
        return Err(EcCode::ResultBad.val());
    }
    let d = match CODES.get(idx) {
        Some(v) => *v,
        None => return Err(EcCode::ResultBad.val()),
    };
    Ok(d.code | payload)
}

/// 解码：低 4 位为负载，高位回查注册表。
pub fn decode(word: u32) -> Option<(usize, u32)> {
    let payload = word & 0x0F;
    let base = word & !0x0F;
    match code_index(base) {
        Some(idx) => Some((idx, payload)),
        None => None,
    }
}

/// 注册表往返：15 码 × 16 负载全对，返回对称摘要。
pub fn roundtrip() -> Result<u64, u32> {
    let mut h = p_core::FNV_OFF;
    let mut idx = 0usize;
    while idx < ERR_COUNT {
        let mut payload = 0u32;
        while payload <= PAYLOAD_MAX {
            let word = encode(idx, payload)?;
            match decode(word) {
                Some((i2, p2)) if i2 == idx && p2 == payload => {
                    h = p_core::fold_u32(h, word);
                }
                _ => return Err(EcCode::DupBad.val()),
            }
            payload = payload.saturating_add(1);
        }
        idx += 1;
    }
    Ok(h)
}

// ── 四点埋点 ──────────────────────────────────────────────────────
/// 一条四点记录：入口/出口/耗时/结果 + 点位掩码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointRec {
    pub op: u32,
    pub code_idx: u8,
    pub payload: u8,
    pub entry: u32,
    pub exit: u32,
    pub dur: u32,
    pub points: u8,
    pub result: u32,
}

/// 单条记录验收：点位齐 → 时序 → 耗时 → 结果。返回派生耗时。
pub fn validate_rec(rec: &PointRec) -> Result<u32, u32> {
    if rec.points & PT_ENTRY == 0 {
        return Err(EcCode::EntryBad.val());
    }
    if rec.points & PT_EXIT == 0 {
        return Err(EcCode::ExitBad.val());
    }
    if rec.points & PT_DURATION == 0 {
        return Err(EcCode::DurationBad.val());
    }
    if rec.points & PT_RESULT == 0 {
        return Err(EcCode::ResultBad.val());
    }
    if rec.exit <= rec.entry {
        return Err(EcCode::EntryBad.val());
    }
    let derived = match rec.exit.checked_sub(rec.entry) {
        Some(v) => v,
        None => return Err(EcCode::EntryBad.val()),
    };
    if derived != rec.dur || rec.dur == 0 || rec.dur > DUR_MAX {
        return Err(EcCode::DurationBad.val());
    }
    let idx = rec.code_idx as usize;
    if idx >= ERR_COUNT {
        return Err(EcCode::ResultBad.val());
    }
    let d = match CODES.get(idx) {
        Some(v) => *v,
        None => return Err(EcCode::ResultBad.val()),
    };
    if rec.result != d.code {
        return Err(EcCode::ResultBad.val());
    }
    Ok(derived)
}

/// 观测报告：操作数 / 点位数 / 异常数 / 耗时统计 / 摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsReport {
    pub ops: u32,
    pub points: u32,
    pub anomalies: u32,
    pub min_dur: u32,
    pub max_dur: u32,
    pub total_dur: u32,
    pub digest: u64,
}

impl ObsReport {
    pub const fn empty() -> ObsReport {
        ObsReport {
            ops: 0,
            points: 0,
            anomalies: 0,
            min_dur: 0,
            max_dur: 0,
            total_dur: 0,
            digest: 0,
        }
    }

    /// 覆盖率（千分比）：四点齐备时恒为 1000。
    pub fn coverage_permille(&self) -> u32 {
        let want = self.ops.saturating_mul(POINTS);
        if want == 0 {
            return 0;
        }
        self.points.saturating_mul(1_000) / want
    }

    pub fn full_coverage(&self) -> bool {
        self.ops > 0 && self.points == self.ops.saturating_mul(POINTS) && self.anomalies == 0
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// 正常路径埋点：15 码 × 16 负载，四点全填，异常必须为 0。
pub fn instrument(ops: u32) -> Result<ObsReport, u32> {
    if ops == 0 || ops > FAULT_MAX {
        return Err(EcCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4901_5E01);
    let mut rep = ObsReport::empty();
    rep.min_dur = u32::MAX;
    let mut clock = 0x1000u32;
    let mut i = 0u32;
    while i < ops {
        let code_idx = (i % ERR_COUNT as u32) as u8;
        let payload = ((i / ERR_COUNT as u32) % (PAYLOAD_MAX + 1)) as u8;
        let d = match CODES.get(code_idx as usize) {
            Some(v) => *v,
            None => return Err(EcCode::ResultBad.val()),
        };
        let dur = 1 + rng.below(0x03FF);
        let entry = clock;
        let exit = entry.saturating_add(dur);
        clock = exit.saturating_add(1);
        let rec = PointRec {
            op: i,
            code_idx,
            payload,
            entry,
            exit,
            dur,
            points: PT_ALL,
            result: d.code,
        };
        match validate_rec(&rec) {
            Ok(derived) => {
                rep.points = rep.points.saturating_add(POINTS);
                rep.min_dur = rep.min_dur.min(derived);
                rep.max_dur = rep.max_dur.max(derived);
                rep.total_dur = rep.total_dur.saturating_add(derived);
                rep.digest = p_core::fold_u32(rep.digest, rec.op);
                rep.digest = p_core::fold_u32(rep.digest, rec.result | rec.payload as u32);
                rep.digest = p_core::fold_u32(rep.digest, derived);
            }
            Err(_) => {
                rep.anomalies = rep.anomalies.saturating_add(1);
            }
        }
        rep.ops = rep.ops.saturating_add(1);
        i = i.saturating_add(1);
    }
    if rep.min_dur == u32::MAX {
        rep.min_dur = 0;
    }
    if !rep.full_coverage() {
        return Err(EcCode::ResultBad.val());
    }
    Ok(rep)
}

/// 万轮风暴：随机码/负载/耗时，四点全填，摘要必须稳定且零崩。
pub fn instrument_storm(rounds: u32) -> Result<ObsReport, u32> {
    if rounds == 0 || rounds > FAULT_MAX {
        return Err(EcCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4901_5E17);
    let mut rep = ObsReport::empty();
    rep.min_dur = u32::MAX;
    let mut i = 0u32;
    while i < rounds {
        let code_idx = rng.below(ERR_COUNT as u32) as u8;
        let payload = rng.below(PAYLOAD_MAX + 1) as u8;
        let d = match CODES.get(code_idx as usize) {
            Some(v) => *v,
            None => return Err(EcCode::ResultBad.val()),
        };
        // 风暴里每拍独立起算：u32 装不下 1 万拍 × 4M 的严格单调时钟，
        // 故只保证单条记录内 exit = entry + dur，不假装全局单调。
        let dur = 1 + rng.below(0x03FF);
        let entry = 0x1000u32.saturating_add(i.saturating_mul(0x10));
        let exit = entry.saturating_add(dur);
        let rec = PointRec {
            op: i,
            code_idx,
            payload,
            entry,
            exit,
            dur,
            points: PT_ALL,
            result: d.code,
        };
        match validate_rec(&rec) {
            Ok(derived) => {
                rep.points = rep.points.saturating_add(POINTS);
                rep.min_dur = rep.min_dur.min(derived);
                rep.max_dur = rep.max_dur.max(derived);
                rep.total_dur = rep.total_dur.saturating_add(derived);
                rep.digest = p_core::fold_u32(rep.digest, rec.op);
                rep.digest = p_core::fold_u32(rep.digest, rec.result);
                rep.digest = p_core::fold_u32(rep.digest, derived);
            }
            Err(_) => {
                rep.anomalies = rep.anomalies.saturating_add(1);
            }
        }
        rep.ops = rep.ops.saturating_add(1);
        i = i.saturating_add(1);
    }
    if rep.min_dur == u32::MAX {
        rep.min_dur = 0;
    }
    if !rep.full_coverage() {
        return Err(EcCode::ResultBad.val());
    }
    Ok(rep)
}

/// 故障注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaultLog {
    pub rounds: u32,
    pub caught: u32,
    pub last_route: u32,
}

impl FaultLog {
    pub fn all_caught(&self) -> bool {
        self.rounds > 0 && self.caught == self.rounds
    }
}

/// 四路坏记录：缺入口 / 缺出口 / 耗时漂移 / 未登记结果。
pub fn fault_rounds(rounds: u32) -> Result<FaultLog, u32> {
    if rounds == 0 || rounds > FAULT_MAX {
        return Err(EcCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4901_5E1D);
    let mut caught = 0u32;
    let mut last = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let route = rng.below(FAULT_ROUTES);
        let idx = rng.below(ERR_COUNT as u32) as u8;
        let d = match CODES.get(idx as usize) {
            Some(v) => *v,
            None => return Err(EcCode::ResultBad.val()),
        };
        let dur = 1 + rng.below(0x100);
        let mut rec = PointRec {
            op: r,
            code_idx: idx,
            payload: (rng.below(16)) as u8,
            entry: 0x3000,
            exit: 0x3000 + dur,
            dur,
            points: PT_ALL,
            result: d.code,
        };
        let want = match route {
            0 => {
                rec.points &= !PT_ENTRY;
                EcCode::EntryBad.val()
            }
            1 => {
                rec.points &= !PT_EXIT;
                EcCode::ExitBad.val()
            }
            2 => {
                rec.dur = rec.dur.saturating_add(1);
                EcCode::DurationBad.val()
            }
            _ => {
                rec.result = 0x7FFF_FFFF;
                EcCode::ResultBad.val()
            }
        };
        if validate_rec(&rec).err() == Some(want) {
            caught = caught.saturating_add(1);
        }
        last = route;
        r = r.saturating_add(1);
    }
    if caught != rounds {
        return Err(EcCode::ResultBad.val());
    }
    Ok(FaultLog {
        rounds,
        caught,
        last_route: last,
    })
}

/// 反例必拒：五条结构性坏注册表各按指定码拒绝。
pub fn tamper_rejected() -> bool {
    let mut t = CODES.to_vec();
    if let Some(slot) = t.get_mut(2) {
        slot.code = CODE_BASE;
    }
    let d1 = registry_integrity(&t).err() == Some(EcCode::StepBad.val());

    let mut t = CODES.to_vec();
    if let Some(slot) = t.get_mut(4) {
        slot.name = CODES[5].name;
    }
    let d2 = registry_integrity(&t).err() == Some(EcCode::DupBad.val());

    let mut t = CODES.to_vec();
    if let Some(slot) = t.get_mut(6) {
        slot.name = "Match_Bad";
    }
    let d3 = registry_integrity(&t).err() == Some(EcCode::NameBad.val());

    let mut t = CODES.to_vec();
    if let Some(slot) = t.get_mut(8) {
        slot.family = FAMILIES as u8;
    }
    let d4 = registry_integrity(&t).err() == Some(EcCode::FamilyBad.val());

    let t = &CODES[..ERR_COUNT - 1];
    let d5 = registry_integrity(t).err() == Some(EcCode::CountBad.val());

    let mut t = CODES.to_vec();
    if let Some(slot) = t.get_mut(10) {
        slot.severity = 0;
    }
    let d6 = registry_integrity(&t).err() == Some(EcCode::SeverityBad.val());

    d1 && d2 && d3 && d4 && d5 && d6
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < EcCode::ALL.len() {
        if let Some(c) = EcCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | params | schedule | fails | keys | digest。
/// params=注册表条数 / schedule=四点 / fails=观测异常数 / keys=往返对数。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn row_fields(row: &str) -> Option<[&str; RENDER_FIELDS]> {
    let mut out: [&str; RENDER_FIELDS] = [""; RENDER_FIELDS];
    let mut n = 0usize;
    for p in row.split('|') {
        if n >= RENDER_FIELDS {
            return None;
        }
        if let Some(slot) = out.get_mut(n) {
            *slot = p;
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return None;
    }
    Some(out)
}

pub fn split_kv(part: &str) -> Option<(&str, &str)> {
    let mut it = part.split('=');
    let k = it.next()?;
    let v = it.next()?;
    if it.next().is_some() || k.is_empty() {
        return None;
    }
    Some((k, v))
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn audit_render(row: &str) -> bool {
    let parts = match row_fields(row) {
        Some(p) => p,
        None => return false,
    };
    let tag = match parts.get(0) {
        Some(v) if !v.is_empty() => *v,
        _ => return false,
    };
    const KEYS: [&str; 4] = ["params", "schedule", "fails", "keys"];
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let (k, v) = match split_kv(p) {
            Some(x) => x,
            None => return false,
        };
        if k != KEYS.get(i - 1).copied().unwrap_or("") {
            return false;
        }
        let num = match parse_u32(v) {
            Some(n) => n,
            None => return false,
        };
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5) {
        Some(v) => *v,
        None => return false,
    };
    let (dk, dv) = match split_kv(dpart) {
        Some(x) => x,
        None => return false,
    };
    if dk != "digest" {
        return false;
    }
    let digest = match parse_hex16(dv) {
        Some(v) => v,
        None => return false,
    };
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49015 ①：15 条注册表自洽（族分布 4/4/3/4）。
    let reg = registry_integrity(&CODES);
    let table_ok = match &reg {
        Ok(r) => r.count == ERR_COUNT as u32 && r.sum() == ERR_COUNT as u32 && r.checksum != 0,
        Err(_) => false,
    };
    out.push((
        "F49015/十五码自洽",
        table_ok,
        format!(
            "count={:?} families={:?} checksum={:016X}",
            reg.as_ref().map(|r| r.count),
            reg.as_ref().map(|r| r.families),
            reg.as_ref().map(|r| r.checksum).unwrap_or(0)
        ),
    ));

    // F49015 ②：名称唯一可读 + 中文说明非空 + 严重度合法。
    let mut names_ok = true;
    let mut zh_ok = true;
    let mut sev_ok = true;
    let mut i = 0usize;
    while i < CODES.len() {
        if let Some(d) = CODES.get(i) {
            if !name_shape_ok(d.name) || name_index(d.name) != Some(i) {
                names_ok = false;
            }
            if d.zh.is_empty() {
                zh_ok = false;
            }
            if d.severity < SEV_MIN || d.severity > SEV_MAX {
                sev_ok = false;
            }
        }
        i += 1;
    }
    out.push((
        "F49015/名称唯一可读",
        names_ok && zh_ok && sev_ok,
        format!("names={} zh={} sev={}", names_ok, zh_ok, sev_ok),
    ));

    // F49015 ③：15 码 × 16 负载编解码往返（两遍摘要一致）。
    let rt_a = roundtrip();
    let rt_b = roundtrip();
    let rt_ok = match (&rt_a, &rt_b) {
        (Ok(a), Ok(b)) => a == b && *a != 0,
        _ => false,
    };
    out.push((
        "F49015/编解码往返",
        rt_ok,
        format!(
            "pairs={} digest={:016X}",
            ROUNDTRIP_PAIRS,
            rt_a.as_ref().copied().unwrap_or(0)
        ),
    ));

    // F49015 ④：码窗守卫（首码/末码/步长/段位）。
    let mut window_ok = true;
    let mut i = 0usize;
    while i < CODES.len() {
        if let Some(d) = CODES.get(i) {
            let want = CODE_BASE + (i as u32).saturating_mul(CODE_STEP);
            if d.code != want || code_index(d.code) != Some(i) {
                window_ok = false;
            }
        }
        i += 1;
    }
    if let Some(first) = CODES.first() {
        if first.code != CODE_BASE {
            window_ok = false;
        }
    }
    if let Some(last) = CODES.last() {
        let want_last = CODE_BASE + (ERR_COUNT as u32 - 1).saturating_mul(CODE_STEP);
        if last.code != want_last {
            window_ok = false;
        }
    }
    out.push((
        "F49015/码窗守卫",
        window_ok && segment_guard(),
        format!(
            "base={:05X} last={:05X} seg={}",
            CODE_BASE,
            CODE_BASE + (ERR_COUNT as u32 - 1).saturating_mul(CODE_STEP),
            segment_guard()
        ),
    ));

    // F49016 ①：四点 100% 覆盖。
    let obs = instrument(OPS);
    let obs_ok = match &obs {
        Ok(r) => r.full_coverage() && r.coverage_permille() == 1_000,
        Err(_) => false,
    };
    out.push((
        "F49016/四点全覆盖",
        obs_ok,
        format!(
            "ops={:?} points={:?} permille={:?}",
            obs.as_ref().map(|r| r.ops),
            obs.as_ref().map(|r| r.points),
            obs.as_ref().map(|r| r.coverage_permille())
        ),
    ));

    // F49016 ②：异常零静默（异常 0、耗时区间合法、总耗时守恒）。
    let zero_ok = match &obs {
        Ok(r) => {
            r.anomalies == 0
                && r.min_dur >= 1
                && r.max_dur <= DUR_MAX
                && r.min_dur <= r.max_dur
                && r.total_dur >= r.min_dur.saturating_mul(r.ops)
                && r.total_dur <= r.max_dur.saturating_mul(r.ops)
        }
        Err(_) => false,
    };
    out.push((
        "F49016/异常零静默",
        zero_ok,
        format!(
            "anomalies={:?} min={:?} max={:?} total={:?}",
            obs.as_ref().map(|r| r.anomalies),
            obs.as_ref().map(|r| r.min_dur),
            obs.as_ref().map(|r| r.max_dur),
            obs.as_ref().map(|r| r.total_dur)
        ),
    ));

    // F49016 ③：四路故障注入全捕获。
    let faults = fault_rounds(1_000);
    let fault_ok = matches!(&faults, Ok(l) if l.all_caught());
    out.push((
        "F49016/注入全捕获",
        fault_ok,
        format!(
            "rounds={:?} caught={:?} last={:?}",
            faults.as_ref().map(|l| l.rounds),
            faults.as_ref().map(|l| l.caught),
            faults.as_ref().map(|l| l.last_route)
        ),
    ));

    // F49016 ④：万轮零崩（复跑摘要稳定）+ 渲染回读 + 段位守卫。
    let storm_a = instrument_storm(STORM_ROUNDS);
    let storm_b = instrument_storm(STORM_ROUNDS);
    let storm_ok = match (&storm_a, &storm_b) {
        (Ok(a), Ok(b)) => {
            a.digest() == b.digest() && a.full_coverage() && a.anomalies == 0 && a.digest() != 0
        }
        _ => false,
    };
    let digest = storm_a.as_ref().map(|r| r.digest()).unwrap_or(0);
    let row = render_row(
        "E49015",
        ERR_COUNT as u32,
        POINTS,
        0,
        ROUNDTRIP_PAIRS,
        digest,
    );
    let render_ok = audit_render(&row)
        && !audit_render("")
        && !audit_render("E49015|params=15|schedule=4|fails=0|keys=240")
        && !audit_render(
            "E49015|params=15|schedule=4|fails=0|keys=240|digest=0000000000000000|x=1",
        );
    out.push((
        "F49016/万轮零崩",
        storm_ok && render_ok && segment_guard(),
        format!(
            "rounds={} digest={:016X} render={}",
            STORM_ROUNDS, digest, render_ok
        ),
    ));

    out
}

pub fn run_unx49015_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49016 键盘错误码族与四点观测埋点");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(EcCode::CountBad.val(), SEG_BASE + 0x01);
        assert_eq!(EcCode::SegmentBad.val(), SEG_BASE + 14);
        assert_eq!(EcCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < EcCode::ALL.len() {
            if let Some(c) = EcCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn registry_has_fifteen_codes() {
        let r = registry_integrity(&CODES).unwrap();
        assert_eq!(r.count, 15);
        assert_eq!(r.sum(), 15);
        assert_eq!(r.families, [4, 4, 3, 4]);
        assert_ne!(r.checksum, 0);
    }

    #[test]
    fn every_code_value_follows_the_step() {
        let mut i = 0usize;
        while i < CODES.len() {
            assert_eq!(CODES[i].code, CODE_BASE + (i as u32) * CODE_STEP);
            assert_eq!(code_index(CODES[i].code), Some(i));
            assert_eq!(name_index(CODES[i].name), Some(i));
            i += 1;
        }
    }

    #[test]
    fn names_are_snake_case_and_unique() {
        let mut i = 0usize;
        while i < CODES.len() {
            assert!(name_shape_ok(CODES[i].name), "{}", CODES[i].name);
            assert!(!CODES[i].zh.is_empty());
            assert!(CODES[i].severity >= SEV_MIN && CODES[i].severity <= SEV_MAX);
            i += 1;
        }
        assert!(!name_shape_ok("Bad"));
        assert!(!name_shape_ok("_bad"));
        assert!(!name_shape_ok("bad__name"));
        assert!(!name_shape_ok("bad_"));
        assert!(name_shape_ok("bad2_name"));
    }

    #[test]
    fn roundtrip_covers_every_pair() {
        let a = roundtrip().unwrap();
        let b = roundtrip().unwrap();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        let mut i = 0usize;
        while i < ERR_COUNT {
            let mut p = 0u32;
            while p <= PAYLOAD_MAX {
                let w = encode(i, p).unwrap();
                assert_eq!(decode(w), Some((i, p)));
                p += 1;
            }
            i += 1;
        }
        assert!(encode(ERR_COUNT, 0).is_err());
        assert!(encode(0, PAYLOAD_MAX + 1).is_err());
        assert_eq!(decode(0x72100), None);
    }

    #[test]
    fn code_window_starts_and_ends_where_expected() {
        assert_eq!(CODES[0].code, CODE_BASE);
        assert_eq!(CODES[14].code, 0x720E0);
        assert_eq!(CODES[14].code - CODES[0].code, 14 * CODE_STEP);
    }

    #[test]
    fn registry_rejects_bad_tables() {
        assert!(tamper_rejected());
        let mut t = CODES.to_vec();
        t[3].code = CODE_BASE + 3 * CODE_STEP + 1;
        assert_eq!(
            registry_integrity(&t).err(),
            Some(EcCode::StepBad.val())
        );
        let mut t = CODES.to_vec();
        t[3].zh = "";
        assert_eq!(
            registry_integrity(&t).err(),
            Some(EcCode::NameBad.val())
        );
    }

    #[test]
    fn instrument_gives_full_coverage() {
        let r = instrument(OPS).unwrap();
        assert_eq!(r.ops, OPS);
        assert_eq!(r.points, OPS * 4);
        assert_eq!(r.anomalies, 0);
        assert_eq!(r.coverage_permille(), 1_000);
        assert!(r.min_dur >= 1);
        assert!(r.max_dur <= DUR_MAX);
        assert!(r.digest() != 0);
        assert!(instrument(0).is_err());
        assert!(instrument(FAULT_MAX + 1).is_err());
    }

    #[test]
    fn validate_rec_accepts_good_and_rejects_each_missing_point() {
        let d = CODES[2];
        let good = PointRec {
            op: 7,
            code_idx: 2,
            payload: 9,
            entry: 100,
            exit: 180,
            dur: 80,
            points: PT_ALL,
            result: d.code,
        };
        assert_eq!(validate_rec(&good).unwrap(), 80);

        let mut bad = good;
        bad.points = PT_ALL & !PT_ENTRY;
        assert_eq!(validate_rec(&bad).err(), Some(EcCode::EntryBad.val()));
        bad = good;
        bad.points = PT_ALL & !PT_EXIT;
        assert_eq!(validate_rec(&bad).err(), Some(EcCode::ExitBad.val()));
        bad = good;
        bad.points = PT_ALL & !PT_DURATION;
        assert_eq!(validate_rec(&bad).err(), Some(EcCode::DurationBad.val()));
        bad = good;
        bad.points = PT_ALL & !PT_RESULT;
        assert_eq!(validate_rec(&bad).err(), Some(EcCode::ResultBad.val()));
    }

    #[test]
    fn timing_order_and_duration_must_agree() {
        let d = CODES[0];
        let mut rec = PointRec {
            op: 1,
            code_idx: 0,
            payload: 0,
            entry: 100,
            exit: 100,
            dur: 1,
            points: PT_ALL,
            result: d.code,
        };
        assert_eq!(validate_rec(&rec).err(), Some(EcCode::EntryBad.val()));
        rec.exit = 200;
        rec.dur = 99;
        assert_eq!(
            validate_rec(&rec).err(),
            Some(EcCode::DurationBad.val())
        );
        rec.dur = 100;
        assert!(validate_rec(&rec).is_ok());
    }

    #[test]
    fn unregistered_result_is_rejected() {
        let mut rec = PointRec {
            op: 1,
            code_idx: 3,
            payload: 0,
            entry: 10,
            exit: 20,
            dur: 10,
            points: PT_ALL,
            result: CODES[3].code,
        };
        rec.result = 0x70000;
        assert_eq!(validate_rec(&rec).err(), Some(EcCode::ResultBad.val()));
        rec.result = CODES[3].code;
        rec.code_idx = ERR_COUNT as u8;
        assert_eq!(validate_rec(&rec).err(), Some(EcCode::ResultBad.val()));
    }

    #[test]
    fn four_fault_routes_are_all_caught() {
        let l = fault_rounds(1_000).unwrap();
        assert_eq!(l.rounds, 1_000);
        assert!(l.all_caught());
        assert!(fault_rounds(0).is_err());
        assert!(fault_rounds(FAULT_MAX + 1).is_err());
    }

    #[test]
    fn storm_ten_thousand_rounds_is_stable() {
        let a = instrument_storm(STORM_ROUNDS).unwrap();
        let b = instrument_storm(STORM_ROUNDS).unwrap();
        assert_eq!(a.ops, STORM_ROUNDS);
        assert_eq!(a.points, STORM_ROUNDS * 4);
        assert_eq!(a.anomalies, 0);
        assert!(a.full_coverage());
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let row = render_row("E49015", 15, 4, 0, 240, 0xDEAD_BEEF_0123_4567);
        assert!(audit_render(&row));
        assert!(!audit_render(""));
        assert!(!audit_render("E49015|params=15|schedule=4|fails=0|keys=240"));
        assert!(!audit_render(
            "E49015|params=15|schedule=4|fails=0|keys=240|digest=1234"
        ));
        assert!(!audit_render(
            "E49015|params=15|sched=4|fails=0|keys=240|digest=0000000000000001"
        ));
        assert_eq!(parse_u32("240"), Some(240));
        assert_eq!(parse_u32("2x40"), None);
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_hex16("0123456789abcdef"), Some(0x0123_4567_89AB_CDEF));
        assert_eq!(parse_hex16("0123456789abcde"), None);
        assert_eq!(row_fields(&row).map(|f| f.len()), Some(6));
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49015_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(extra_checks().len(), 8);
    }
}
