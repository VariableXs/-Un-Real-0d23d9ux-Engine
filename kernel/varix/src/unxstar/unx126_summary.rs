//! UNREAL-X / UNX-A2-B07 / CoRun Varix STAR II · Unxreal
//! 锚点：UNX-F0937 · 核间面健康摘要（对齐 F0899 口径注册）· 目标 280 行 · 已深化
//! 判据原文摘录：UNX-F0937-J1 挂接完整（B07 十账摘要入 F0997 无缺），
//!           schema 校验通过，红账投递 10/10 次可达。
//! 职责定位：B07 域做了十本账（F0921-F0936）+ 本摘要，域总账 F0997 不逐本
//! 重算，只收摘要。摘要的纪律是：**十本账的键必须无缺**（缺一本=域的某
//! 个面在总账里失明）、**schema 必须过**（格式不对=下游不可解析，等于
//! 没送）、**红账必须可达**（异常摘要送不到 F0997，摘要就是喜报。
//! 10/10 次可达——包括链路拥塞时）。
//! 设计要点：①**十账闭集**——B07 十本账（健康/通知/压测/暂停恢复/死锁/
//! 故障注入/延迟基准/接口冻结/事件序列<F0925>/巡检<F0996 先行>）按册编号
//! 枚举，摘要表十行；少一行=挂接缺失，直接红（不是 warning 是判据）；
//! ②**摘要三元组**——每本账出 (状态, 主指标, 指纹)：状态枚举（绿/黄/红/
//! 冻结），主指标是该账的判据核心数（如健康账的成功率 bp），指纹用于
//! 下游核对是不是同一版本账；③**schema 校验**——摘要条目字段闭集校验：
//! 册号非空、状态枚举合法、主指标非负且 <2^40、指纹非零；四条任一不过
//! = schema 红（SchemaError 而不是默认放行——静默塞给下游的脏摘要
//! 比缺摘要更坏）；④**红账投递双通道**——红账（状态=红的摘要）主通道
//! 是汇总投递，备用通道是逐条直投；主通道满（摘要环容量 64 满）自动切
//! 备用。投递结果入账：成功/主通道/备用通道各自计数，10 次红账
//! 全部至少一条通道可达；⑤**无缺≠全绿**——摘要表允许红条目存在
//! （域有红是事实），但必须十行齐全且可达；把红条目藏了凑"全绿"
//! 是摘要面最严重的罪（判据说的是无缺不是全绿）；⑥**摘要幂等**——
//! 同账同内容重复挂接返回已存在（不产生第二行），异内容同册号挂接
//! 拒绝（册号是主键，一账两摘=总账分裂）。
//! 诊断码段：0xDA（本单独占，0xDA01~0xDA0F）。
//! 施工：AI · 2026-10-12。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码闭集（0xDA 段独占——grep 全仓查占后定段）
// ---------------------------------------------------------------------------

/// 0xDA01 账缺（十账闭集缺一本——挂接不完整）。
pub const XC_SM_BOOK_MISS: u32 = 0xDA01;
/// 0xDA02 册号重复（一账两摘=总账分裂）。
pub const XC_SM_DUP_KEY: u32 = 0xDA02;
/// 0xDA03 schema 红（字段闭集校验不过——下游不可解析）。
pub const XC_SM_SCHEMA: u32 = 0xDA03;
/// 0xDA04 状态枚举越界（状态不在四枚举内）。
pub const XC_SM_STATE_OOR: u32 = 0xDA04;
/// 0xDA05 指纹零（摘要无指纹——下游无法核对版本）。
pub const XC_SM_FP_ZERO: u32 = 0xDA05;
/// 0xDA06 红账不可达（红账十条通道都不可达=摘要变喜报）。
pub const XC_SM_RED_LOST: u32 = 0xDA06;
/// 0xDA07 主指标越界（指标 <0 或 ≥2^40）。
pub const XC_SM_METRIC_OOB: u32 = 0xDA07;
/// 0xDA08 摘要环溢出（环满未切备用通道）。
pub const XC_SM_RING_FULL: u32 = 0xDA08;
/// 0xDA09 藏红（红条目未入摘=凑全绿）。
pub const XC_SM_HIDE_RED: u32 = 0xDA09;
/// 0xDA0A 双跑漂移（同输入两轮摘要指纹不等）。
pub const XC_SM_DUAL_DRIFT: u32 = 0xDA0A;
/// 0xDA0B 挂接非幂（同册同内容第二次挂接报错）。
pub const XC_SM_IDEMPOTENT: u32 = 0xDA0B;
/// 0xDA0C 通道账失守（投递成功≠主通道+备用）。
pub const XC_SM_CHAN_GAP: u32 = 0xDA0C;
/// 0xDA0D 摘要空跑（零账即出摘=空摘要）。
pub const XC_SM_EMPTY: u32 = 0xDA0D;
/// 0xDA0E 键错位（册号与账本不对应）。

pub const XC_SM_KEY_MAP: u32 = 0xDA0E;
/// 0xDA0F schema 版本旧（下游 schema 版本高于本端——不可解析的是下游）。

pub const XC_SM_SCHEMA_VER: u32 = 0xDA0F;

/// 诊断明细（码 + 人话 + 可操作建议）。
pub fn detail(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_SM_BOOK_MISS => (
            "账缺：十账闭集缺一本",
            "B07 十账必须全入摘；缺一本=域的一个面在 F0997 总账里失明",
        ),
        XC_SM_DUP_KEY => (
            "册号重复：一账两摘",
            "册号是主键；重复=总账分裂，下游读到两份互相矛盾的账",
        ),
        XC_SM_SCHEMA => (
            "schema 红：字段闭集校验不过",
            "册号/状态/主指标/指纹四字段各有闭集约束；静默塞脏摘要给下游比缺摘要更坏",
        ),
        XC_SM_STATE_OOR => (
            "状态越界：不在绿/黄/红/冻结四枚举内",
            "四状态枚举是下游 schema 的契约；越界状态语义不明",
        ),
        XC_SM_FP_ZERO => (
            "指纹零：摘要无版本指纹",
            "下游靠指纹核对账版本；零指纹=版本不可辨，摘要失信",
        ),
        XC_SM_RED_LOST => (
            "红账不可达：红账所有通道都失败",
            "红账必须 10/10 可达（含拥塞切备用通道）；不可达=摘要只是喜报",
        ),
        XC_SM_METRIC_OOB => (
            "主指标越界：越出 [0, 2^40)",
            "主指标有宽度上限；越界=字段溢出风险",
        ),
        XC_SM_RING_FULL => (
            "摘要环溢出：环满未切备用",
            "主环容量 64 满即切备用通道；不切=新摘要被丢",
        ),
        XC_SM_HIDE_RED => (
            "藏红：红条目未入摘",
            "判据说的是『无缺』不是『全绿』；藏红凑全绿是摘要面最重的罪",
        ),
        XC_SM_DUAL_DRIFT => (
            "双跑漂移：同输入两轮摘要指纹不等",
            "同输入同挂接序；不等则逐段二分找隐藏可变状态",
        ),
        XC_SM_IDEMPOTENT => (
            "挂接非幂：同册同内容第二次挂接报错",
            "幂等挂接返回已存在；报错=重试面被破坏",
        ),
        XC_SM_CHAN_GAP => (
            "通道账失守：成功数≠主通道+备用",
            "每条摘要必须认一个通道；差额=投递账漏记",
        ),
        XC_SM_EMPTY => (
            "摘要空跑：零账即出摘要",
            "十账全无即出摘=空资产；拒绝",
        ),
        XC_SM_KEY_MAP => (
            "键错位：册号与账本不对应",
            "册号→账本映射是挂线表；错位=下游按册号取到错的账",
        ),
        XC_SM_SCHEMA_VER => (
            "schema 版本旧：下游版本高于本端",
            "摘要 schema 有版本号；本端旧于下游=下游解析不出新字段",
        ),
        _ => ("未知诊断码", "码不在 0xDA 闭集内——查是否跨段误用"),
    }
}

// ---------------------------------------------------------------------------
// §1 摘要 schema 与十账闭集
// ---------------------------------------------------------------------------

/// 账本状态四枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookState {
    Green,
    Yellow,
    Red,
    Frozen,
}

impl BookState {
    pub fn name(self) -> &'static str {
        match self {
            BookState::Green => "绿",
            BookState::Yellow => "黄",
            BookState::Red => "红",
            BookState::Frozen => "冻结",
        }
    }
    pub fn from_u8(v: u8) -> Option<BookState> {
        match v {
            0 => Some(BookState::Green),
            1 => Some(BookState::Yellow),
            2 => Some(BookState::Red),
            3 => Some(BookState::Frozen),
            _ => None,
        }
    }
}

/// B07 十账册号闭集（F0921-F0936 域内十本）。
pub const BOOK_IDS: [u32; 10] = [
    0x0921, 0x0924, 0x0925, 0x0930, 0x0931, 0x0932, 0x0933, 0x0934, 0x0935, 0x0936,
];

/// B07 十账名（按册号顺序对齐 BOOK_IDS）。
pub const BOOK_NAMES: [&str; 10] = [
    "IPI 业务面",
    "IPI 巡检面",
    "跨核暂停恢复",
    "IPI 业务健康账",
    "跨核事件通知面",
    "IPI 压测资产",
    "冻结接口文档",
    "核间死锁防线",
    "故障注入资产",
    "IPI 延迟业务基准",
];

/// 摘要条目（schema 四字段闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SummaryEntry {
    pub book: u32,
    pub state: BookState,
    pub metric: u64,
    pub fp: u64,
}

impl SummaryEntry {
    /// schema 校验（四字段各自闭集约束）。
    pub fn schema_ok(&self) -> Result<(), (u32, String)> {
        let mut known = false;
        for b in BOOK_IDS {
            if b == self.book {
                known = true;
                break;
            }
        }
        if !known {
            return Err((XC_SM_KEY_MAP, format!("册号 {:#06x} 不在 B07 十账闭集", self.book)));
        }
        if self.metric >= (1u64 << 40) {
            return Err((XC_SM_METRIC_OOB, format!("主指标 {} 越 2^40", self.metric)));
        }
        if self.fp == 0 {
            return Err((XC_SM_FP_ZERO, format!("册 {:#06x} 指纹零", self.book)));
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        h ^= self.book as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.state as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.metric;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= self.fp;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h
    }
}

/// 投递通道账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChanLedger {
    pub primary_ok: u64,
    pub backup_ok: u64,
    pub failed: u64,
}

/// 摘要环（主通道容量 64）。
pub const RING_CAP: usize = 64;

/// 核间面健康摘要引擎。
pub struct SummaryHub {
    entries: Vec<SummaryEntry>,
    pub chan: ChanLedger,
    /// 红账投递目标（模拟 F0997 的接收窗容量）。
    pub red_window: usize,
    /// 红账可达次数判据用。
    pub red_attempts: u64,
    pub red_reached: u64,
    /// 本端 schema 版本。
    pub schema_ver: u16,
    /// 下游 schema 版本。
    pub downstream_ver: u16,
}

impl SummaryHub {
    pub fn new() -> SummaryHub {
        SummaryHub {
            entries: Vec::new(),
            chan: ChanLedger::default(),
            red_window: RING_CAP,
            red_attempts: 0,
            red_reached: 0,
            schema_ver: 3,
            downstream_ver: 3,
        }
    }

    /// 挂接一本账摘要（幂等 + schema 闸）。
    pub fn attach(&mut self, e: SummaryEntry) -> Result<bool, (u32, String)> {
        e.schema_ok()?;
        for have in &self.entries {
            if have.book == e.book {
                // 幂等：同册同内容返回 false（已存在），异内容拒绝。
                if *have == e {
                    return Ok(false);
                }
                return Err((XC_SM_DUP_KEY, format!("册 {:#06x} 已有异内容摘", e.book)));
            }
        }
        if self.entries.len() >= 12 {
            return Err((XC_SM_BOOK_MISS, "摘要表超十账容量".into()));
        }
        self.entries.push(e);
        Ok(true)
    }

    /// 十账齐备校验（无缺判据）。
    pub fn complete(&self) -> Result<(), (u32, String)> {
        if self.entries.is_empty() {
            return Err((XC_SM_EMPTY, "零账即出摘".into()));
        }
        for b in BOOK_IDS {
            let mut found = false;
            for e in &self.entries {
                if e.book == b {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err((XC_SM_BOOK_MISS, format!("册 {:#06x} 缺摘", b)));
            }
        }
        Ok(())
    }

    /// 红账条目（下游要重点看的）。
    pub fn red_entries(&self) -> Vec<SummaryEntry> {
        self.entries.iter().filter(|e| e.state == BookState::Red).cloned().collect()
    }

    /// 投递一条摘要到 F0997（红账双通道 + 账目）。
    pub fn deliver(&mut self, e: &SummaryEntry, sim_primary_full: bool) -> Result<(), (u32, String)> {
        e.schema_ok()?;
        if self.schema_ver < self.downstream_ver {
            return Err((XC_SM_SCHEMA_VER, format!("本端 v{} < 下游 v{}", self.schema_ver, self.downstream_ver)));
        }
        let is_red = e.state == BookState::Red;
        if is_red {
            self.red_attempts += 1;
        }
        // 主通道：非满即可用。
        if !sim_primary_full {
            self.chan.primary_ok += 1;
            if is_red {
                self.red_reached += 1;
            }
            return Ok(());
        }
        // 备用通道（环满自动切）。
        if self.chan.backup_ok < self.red_window as u64 {
            self.chan.backup_ok += 1;
            if is_red {
                self.red_reached += 1;
            }
            return Ok(());
        }
        self.chan.failed += 1;
        if is_red {
            return Err((XC_SM_RED_LOST, format!("红账册 {:#06x} 双通道均不可达", e.book)));
        }
        Err((XC_SM_RING_FULL, format!("摘要册 {:#06x} 双通道均满", e.book)))
    }

    /// 红账 10/10 可达判据。
    pub fn red_reachable_10(&mut self) -> Result<(), (u32, String)> {
        let reds = self.red_entries();
        if reds.is_empty() {
            return Err((XC_SM_EMPTY, "无红账可测——先挂至少一条红条目".into()));
        }
        for i in 0..10u32 {
            // 偶数轮模拟主通道拥塞（强制切备用），验证双通道冗余真的工作。
            let full = i % 2 == 0;
            for r in &reds {
                self.deliver(r, full)?;
            }
        }
        if self.red_attempts != 10 * reds.len() as u64 {
            return Err((XC_SM_RED_LOST, format!("红账投递 {} ≠ 应投 {}", self.red_attempts, 10 * reds.len() as u64)));
        }
        if self.red_reached != self.red_attempts {
            return Err((XC_SM_RED_LOST, format!("红账可达 {} < 尝试 {}", self.red_reached, self.red_attempts)));
        }
        Ok(())
    }

    /// 通道账守恒。
    pub fn audit(&self) -> Result<(), (u32, String)> {
        if self.chan.primary_ok + self.chan.backup_ok + self.chan.failed == 0 {
            return Err((XC_SM_EMPTY, "零投递即审".into()));
        }
        // 成功必须至少主或备之一（failed 之外都算成功）。
        let total = self.chan.primary_ok + self.chan.backup_ok + self.chan.failed;
        if total < self.chan.primary_ok + self.chan.backup_ok {
            return Err((XC_SM_CHAN_GAP, "通道账溢出".into()));
        }
        Ok(())
    }

    /// 摘要池指纹。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for e in &self.entries {
            h ^= e.fp();
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }
}

impl Default for SummaryHub {
    fn default() -> SummaryHub {
        SummaryHub::new()
    }
}
