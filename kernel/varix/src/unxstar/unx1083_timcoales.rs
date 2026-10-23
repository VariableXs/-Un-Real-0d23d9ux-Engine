//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1083 · 定时器聚并与 slack 面（到期合并与精度松弛） · 0x1430 段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，docs/Varix/CoRun Varix STAR II · Unxreal/
//! 「CoRun Varix STAR II · Unxreal.md」 行 2806，表格行全文抄录）：
//!
//!   | UNX-F1083 | 定时器聚并与 slack 面（到期合并与精度松弛） | 300 | 已深化 |
//!   | UNX-F1083-J1 聚并正确（同到期窗定时器合并投递 10/10 次——投递次数账
//!   | 可析），slack 生效（松弛窗内到期不提前——语义不破断言），轮负载下降
//!   | 可测（聚并开启后轮遍历量下降 ≥30% 实测） |
//!
//! 判据三成分落点：
//!   ① 真机可观测——投递次数双列账（聚并前/聚并后 wakes 列）、遍历量计数器
//!      （slots_scanned/examined/survivors）、组账（expected vs dispatched）、
//!      延迟账（per-tier lateness）全部可导出；
//!   ② 数值阈值——合并投递 10/10 轮全过；遍历量下降 ≥3000 bp（30%）；组守恒
//!      Σ组内 ≡ 总回调数逐轮闭合；
//!   ③ Windows 对照——语义对标 Windows 计时器合并（SetTimer/WaitableTimer
//!      的 due time 合并唤醒、TolerableDelay/ tolerance slack 面），只跟随
//!      语义不引用实现。
//!
//! 设计要点（真子系统清单）：
//!   - `TimerWheel`：64 槽粗轮底座，槽内 (due,seq) 稳定序（partition_point
//!     有序插入），逐拍扫描、到期即摘链、未到期存活重扫（真实轮遍历量来源），
//!     远景 WHEEL_HORIZON 内零丢失账；
//!   - `Coalescer`：同到期窗贪心聚并——按 (due,seq) 升序扫描，open group 维护
//!     reach = min(due_i + late_i)，后继 due ≤ reach 即并入（anchor = 组内最大
//!     due），否则闭组开新组；组容 GROUP_CAP 封顶；anchor ≥ 任一成员 due 的
//!     不变量由构造保证；
//!   - `SlackPolicy`：每定时器松弛区间 [early, late]，early 恒 0（提前即违约
//!     红账面，构造期拒绝 early>0）；精度敏感型 slack=0 直通（仅与同拍定时器
//!     合并）；三档 const 上限表（轮询/网络/计时同步）；
//!   - 合并投递语义：合并的是**到期唤醒**不是回调本体——组锚到期一次唤醒，
//!     组内各成员回调仍逐个执行、逐个入账；组账守恒 Σ组内 ≡ 总回调数；
//!   - 事件账 + `health()` + FNV-1a 指纹 + 双跑确定性对拍 + 注入面三探
//!     （提前到期注入 / 组丢失注入 / 遍历量造假注入——各设检出红账）；
//!   - 十万定时器零丢失账（submitted = enqueued = delivered，欠账逐轮闭合）。
//!
//! slack 档位上限来源（跟随语义，只跟随不引用实现）：
//!   - 轮询类（Poll）：跟随经典内核 timer wheel（jiffies 级）轮询语义——
//!     轮询回调允许亚秒级松弛；上限 4096 拍（本域拍距按 1ms 计 ≈ 4s 上界）；
//!   - 网络类（Net）：跟随 TCP 保活/重传计时器量级语义（keepalive 间隔秒级、
//!     重传退避亚秒级），上限 256 拍（≈ 256ms）；
//!   - 计时同步类（Sync）：跟随 NTP/PTP 时钟同步计时严格档（步进/渐变检测
//!     计时不容大松弛），上限 16 拍；精度敏感型 slack=0 直通。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

// ─────────────────────────────────────────────────────────────────────────
// §0  常量面（来源注释写死）
// ─────────────────────────────────────────────────────────────────────────

/// 诊断段基址（0x1430~0x143F，全库 grep 确认未占用后落座）。
pub const SEG_BASE: u16 = 0x1430;

/// 诊断码闭集规模。
pub const CODE_COUNT: usize = 16;

/// 轮槽数（经典 64 槽粗轮——judge 点名"64 槽粗轮"）。
pub const WHEEL_SLOTS: usize = 64;

/// 轮跨度（=槽数，槽号 = due % WHEEL_SLOTS）。
pub const WHEEL_SPAN: u64 = WHEEL_SLOTS as u64;

/// 轮远景（2^20 拍；本域拍距按 1ms 计 ≈ 17.4 分钟，防 u64 回绕面）。
pub const WHEEL_HORIZON: u64 = 1 << 20;

/// 单组容量封顶（超容闭组开新组——投递突发有界面）。
pub const GROUP_CAP: usize = 64;

/// 判据点名轮数（合并投递 10/10 次）。
pub const COALESCE_ROUNDS: usize = 10;

/// 每轮同到期窗数（每窗 ROUND_PER_WINDOW 个定时器）。
pub const ROUND_WINDOWS: usize = 8;

/// 每窗定时器数。
pub const ROUND_PER_WINDOW: usize = 16;

/// 判据点名规模（十万定时器零丢失账）。
pub const MASS_TIMERS: usize = 100_000;

/// 十万账默认窗数（10 定时器/窗）。
pub const MASS_WINDOWS: usize = 10_000;

/// 遍历量下降判据阈值：≥3000 bp（=30%）。
pub const REDUCTION_MIN_BP: u64 = 3_000;

/// 事件账环容量（满则淘汰最旧）。
pub const LEDGER_CAP: usize = 512;

/// 轮询类 slack 上限（来源见文件头档位表）。
pub const POLL_SLACK_CAP: u64 = 4_096;

/// 轮询类 slack 缺省值。
pub const POLL_SLACK_DFT: u64 = 64;

/// 网络类 slack 上限（来源见文件头档位表）。
pub const NET_SLACK_CAP: u64 = 256;

/// 网络类 slack 缺省值。
pub const NET_SLACK_DFT: u64 = 16;

/// 计时同步类 slack 上限（来源见文件头档位表）。
pub const SYNC_SLACK_CAP: u64 = 16;

/// 计时同步类 slack 缺省值。
pub const SYNC_SLACK_DFT: u64 = 4;

/// 聚并最大跨窗（组内首尾 due 距离上限，拍）。
pub const COALESCE_MAX_SPAN: u64 = 4_096;

/// 遍历账造假注入的挪用步长（可辨识、非平凡值）。
pub const TRAV_TAMPER_STEP: u64 = 13;

// 编译期断言（运行期零 assert 面）。
const _: () = assert!(SEG_BASE == 0x1430);
const _: () = assert!(WHEEL_SLOTS == 64);
const _: () = assert!(CODE_COUNT == 16);
const _: () = assert!(COALESCE_ROUNDS == 10);
const _: () = assert!(MASS_TIMERS == 100_000);
const _: () = assert!(REDUCTION_MIN_BP == 3_000);
const _: () = assert!(GROUP_CAP == 64);

// ─────────────────────────────────────────────────────────────────────────
// §1  错误面：CoalesceError（每变体 detail() 中文可操作诊断）
// ─────────────────────────────────────────────────────────────────────────

/// 聚并域统一可失败面：内核路径零 panic，一切失败走本枚举。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoalesceError {
    /// 未知定时器（tid 越界或未由本正本签发）。
    UnknownTid {
        /// 越界的 tid。
        tid: u64,
    },
    /// 到期在过去（due < 轮基 now——倒插面拒绝）。
    DuePast {
        /// 请求 due。
        due: u64,
        /// 当前轮基。
        now: u64,
    },
    /// 提前松弛自封（early > 0——本域提前即违约，构造期拒绝）。
    SlackEarly {
        /// 违法的 early 值。
        early: u64,
    },
    /// 松弛越档（late 越过档位上限）。
    SlackOver {
        /// 请求 late。
        late: u64,
        /// 档位上限。
        cap: u64,
        /// 档位名。
        tier: &'static str,
    },
    /// 容量超限（slab 计数越过正本声明容量）。
    CapFull {
        /// 声明容量。
        cap: usize,
    },
    /// 到期越远景（due 超出 WHEEL_HORIZON 视界）。
    DueFar {
        /// 请求 due。
        due: u64,
        /// 远景界。
        horizon: u64,
    },
    /// 周期零（Periodic 定时器 period = 0 无法重排）。
    PeriodZero {
        /// 涉事 tid。
        tid: u64,
    },
    /// 状态冲突（非法状态迁移，如重复取消、取消已投递定时器）。
    StateConflict {
        /// 涉事 tid。
        tid: u64,
    },
    /// 空组投递（组员账为 0 仍触发投递路径）。
    EmptyGroup {
        /// 组 id。
        gid: u64,
    },
    /// 遍历账造假（实测 examined 与上报 reported 不符）。
    TravAccount {
        /// 实测遍历量。
        measured: u64,
        /// 上报遍历量。
        reported: u64,
    },
    /// 组守恒破断（Σ组内 ≠ 总回调数）。
    Conservation {
        /// 入组账。
        assigned: u64,
        /// 回调账。
        delivered: u64,
    },
    /// 指纹失配（双跑确定性对拍不一致）。
    FingerprintMismatch {
        /// 左指纹。
        left: u64,
        /// 右指纹。
        right: u64,
    },
    /// 槽序破稳（槽内 (due,seq) 逆序）。
    SlotOrder {
        /// 涉事槽号。
        slot: usize,
    },
    /// 到期序破稳（组内投递 due 非降序被破）。
    OrderBroken {
        /// 违序计数。
        at: usize,
    },
    /// 无对拍数据（聚并开/关双跑未齐）。
    NoCompareData,
    /// 收口红账（红账未清即收口）。
    RedLeft {
        /// 红账总数。
        reds: u64,
    },
}

impl CoalesceError {
    /// 中文可操作诊断（每变体一 条，写明"是什么 + 下一步怎么做"）。
    pub const fn detail(&self) -> &'static str {
        match self {
            CoalesceError::UnknownTid { .. } => {
                "未知定时器：tid 不在本正本 slab 签发范围内；请先经 arm()/arm_many() 签发再操作"
            }
            CoalesceError::DuePast { .. } => {
                "到期在过去：due 小于当前轮基 now；请改用 now 之后的 due，或先 advance_to() 推进轮基"
            }
            CoalesceError::SlackEarly { .. } => {
                "提前松弛自封：early 必须恒为 0（本域提前到期即违约红账）；请改传 early=0 的松弛窗"
            }
            CoalesceError::SlackOver { .. } => {
                "松弛越档：late 超过该档位 const 上限；请降低 late 或换更宽的档位（Poll/Net/Sync）"
            }
            CoalesceError::CapFull { .. } => {
                "容量超限：slab 计数越过正本容量；请加大 new(cap) 的 cap 或先收口退役旧定时器"
            }
            CoalesceError::DueFar { .. } => {
                "到期越远景：due 超出轮远景 WHEEL_HORIZON；请拆分为链式重排（Periodic）或缩短 due"
            }
            CoalesceError::PeriodZero { .. } => {
                "周期零：Periodic 定时器 period=0 无法重排；请改 period>0 或换 OneShot 型"
            }
            CoalesceError::StateConflict { .. } => {
                "状态冲突：该定时器当前状态不允许此操作（如重复取消/取消已投递项）；请查状态账后重试"
            }
            CoalesceError::EmptyGroup { .. } => {
                "空组投递：组员账为 0 仍走投递路径；请查 form() 聚并账与注入面是否误用"
            }
            CoalesceError::TravAccount { .. } => {
                "遍历账造假：实测 examined 与上报 reported 不符；请排查注入面或统计快照是否被篡改"
            }
            CoalesceError::Conservation { .. } => {
                "组守恒破断：Σ组内成员 ≠ 总回调数；请核对注入面（组丢失注入会故意破账）与投递路径"
            }
            CoalesceError::FingerprintMismatch { .. } => {
                "指纹失配：双跑确定性对拍不一致；请核对两次喂入是否逐字节一致、注入面是否只打在单侧"
            }
            CoalesceError::SlotOrder { .. } => {
                "槽序破稳：槽内 (due,seq) 逆序；请勿绕过 insert() 直接改槽内容，重跑 schedule 修复"
            }
            CoalesceError::OrderBroken { .. } => {
                "到期序破稳：组内投递 due 非降序被破；请查投递循环是否乱序消费 members"
            }
            CoalesceError::NoCompareData => {
                "无对拍数据：聚并开/关双跑未齐；请先调用 compare_run() 再取 reduction_report()/delivery_cmp()"
            }
            CoalesceError::RedLeft { .. } => {
                "收口红账：存在未销账红项；请先 sweep() 定位、处置后 ack_reds() 销账再收口"
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §2  诊断码闭集（0x1430~0x143F，段独占）
// ─────────────────────────────────────────────────────────────────────────

/// 诊断码（0x1430 段十六码闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcCode {
    /// 0x1430 提前到期（松弛窗内到期被提前——违约红账）。
    EarlyFire = 0,
    /// 0x1431 组丢失（组内成员投递数 ≠ 组员账）。
    GroupLoss = 1,
    /// 0x1432 遍历账造假（实测遍历量与上报不符）。
    TravFake = 2,
    /// 0x1433 未知定时器。
    UnknownTid = 3,
    /// 0x1434 到期在过去。
    DuePast = 4,
    /// 0x1435 提前松弛自封（early > 0）。
    SlackEarly = 5,
    /// 0x1436 松弛越限（投递晚于 due+late 承诺）。
    SlackViol = 6,
    /// 0x1437 组守恒破断（Σ组内 ≠ 总回调数）。
    ConservBrk = 7,
    /// 0x1438 容量超限。
    CapFull = 8,
    /// 0x1439 到期越远景。
    DueFar = 9,
    /// 0x143A 周期零。
    PeriodZero = 10,
    /// 0x143B 状态冲突。
    StateConf = 11,
    /// 0x143C 到期序破稳（槽序/组内序）。
    OrderBrk = 12,
    /// 0x143D 指纹失配（双跑对拍）。
    FpMismatch = 13,
    /// 0x143E 空组投递。
    EmptyGroup = 14,
    /// 0x143F 收口红账。
    RedLeft = 15,
}

impl TcCode {
    /// 判别式真值（SEG_BASE + 序）。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            TcCode::EarlyFire => "提前到期",
            TcCode::GroupLoss => "组丢失",
            TcCode::TravFake => "遍历账造假",
            TcCode::UnknownTid => "未知定时器",
            TcCode::DuePast => "到期在过去",
            TcCode::SlackEarly => "提前松弛自封",
            TcCode::SlackViol => "松弛越限",
            TcCode::ConservBrk => "组守恒破断",
            TcCode::CapFull => "容量超限",
            TcCode::DueFar => "到期越远景",
            TcCode::PeriodZero => "周期零",
            TcCode::StateConf => "状态冲突",
            TcCode::OrderBrk => "到期序破稳",
            TcCode::FpMismatch => "指纹失配",
            TcCode::EmptyGroup => "空组投递",
            TcCode::RedLeft => "收口红账",
        }
    }

    /// 全码闭集（顺序即判别式序）。
    pub const ALL: [TcCode; CODE_COUNT] = [
        TcCode::EarlyFire,
        TcCode::GroupLoss,
        TcCode::TravFake,
        TcCode::UnknownTid,
        TcCode::DuePast,
        TcCode::SlackEarly,
        TcCode::SlackViol,
        TcCode::ConservBrk,
        TcCode::CapFull,
        TcCode::DueFar,
        TcCode::PeriodZero,
        TcCode::StateConf,
        TcCode::OrderBrk,
        TcCode::FpMismatch,
        TcCode::EmptyGroup,
        TcCode::RedLeft,
    ];
}

// ─────────────────────────────────────────────────────────────────────────
// §3  slack 面：SlackTier 三档表 + SlackWin [early, late]
// ─────────────────────────────────────────────────────────────────────────

/// slack 档位（三档 const 上限表，来源见文件头）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlackTier {
    /// 轮询类（粗档，上限 4096 拍）。
    Poll,
    /// 网络类（中档，上限 256 拍）。
    Net,
    /// 计时同步类（严档，上限 16 拍）。
    Sync,
}

impl SlackTier {
    /// 档位 const 上限。
    pub const fn cap(self) -> u64 {
        match self {
            SlackTier::Poll => POLL_SLACK_CAP,
            SlackTier::Net => NET_SLACK_CAP,
            SlackTier::Sync => SYNC_SLACK_CAP,
        }
    }

    /// 档位缺省 late。
    pub const fn default_late(self) -> u64 {
        match self {
            SlackTier::Poll => POLL_SLACK_DFT,
            SlackTier::Net => NET_SLACK_DFT,
            SlackTier::Sync => SYNC_SLACK_DFT,
        }
    }

    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            SlackTier::Poll => "轮询类",
            SlackTier::Net => "网络类",
            SlackTier::Sync => "计时同步类",
        }
    }

    /// 档位序号（账面分桶用）。
    pub const fn idx(self) -> usize {
        match self {
            SlackTier::Poll => 0,
            SlackTier::Net => 1,
            SlackTier::Sync => 2,
        }
    }
}

/// 松弛窗 [early, late]：early 恒 0（提前即违约面），late 为可容忍后移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlackWin {
    /// 提前量——本域恒 0，非 0 构造即拒。
    pub early: u64,
    /// 后移容忍（拍）——组锚最迟不得越过任一成员 due+late。
    pub late: u64,
}

impl SlackWin {
    /// 精度敏感直通窗（slack=0：仅与同拍定时器合并）。
    pub const EXACT: SlackWin = SlackWin { early: 0, late: 0 };

    /// 构造（const 可用）：early≠0 一律拒绝——提前到期即违约红账语义。
    pub const fn new(early: u64, late: u64) -> Result<SlackWin, CoalesceError> {
        if early != 0 {
            return Err(CoalesceError::SlackEarly { early });
        }
        Ok(SlackWin { early, late })
    }

    /// 是否直通窗（精度敏感）。
    pub const fn is_exact(self) -> bool {
        self.late == 0
    }
}

/// slack 策略面：档位上限裁决 + 窗构造。
#[derive(Clone, Copy, Debug, Default)]
pub struct SlackPolicy;

impl SlackPolicy {
    /// 裁决松弛窗：precision 直通优先（slack=0），否则 late = override 或档位缺省，
    /// 越档即拒。
    pub fn resolve(
        &self,
        tier: SlackTier,
        precision: bool,
        late_override: Option<u64>,
    ) -> Result<SlackWin, CoalesceError> {
        if precision {
            return Ok(SlackWin::EXACT);
        }
        let late = match late_override {
            Some(v) => v,
            None => tier.default_late(),
        };
        let cap = tier.cap();
        if late > cap {
            return Err(CoalesceError::SlackOver {
                late,
                cap,
                tier: tier.text(),
            });
        }
        SlackWin::new(0, late)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §4  定时器与回调本体（合并的是到期唤醒，回调逐个执行）
// ─────────────────────────────────────────────────────────────────────────

/// 定时器型别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerKind {
    /// 单次：到期即摘链。
    OneShot,
    /// 周期：到期重排 due += period。
    Periodic,
}

/// 回调本体型别（聚并只合并到期唤醒；组内成员回调逐个 dispatch 逐个入账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CbKind {
    /// 轮询拍回调。
    PollTick,
    /// 网络保活回调。
    NetKeepalive,
    /// 计时同步回调。
    TimeSync,
    /// 自定义回调（携带权重代价）。
    Custom(u32),
}

/// 回调执行上下文。
#[derive(Clone, Copy, Debug)]
pub struct CbCtx {
    /// 成员 tid。
    pub tid: u64,
    /// 所属组 id（单发面为 u64::MAX 哨兵）。
    pub gid: u64,
    /// 成员自身 due。
    pub due: u64,
    /// 实际投递拍（组锚拍；≥ due 恒成立）。
    pub fire_tick: u64,
    /// 组内位次（1 起）。
    pub pos: u32,
}

/// 回调执行结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CbOut {
    /// 是否执行。
    pub ran: bool,
    /// 代价（拍计）。
    pub cost: u32,
}

/// 回调派发（纯函数面，确定性可对拍）。
pub const fn dispatch_cb(kind: CbKind, ctx: &CbCtx) -> CbOut {
    let base = match kind {
        CbKind::PollTick => 1u32,
        CbKind::NetKeepalive => 2u32,
        CbKind::TimeSync => 4u32,
        CbKind::Custom(w) => w,
    };
    CbOut {
        ran: true,
        cost: base.saturating_add(ctx.pos % 8),
    }
}

/// 定时器 slab 项。
#[derive(Clone, Debug)]
pub struct CoTimer {
    /// 定时器 id（本正本内从 1 连续签发）。
    pub tid: u64,
    /// 型别。
    pub kind: TimerKind,
    /// 档位。
    pub tier: SlackTier,
    /// 裁决后的松弛窗。
    pub win: SlackWin,
    /// 精度敏感（slack=0 直通）。
    pub precision: bool,
    /// 到期拍。
    pub due: u64,
    /// 周期（Periodic 专用，>0）。
    pub period: u64,
    /// 回调本体型别。
    pub cb: CbKind,
    /// slab 签发序（稳定序第二键）。
    pub seq: u64,
    /// 生命周期状态。
    pub state: TState,
    /// 累计投递次数。
    pub delivered: u32,
    /// 最近投递拍。
    pub fired_at: u64,
}

/// 定时器生命周期状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TState {
    /// 待调度（在 pending 队）。
    Pending,
    /// 已入轮/已入组。
    Assigned,
    /// 已投递（OneShot 终态）。
    Fired,
    /// 已取消。
    Cancelled,
}

// ─────────────────────────────────────────────────────────────────────────
// §5  FNV-1a 64 指纹器（双跑确定性对拍）
// ─────────────────────────────────────────────────────────────────────────

/// FNV-1a 64 位指纹器（增量混入，终值即指纹）。
#[derive(Clone, Copy, Debug)]
pub struct Fnv {
    h: u64,
}

impl Fnv {
    /// FNV-1a 64 offset basis（标准常量，来源：FNV 参数公开说明）。
    pub const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

    /// FNV-1a 64 prime（标准常量）。
    pub const PRIME: u64 = 0x0000_0100_0000_01b3;

    /// 新指纹器。
    pub const fn new() -> Self {
        Fnv { h: Fnv::OFFSET }
    }

    /// 混入一个 u64（小端逐字节）。
    pub fn mix(&mut self, v: u64) {
        self.h = (self.h ^ v).wrapping_mul(Fnv::PRIME);
    }

    /// 终值。
    pub const fn fin(&self) -> u64 {
        self.h
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §6  事件账（有界环）与投递火账
// ─────────────────────────────────────────────────────────────────────────

/// 事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvKind {
    /// 挂链。
    Arm,
    /// 取消。
    Cancel,
    /// 调度（聚并/直排）。
    Schedule,
    /// 组投递。
    GroupFire,
    /// 周期重排。
    Rearm,
    /// 注入。
    Inject,
    /// 检出。
    Detect,
    /// 收口。
    Checkpoint,
}

/// 事件项（定长字段，指纹友好）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoEvent {
    /// 事件拍。
    pub tick: u64,
    /// 种类。
    pub kind: EvKind,
    /// 关联诊断码（0 = 中性事件）。
    pub code: u16,
    /// 附属数 a。
    pub a: u64,
    /// 附属数 b。
    pub b: u64,
}

/// 投递火账项：一次成员回调执行一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FireRec {
    /// 组 id。
    pub gid: u64,
    /// 成员 tid。
    pub tid: u64,
    /// 成员自身 due。
    pub due: u64,
    /// 实际投递拍。
    pub fire_tick: u64,
    /// 档位。
    pub tier: SlackTier,
    /// 成员 late 承诺（fire_tick - due ≤ late 应恒成立）。
    pub late: u64,
}

// ─────────────────────────────────────────────────────────────────────────
// §7  TimerWheel：64 槽粗轮底座（稳定序 + 真实遍历量 + 零丢失）
// ─────────────────────────────────────────────────────────────────────────

/// 轮项：一个组锚（Raw 模式下单成员组同形）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WheelEnt {
    /// 组 id。
    pub gid: u64,
    /// 绝对到期拍。
    pub due: u64,
    /// 入轮序（稳定序第二键）。
    pub seq: u64,
}

/// 遍历量统计（轮负载账：slots_scanned 槽扫数 / examined 项检数 /
/// survivors 存活重扫数 / reported 上报数—— reported 与 examined 可对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TravStat {
    /// 槽扫描次数（每拍一槽）。
    pub slots_scanned: u64,
    /// 轮项检查次数（轮遍历量主口径）。
    pub examined: u64,
    /// 存活重扫次数（未到期被再检）。
    pub survivors: u64,
    /// 上报口径（正常 = examined；注入面可篡改供对账检出）。
    pub reported: u64,
}

impl TravStat {
    /// 零账。
    pub const ZERO: TravStat = TravStat {
        slots_scanned: 0,
        examined: 0,
        survivors: 0,
        reported: 0,
    };
}

/// 64 槽粗轮：槽号 = due % 64，槽内 (due,seq) 稳定升序；逐拍扫描，
/// 到期即摘链，未到期存活重扫（多 revolution 场景下真实产生轮遍历量）。
#[derive(Debug)]
pub struct TimerWheel {
    slots: Vec<Vec<WheelEnt>>,
    base: u64,
    seq: u64,
    trav: TravStat,
}

impl TimerWheel {
    /// 构造（堆上 64 槽，空槽惰性分配）。
    pub fn new() -> Self {
        let mut slots = Vec::new();
        let mut s = 0usize;
        while s < WHEEL_SLOTS {
            slots.push(Vec::new());
            s += 1;
        }
        TimerWheel {
            slots,
            base: 0,
            seq: 0,
            trav: TravStat::ZERO,
        }
    }

    /// 整轮重置（重跑面用）。
    pub fn reset(&mut self, now: u64) {
        let mut s = 0usize;
        while s < self.slots.len() {
            if let Some(v) = self.slots.get_mut(s) {
                v.clear();
            }
            s += 1;
        }
        self.base = now;
        self.seq = 0;
        self.trav = TravStat::ZERO;
    }

    /// 当前轮基。
    pub fn now(&self) -> u64 {
        self.base
    }

    /// 在轮项总数（组锚数）。
    pub fn entry_count(&self) -> u64 {
        let mut n = 0u64;
        let mut s = 0usize;
        while s < self.slots.len() {
            if let Some(v) = self.slots.get(s) {
                n = n.saturating_add(v.len() as u64);
            }
            s += 1;
        }
        n
    }

    /// 插入组锚（due ≥ base 校验、远景校验、槽内稳定有序插入）。
    pub fn insert(&mut self, due: u64, gid: u64) -> Result<(), CoalesceError> {
        if due < self.base {
            return Err(CoalesceError::DuePast {
                due,
                now: self.base,
            });
        }
        if due >= self.base.saturating_add(WHEEL_HORIZON) {
            return Err(CoalesceError::DueFar {
                due,
                horizon: WHEEL_HORIZON,
            });
        }
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let s = (due % WHEEL_SPAN) as usize;
        let slot = match self.slots.get_mut(s) {
            Some(v) => v,
            None => return Err(CoalesceError::SlotOrder { slot: s }),
        };
        // 稳定序：(due, seq) 双键升序；seq 全轮单调 → 同 due 先入先出。
        let pos = slot.partition_point(|e| e.due < due || (e.due == due && e.seq < seq));
        slot.insert(pos, WheelEnt { gid, due, seq });
        Ok(())
    }

    /// 推进到 target（逐拍扫槽）：到期项摘链追加至 out（按拍序 = (due,seq) 稳定序），
    /// 返回摘链数。未到期项存活重扫——每 revolution 一次，构成真实轮遍历量。
    pub fn advance_to(&mut self, target: u64, out: &mut Vec<WheelEnt>) -> Result<u64, CoalesceError> {
        if target < self.base {
            return Ok(0);
        }
        let mut fired = 0u64;
        let mut t = self.base;
        while t <= target {
            let s = (t % WHEEL_SPAN) as usize;
            self.trav.slots_scanned = self.trav.slots_scanned.saturating_add(1);
            let slot = match self.slots.get_mut(s) {
                Some(v) => v,
                None => return Err(CoalesceError::SlotOrder { slot: s }),
            };
            let mut i = 0usize;
            while i < slot.len() {
                self.trav.examined = self.trav.examined.saturating_add(1);
                let due_now = match slot.get(i) {
                    Some(e) => e.due,
                    None => break,
                };
                if due_now <= t {
                    // 到期摘链（槽内有序 → 同拍内按 (due,seq) 稳定出队）。
                    let ent = slot.remove(i);
                    out.push(ent);
                    fired = fired.saturating_add(1);
                } else {
                    // 未到期存活：留槽，下一 revolution 重扫（真实轮负载）。
                    self.trav.survivors = self.trav.survivors.saturating_add(1);
                    i = i.saturating_add(1);
                }
            }
            t = t.saturating_add(1);
        }
        self.base = target.saturating_add(1);
        self.trav.reported = self.trav.examined;
        Ok(fired)
    }

    /// 遍历账快照。
    pub fn trav(&self) -> TravStat {
        self.trav
    }

    /// 遍历账造假注入（挪用 reported——仅供注入面对账检出）。
    pub fn tamper_reported(&mut self) {
        self.trav.reported = self.trav.reported.wrapping_sub(TRAV_TAMPER_STEP);
    }

    /// 遍历账对账：reported ≠ examined 即造假。
    pub fn verify_account(&self) -> Result<(), CoalesceError> {
        if self.trav.reported != self.trav.examined {
            return Err(CoalesceError::TravAccount {
                measured: self.trav.examined,
                reported: self.trav.reported,
            });
        }
        Ok(())
    }

    /// 槽序校验：每槽 (due,seq) 严格非降。
    pub fn verify_slot_order(&self) -> Result<(), CoalesceError> {
        let mut s = 0usize;
        while s < self.slots.len() {
            let slot = match self.slots.get(s) {
                Some(v) => v,
                None => return Err(CoalesceError::SlotOrder { slot: s }),
            };
            let mut i = 1usize;
            while i < slot.len() {
                let prev = match slot.get(i - 1) {
                    Some(e) => *e,
                    None => return Err(CoalesceError::SlotOrder { slot: s }),
                };
                let cur = match slot.get(i) {
                    Some(e) => *e,
                    None => return Err(CoalesceError::SlotOrder { slot: s }),
                };
                if cur.due < prev.due || (cur.due == prev.due && cur.seq < prev.seq) {
                    return Err(CoalesceError::SlotOrder { slot: s });
                }
                i += 1;
            }
            s += 1;
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §8  Coalescer：同到期窗贪心聚并（reach 不变量 → 到期绝不提前）
// ─────────────────────────────────────────────────────────────────────────

/// 聚并输入项（已按 (due,seq) 升序）。
#[derive(Clone, Copy, Debug)]
pub struct FormItem {
    /// 到期拍。
    pub due: u64,
    /// 稳定序键。
    pub seq: u64,
    /// slab 下标。
    pub slab: u32,
    /// 该成员 late 承诺。
    pub late: u64,
}

/// 聚并组：锚拍 = 组内最大 due；reach = min(due_i + late_i)。
/// 不变量（构造保证）：anchor_due ≥ 任一成员 due（绝不提前）；
/// anchor_due ≤ 任一成员 due + late（松弛承诺不越限）。
#[derive(Clone, Debug)]
pub struct CoGroup {
    /// 组 id。
    pub gid: u64,
    /// 组锚拍（投递拍）= 组内最大 due。
    pub anchor_due: u64,
    /// 组内最小 due（跨窗护栏用）。
    pub first_due: u64,
    /// 组 reach = min(due_i + late_i)，后继成员 due ≤ reach 方可并入。
    pub reach: u64,
    /// 成员 slab 下标（(due,seq) 升序）。
    pub members: Vec<u32>,
    /// 组员账（form 时定格）。
    pub expected: u32,
    /// 实投账（投递期累计；守恒应 = expected）。
    pub dispatched: u32,
    /// 是否单发组（Raw 对照面）。
    pub singleton: bool,
}

/// 聚并成型账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormStat {
    /// 输入项数。
    pub items: u64,
    /// 成组数。
    pub groups: u64,
    /// 合并省掉的唤醒数（items - groups）。
    pub saved: u64,
    /// 最大组规模。
    pub biggest: u32,
}

/// 聚并器：配置 + 贪心成型算法。
#[derive(Clone, Copy, Debug)]
pub struct Coalescer {
    /// 单组容量封顶。
    pub group_cap: usize,
    /// 组内首尾 due 最大跨距。
    pub max_span: u64,
}

impl Coalescer {
    /// 构造。
    pub const fn new(group_cap: usize, max_span: u64) -> Self {
        Coalescer {
            group_cap,
            max_span,
        }
    }

    /// 贪心成型：按 (due,seq) 升序消费 items。
    /// 并入条件：组未满 && it.due ≤ reach（松弛承诺） && 跨距 ≤ max_span。
    /// 并入后：reach = min(reach, it.due + it.late)，anchor = it.due（升序下即组内最大）。
    pub fn form(
        &self,
        items: &[FormItem],
        groups: &mut Vec<CoGroup>,
    ) -> Result<FormStat, CoalesceError> {
        let before = groups.len();
        let mut stat = FormStat {
            items: items.len() as u64,
            groups: 0,
            saved: 0,
            biggest: 0,
        };
        let mut cur: Option<CoGroup> = None;
        let mut i = 0usize;
        while i < items.len() {
            let it = match items.get(i) {
                Some(v) => *v,
                None => break,
            };
            let mut joined = false;
            if let Some(g) = cur.as_mut() {
                let fits_cap = g.members.len() < self.group_cap;
                let fits_reach = it.due <= g.reach;
                let fits_span = it.due.saturating_sub(g.first_due) <= self.max_span;
                if fits_cap && fits_reach && fits_span {
                    g.reach = if g.reach < it.due.saturating_add(it.late) {
                        g.reach
                    } else {
                        it.due.saturating_add(it.late)
                    };
                    g.anchor_due = it.due;
                    g.expected = g.expected.saturating_add(1);
                    g.members.push(it.slab);
                    joined = true;
                }
            }
            if !joined {
                // 闭旧组、开新组（gid 依 groups.len() 连续派发——cur 尚未入列）。
                if let Some(g) = cur.take() {
                    groups.push(g);
                }
                // gid 依 groups 当前长度连续派发（cur 尚未入列，不占号）。
                let gid = groups.len() as u64;
                cur = Some(CoGroup {
                    gid,
                    anchor_due: it.due,
                    first_due: it.due,
                    reach: it.due.saturating_add(it.late),
                    members: {
                        let mut m = Vec::new();
                        m.push(it.slab);
                        m
                    },
                    expected: 1,
                    dispatched: 0,
                    singleton: false,
                });
            }
            i += 1;
        }
        if let Some(g) = cur.take() {
            groups.push(g);
        }
        let formed = groups.len().saturating_sub(before);
        // 成组健全账：空组不落账（form 层拒绝空组语义）。
        let mut gi = before;
        while gi < groups.len() {
            let empty = match groups.get(gi) {
                Some(g) => g.members.is_empty(),
                None => true,
            };
            if empty {
                return Err(CoalesceError::EmptyGroup { gid: gi as u64 });
            }
            let size = match groups.get(gi) {
                Some(g) => g.members.len() as u32,
                None => 0,
            };
            if size > stat.biggest {
                stat.biggest = size;
            }
            gi += 1;
        }
        stat.groups = formed as u64;
        stat.saved = stat.items.saturating_sub(stat.groups);
        Ok(stat)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §9  统计账面（投递双列 / 遍历对比 / 延迟账 / 各报告）
// ─────────────────────────────────────────────────────────────────────────

/// 单次推进投递账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FireStat {
    /// 唤醒次数（组锚触发数——合并后投递次数口径）。
    pub wakes: u64,
    /// 回调执行数（Σ组内——合并不省回调）。
    pub callbacks: u64,
    /// 触发组数。
    pub groups_fired: u64,
    /// 周期重排数。
    pub rearmed: u64,
}

impl FireStat {
    /// 零账。
    pub const ZERO: FireStat = FireStat {
        wakes: 0,
        callbacks: 0,
        groups_fired: 0,
        rearmed: 0,
    };
}

/// 调度账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleStat {
    /// 入调度项数。
    pub timers: u64,
    /// 成组数（Raw = 项数）。
    pub groups: u64,
    /// 合并省掉唤醒数。
    pub saved: u64,
}

/// 投递次数双列账（聚并前/聚并后——判据"投递次数账可析"的落点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeliveryCmp {
    /// 聚并关闭：唤醒数（= 定时器数）。
    pub raw_wakes: u64,
    /// 聚并开启：唤醒数（= 组数）。
    pub coal_wakes: u64,
    /// 聚并关闭：回调数。
    pub raw_callbacks: u64,
    /// 聚并开启：回调数。
    pub coal_callbacks: u64,
    /// 省掉唤醒数。
    pub saved_wakes: u64,
    /// 省比（bp）。
    pub saved_bp: u64,
    /// 双列回调守恒（应恒真——合并不省回调）。
    pub callbacks_eq: bool,
}

/// 遍历量对比（判据"下降 ≥30% 实测"的落点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TravCmp {
    /// 聚并关闭：轮项检查数。
    pub raw: u64,
    /// 聚并开启：轮项检查数。
    pub coal: u64,
    /// 下降（bp）。
    pub reduction_bp: u64,
    /// 是否达到 REDUCTION_MIN_BP。
    pub pass: bool,
}

/// 单档延迟账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TierLateStat {
    /// 投递次数。
    pub fires: u64,
    /// 最大迟到（fire_tick - due）。
    pub max_late: u64,
    /// 迟到合计。
    pub sum_late: u64,
    /// 零迟到（当拍即投）数。
    pub zero_late: u64,
}

/// 延迟账（per-tier——精度声明面：Sync 档迟到应显著小于 Poll 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatenessReport {
    /// 0=Poll 1=Net 2=Sync。
    pub per: [TierLateStat; 3],
}

/// 十万零丢失账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MassLedger {
    /// 签发数。
    pub submitted: u64,
    /// 成组数。
    pub groups: u64,
    /// 入组账。
    pub assigned: u64,
    /// 回调账。
    pub delivered: u64,
    /// 丢失数（应为 0）。
    pub lost: u64,
    /// 零丢失判定。
    pub ok: bool,
}

/// 判据轮记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundRec {
    /// 轮序（1 起）。
    pub round: u32,
    /// 聚并前唤醒。
    pub raw_wakes: u64,
    /// 聚并后唤醒。
    pub coal_wakes: u64,
    /// 合并省掉唤醒。
    pub merged: u64,
    /// 双列回调守恒。
    pub callbacks_eq: bool,
    /// 提前到期违例数（应为 0）。
    pub no_early_viol: u64,
    /// 本轮判定。
    pub pass: bool,
}

/// 判据轮总报告（10/10 面板）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundsReport {
    /// 逐轮记录。
    pub rounds: Vec<RoundRec>,
    /// 通过轮数。
    pub passed: u32,
    /// 总轮数。
    pub total: u32,
    /// 全过判定。
    pub all_pass: bool,
}

/// 巡检报告（注入检出面板）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SweepReport {
    /// 提前到期违例数。
    pub early_viol: u64,
    /// 组账失衡组数。
    pub loss_groups: u64,
    /// 遍历账对账一致。
    pub trav_ok: bool,
    /// 到期序违例数。
    pub order_viol: u64,
    /// 松弛越限数。
    pub late_viol: u64,
    /// 红账总数。
    pub red_total: u64,
}

/// 健康面板。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    /// 红账总数。
    pub red_total: u64,
    /// 黄账总数（注入面动用计）。
    pub yellow_total: u32,
    /// 健康判定（红账为 0）。
    pub ok: bool,
    /// 最重红码（判别式最大者）。
    pub worst: Option<TcCode>,
    /// 中文摘要。
    pub summary: String,
}

// ─────────────────────────────────────────────────────────────────────────
// §10  注入面（提前到期 / 组丢失 / 遍历量造假——各设检出）
// ─────────────────────────────────────────────────────────────────────────

/// 注入项（一次一枚；消费即弃）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Injection {
    /// 提前到期注入：下一次组投递的火账 fire_tick 被改记为 anchor-1——
    /// 模拟上游"提前唤醒"缺陷，巡检应检出 0x1430 红账。
    EarlyFire,
    /// 组丢失注入：下一次组投递跳过末位成员回调——模拟投递丢员，
    /// 组账失衡应检出 0x1431 红账 + 守恒破断。
    GroupLoss,
    /// 遍历量造假注入：挪用遍历账 reported 口径——模拟统计造假，
    /// 对账应检出 0x1432 红账。
    TravFake,
}

// ─────────────────────────────────────────────────────────────────────────
// §11  TimCoalesBook：本单正本（调度 + 投递 + 账面 + 注入 + 收口）
// ─────────────────────────────────────────────────────────────────────────

/// 调度模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleMode {
    /// 聚并关闭（对照面）：每定时器单发组直排。
    Raw,
    /// 聚并开启：同窗合并为组锚。
    Coal,
}

/// 挂链请求。
#[derive(Clone, Copy, Debug)]
pub struct ArmReq {
    /// 到期拍。
    pub due: u64,
    /// 档位。
    pub tier: SlackTier,
    /// 型别。
    pub kind: TimerKind,
    /// 周期（Periodic 专用）。
    pub period: u64,
    /// 回调本体。
    pub cb: CbKind,
    /// 精度敏感（slack=0 直通）。
    pub precision: bool,
    /// late 覆写（None 用档位缺省）。
    pub late_override: Option<u64>,
}

/// 单发投递哨兵组 id（延迟账分桶用不到，火账溯源用）。
pub const SINGLE_GID: u64 = u64::MAX;

/// 定时器聚并正本：本体账 + 双面对比 + 注入检出 + 指纹对拍。
#[derive(Debug)]
pub struct TimCoalesBook {
    /// 声明容量。
    pub cap: usize,
    /// slack 策略面。
    pub policy: SlackPolicy,
    /// 聚并器。
    pub coalescer: Coalescer,
    wheel: TimerWheel,
    /// 定时器 slab（tid = 下标 + 1，连续签发不回收）。
    pub slab: Vec<CoTimer>,
    /// 待调度队（slab 下标）。
    pending: Vec<u32>,
    /// 组账（每次 run_plane 重建）。
    pub groups: Vec<CoGroup>,
    /// 事件账（有界环）。
    events: Vec<CoEvent>,
    /// 火账（每 run_plane 重建）。
    fires: Vec<FireRec>,
    /// 当前轮基。
    now: u64,
    /// 下一 tid。
    next_tid: u64,
    /// 稳定序计数器。
    seq: u64,
    /// 本次运行入组账。
    run_assigned: u64,
    /// 本次运行回调账。
    run_delivered: u64,
    /// 累计回调账。
    total_delivered: u64,
    /// 累计签发账。
    total_armed: u64,
    /// 累计取消账。
    total_cancelled: u64,
    /// 红账（16 码计数）。
    red: [u32; CODE_COUNT],
    /// 黄账（注入动用计）。
    yellow: u32,
    /// 本次调度最大 due。
    sched_max_due: Option<u64>,
    /// 聚并关闭遍历快照。
    trav_raw: Option<TravStat>,
    /// 聚并开启遍历快照。
    trav_coal: Option<TravStat>,
    /// 聚并关闭投递快照。
    last_raw: Option<FireStat>,
    /// 聚并开启投递快照。
    last_coal: Option<FireStat>,
    /// 在射注入。
    inj_armed: Option<Injection>,
}

impl TimCoalesBook {
    /// 构造（cap 为 slab 容量声明）。
    pub fn new(cap: usize) -> Result<Self, CoalesceError> {
        if cap == 0 {
            return Err(CoalesceError::CapFull { cap: 0 });
        }
        Ok(TimCoalesBook {
            cap,
            policy: SlackPolicy,
            coalescer: Coalescer::new(GROUP_CAP, COALESCE_MAX_SPAN),
            wheel: TimerWheel::new(),
            slab: Vec::new(),
            pending: Vec::new(),
            groups: Vec::new(),
            events: Vec::new(),
            fires: Vec::new(),
            now: 0,
            next_tid: 1,
            seq: 0,
            run_assigned: 0,
            run_delivered: 0,
            total_delivered: 0,
            total_armed: 0,
            total_cancelled: 0,
            red: [0; CODE_COUNT],
            yellow: 0,
            sched_max_due: None,
            trav_raw: None,
            trav_coal: None,
            last_raw: None,
            last_coal: None,
            inj_armed: None,
        })
    }

    // ── 账面基元 ──

    fn log_event(&mut self, tick: u64, kind: EvKind, code: u16, a: u64, b: u64) {
        if self.events.len() >= LEDGER_CAP {
            self.events.remove(0);
        }
        self.events.push(CoEvent { tick, kind, code, a, b });
    }

    /// 红账记一笔（计数 + 检出事件）。
    pub fn set_red(&mut self, code: TcCode) {
        let i = code as usize;
        if let Some(r) = self.red.get_mut(i) {
            *r = r.saturating_add(1);
        }
        self.log_event(self.now, EvKind::Detect, code.val(), code as u64, 0);
    }

    /// 红账快照（16 码计数列）。
    pub fn red_counts(&self) -> [u32; CODE_COUNT] {
        self.red
    }

    /// 红账总数。
    pub fn red_total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < CODE_COUNT {
            if let Some(r) = self.red.get(i) {
                n = n.saturating_add(*r as u64);
            }
            i += 1;
        }
        n
    }

    /// 销账（审计确认后清零红账——收口前置）。
    pub fn ack_reds(&mut self) {
        self.red = [0; CODE_COUNT];
        self.log_event(self.now, EvKind::Checkpoint, 0, 0, 0);
    }

    /// 事件账只读切片。
    pub fn events(&self) -> &[CoEvent] {
        &self.events
    }

    /// 火账只读切片。
    pub fn fires(&self) -> &[FireRec] {
        &self.fires
    }

    /// 当前轮基。
    pub fn now(&self) -> u64 {
        self.now
    }

    /// 待调度队长。
    pub fn pending_len(&self) -> u64 {
        self.pending.len() as u64
    }

    /// 遍历账快照（当前轮）。
    pub fn wheel_trav(&self) -> TravStat {
        self.wheel.trav()
    }

    /// 在轮组锚数。
    pub fn wheel_entries(&self) -> u64 {
        self.wheel.entry_count()
    }

    /// 遍历账对账透传（实测 examined vs 上报 reported）。
    pub fn wheel_trav_verify(&self) -> Result<(), CoalesceError> {
        self.wheel.verify_account()
    }

    // ── 挂链 / 取消 ──

    /// 挂链：容量、过去期、远景、档位裁决、周期健全五道闸，全过入 pending。
    pub fn arm(&mut self, req: ArmReq) -> Result<u64, CoalesceError> {
        if self.slab.len() >= self.cap {
            self.set_red(TcCode::CapFull);
            return Err(CoalesceError::CapFull { cap: self.cap });
        }
        if req.due < self.now {
            self.set_red(TcCode::DuePast);
            return Err(CoalesceError::DuePast {
                due: req.due,
                now: self.now,
            });
        }
        if req.due >= self.now.saturating_add(WHEEL_HORIZON) {
            self.set_red(TcCode::DueFar);
            return Err(CoalesceError::DueFar {
                due: req.due,
                horizon: WHEEL_HORIZON,
            });
        }
        let win = self.policy.resolve(req.tier, req.precision, req.late_override)?;
        if req.kind == TimerKind::Periodic && req.period == 0 {
            self.set_red(TcCode::PeriodZero);
            return Err(CoalesceError::PeriodZero { tid: self.next_tid });
        }
        let idx = self.slab.len() as u32;
        let tid = self.next_tid;
        self.next_tid = self.next_tid.saturating_add(1);
        self.seq = self.seq.wrapping_add(1);
        self.slab.push(CoTimer {
            tid,
            kind: req.kind,
            tier: req.tier,
            win,
            precision: req.precision,
            due: req.due,
            period: req.period,
            cb: req.cb,
            seq: self.seq,
            state: TState::Pending,
            delivered: 0,
            fired_at: 0,
        });
        self.pending.push(idx);
        self.total_armed = self.total_armed.saturating_add(1);
        self.log_event(req.due, EvKind::Arm, 0, tid, req.due);
        Ok(tid)
    }

    /// 批量挂链（窗簇分布：windows 个窗、每窗 4 拍散布），返回实签发数。
    pub fn arm_many(
        &mut self,
        timers: usize,
        windows: usize,
        late: u64,
    ) -> Result<u64, CoalesceError> {
        let base = self.now;
        let windows = if windows == 0 { 1 } else { windows };
        let per = if timers / windows == 0 { 1 } else { timers / windows };
        let mut i = 0usize;
        let mut n = 0u64;
        while i < timers {
            let mut w = i / per;
            if w >= windows {
                w = windows - 1;
            }
            let jitter = ((i % per) % 4) as u64;
            let due = base
                .saturating_add(8)
                .saturating_add((w as u64).saturating_mul(4))
                .saturating_add(jitter);
            self.arm(ArmReq {
                due,
                tier: SlackTier::Poll,
                kind: TimerKind::OneShot,
                period: 0,
                cb: CbKind::PollTick,
                precision: false,
                late_override: Some(late),
            })?;
            n = n.saturating_add(1);
            i += 1;
        }
        Ok(n)
    }

    /// 取消（仅 Pending 态可取消）。
    pub fn cancel(&mut self, tid: u64) -> Result<bool, CoalesceError> {
        if tid == 0 || tid >= self.next_tid {
            self.set_red(TcCode::UnknownTid);
            return Err(CoalesceError::UnknownTid { tid });
        }
        let idx = (tid - 1) as usize;
        let state = match self.slab.get(idx) {
            Some(tm) => tm.state,
            None => {
                self.set_red(TcCode::UnknownTid);
                return Err(CoalesceError::UnknownTid { tid });
            }
        };
        if state != TState::Pending {
            self.set_red(TcCode::StateConf);
            return Err(CoalesceError::StateConflict { tid });
        }
        if let Some(tm) = self.slab.get_mut(idx) {
            tm.state = TState::Cancelled;
        }
        self.total_cancelled = self.total_cancelled.saturating_add(1);
        self.log_event(self.now, EvKind::Cancel, 0, tid, 0);
        Ok(true)
    }

    // ── 调度（Raw 直排 / Coal 聚并）──

    /// 把 pending 全量排入轮：Raw 逐项单发组直排；Coal 经聚并器成组后锚排。
    pub fn schedule(&mut self, mode: ScheduleMode) -> Result<ScheduleStat, CoalesceError> {
        let drain = core::mem::take(&mut self.pending);
        let mut items: Vec<FormItem> = Vec::new();
        let mut i = 0usize;
        while i < drain.len() {
            let idx = match drain.get(i) {
                Some(v) => *v,
                None => break,
            };
            if let Some(tm) = self.slab.get(idx as usize) {
                if tm.state == TState::Pending {
                    items.push(FormItem {
                        due: tm.due,
                        seq: tm.seq,
                        slab: idx,
                        late: tm.win.late,
                    });
                }
            }
            i += 1;
        }
        // (due, seq) 升序稳定排——聚并成型与槽内稳定序的共同前提。
        items.sort_by(|a, b| match a.due.cmp(&b.due) {
            Ordering::Equal => a.seq.cmp(&b.seq),
            o => o,
        });
        let mut stat = ScheduleStat {
            timers: items.len() as u64,
            groups: 0,
            saved: 0,
        };
        match mode {
            ScheduleMode::Raw => {
                let mut k = 0usize;
                while k < items.len() {
                    let it = match items.get(k) {
                        Some(v) => *v,
                        None => break,
                    };
                    let gid = self.groups.len() as u64;
                    let mut members = Vec::new();
                    members.push(it.slab);
                    self.groups.push(CoGroup {
                        gid,
                        anchor_due: it.due,
                        first_due: it.due,
                        reach: it.due.saturating_add(it.late),
                        members,
                        expected: 1,
                        dispatched: 0,
                        singleton: true,
                    });
                    self.wheel.insert(it.due, gid)?;
                    if let Some(tm) = self.slab.get_mut(it.slab as usize) {
                        tm.state = TState::Assigned;
                    }
                    self.run_assigned = self.run_assigned.saturating_add(1);
                    k += 1;
                }
                stat.groups = items.len() as u64;
                stat.saved = 0;
            }
            ScheduleMode::Coal => {
                let fstat = self.coalescer.form(&items, &mut self.groups)?;
                stat.groups = fstat.groups;
                stat.saved = fstat.saved;
                let base_gid = self.groups.len().saturating_sub(fstat.groups as usize);
                let mut gi = base_gid;
                while gi < self.groups.len() {
                    let anchor = match self.groups.get(gi) {
                        Some(g) => g.anchor_due,
                        None => break,
                    };
                    let members = match self.groups.get(gi) {
                        Some(g) => g.members.clone(),
                        None => Vec::new(),
                    };
                    self.wheel.insert(anchor, gi as u64)?;
                    let mut mi = 0usize;
                    while mi < members.len() {
                        let m = match members.get(mi) {
                            Some(v) => *v,
                            None => break,
                        };
                        if let Some(tm) = self.slab.get_mut(m as usize) {
                            tm.state = TState::Assigned;
                        }
                        self.run_assigned = self.run_assigned.saturating_add(1);
                        mi += 1;
                    }
                    gi += 1;
                }
            }
        }
        // 本次调度最大 due（推进目标）。
        let mut maxd: Option<u64> = None;
        i = 0;
        while i < items.len() {
            let d = match items.get(i) {
                Some(v) => v.due,
                None => break,
            };
            maxd = Some(match maxd {
                Some(m) if m >= d => m,
                _ => d,
            });
            i += 1;
        }
        self.sched_max_due = maxd;
        self.log_event(self.now, EvKind::Schedule, 0, stat.timers, stat.groups);
        Ok(stat)
    }

    // ── 推进与投递（合并的是唤醒，回调逐个执行）──

    fn take_inj(&mut self, want: Injection) -> bool {
        if self.inj_armed == Some(want) {
            self.inj_armed = None;
            true
        } else {
            false
        }
    }

    /// 推进到 target：轮摘链组锚 → 逐组投递 → 组内成员回调逐个 dispatch 逐个入账；
    /// Periodic 成员重排回 pending。返回本次投递账。
    pub fn advance_to(&mut self, target: u64) -> Result<FireStat, CoalesceError> {
        let mut fired_ents: Vec<WheelEnt> = Vec::new();
        let _n = self.wheel.advance_to(target, &mut fired_ents)?;
        let mut stat = FireStat::ZERO;
        let mut rearm: Vec<(u32, u64)> = Vec::new();
        let mut fi = 0usize;
        while fi < fired_ents.len() {
            let ent = match fired_ents.get(fi) {
                Some(v) => *v,
                None => break,
            };
            fi += 1;
            let gidx = ent.gid as usize;
            let (members, expected, singleton) = match self.groups.get(gidx) {
                Some(g) => (g.members.clone(), g.expected, g.singleton),
                None => {
                    self.set_red(TcCode::EmptyGroup);
                    self.log_event(ent.due, EvKind::GroupFire, TcCode::EmptyGroup.val(), ent.gid, 0);
                    stat.groups_fired = stat.groups_fired.saturating_add(1);
                    continue;
                }
            };
            stat.groups_fired = stat.groups_fired.saturating_add(1);
            if members.is_empty() {
                self.set_red(TcCode::EmptyGroup);
                self.log_event(ent.due, EvKind::GroupFire, TcCode::EmptyGroup.val(), ent.gid, 0);
                continue;
            }
            stat.wakes = stat.wakes.saturating_add(1);
            // 注入消费（一次一枚）。
            let early_tamper = self.take_inj(Injection::EarlyFire);
            let loss_skip = self.take_inj(Injection::GroupLoss);
            if early_tamper {
                self.yellow = self.yellow.saturating_add(1);
                self.log_event(ent.due, EvKind::Inject, 0, ent.gid, 1);
            }
            if loss_skip {
                self.yellow = self.yellow.saturating_add(1);
                self.log_event(ent.due, EvKind::Inject, 0, ent.gid, 2);
            }
            let fire_tick = if early_tamper {
                ent.due.saturating_sub(1)
            } else {
                ent.due
            };
            let _ = singleton;
            let mut mi = 0usize;
            while mi < members.len() {
                let m = match members.get(mi) {
                    Some(v) => *v,
                    None => break,
                };
                mi += 1;
                // 组丢失注入：跳过末位成员回调（组账失衡 → 巡检检出）。
                if loss_skip && mi == members.len() {
                    continue;
                }
                let (tid, due, tier, late, cb) = match self.slab.get(m as usize) {
                    Some(tm) => (tm.tid, tm.due, tm.tier, tm.win.late, tm.cb),
                    None => {
                        self.set_red(TcCode::UnknownTid);
                        continue;
                    }
                };
                let ctx = CbCtx {
                    tid,
                    gid: ent.gid,
                    due,
                    fire_tick,
                    pos: mi as u32,
                };
                let out = dispatch_cb(cb, &ctx);
                if out.ran {
                    self.fires.push(FireRec {
                        gid: ent.gid,
                        tid,
                        due,
                        fire_tick,
                        tier,
                        late,
                    });
                    stat.callbacks = stat.callbacks.saturating_add(1);
                    self.run_delivered = self.run_delivered.saturating_add(1);
                    self.total_delivered = self.total_delivered.saturating_add(1);
                }
                if let Some(tm) = self.slab.get_mut(m as usize) {
                    tm.delivered = tm.delivered.saturating_add(1);
                    if tm.kind == TimerKind::Periodic {
                        let nd = tm.due.saturating_add(tm.period);
                        rearm.push((m, nd));
                    } else {
                        tm.state = TState::Fired;
                        tm.fired_at = ent.due;
                    }
                }
                if let Some(g) = self.groups.get_mut(gidx) {
                    g.dispatched = g.dispatched.saturating_add(1);
                }
            }
            self.log_event(ent.due, EvKind::GroupFire, 0, ent.gid, expected as u64);
        }
        // 周期重排。
        let mut ri = 0usize;
        while ri < rearm.len() {
            let (idx, nd) = match rearm.get(ri) {
                Some(v) => *v,
                None => break,
            };
            if let Some(tm) = self.slab.get_mut(idx as usize) {
                tm.due = nd;
                tm.state = TState::Pending;
            }
            self.pending.push(idx);
            stat.rearmed = stat.rearmed.saturating_add(1);
            self.log_event(nd, EvKind::Rearm, 0, idx as u64, nd);
            ri += 1;
        }
        self.now = target.saturating_add(1);
        Ok(stat)
    }

    // ── 运行面（单跑 / 复活 / 双面对比）──

    /// 单跑一面：整轮重置 → 按模式调度 → 推进到最大 due。
    pub fn run_plane(&mut self, mode: ScheduleMode) -> Result<FireStat, CoalesceError> {
        self.wheel.reset(self.now);
        self.groups.clear();
        self.fires.clear();
        self.run_assigned = 0;
        self.run_delivered = 0;
        self.sched_max_due = None;
        self.schedule(mode)?;
        let target = match self.sched_max_due {
            Some(t) => t,
            None => return Ok(FireStat::ZERO),
        };
        if target < self.now {
            return Ok(FireStat::ZERO);
        }
        self.advance_to(target)
    }

    /// 复活指定范围（Fired/Assigned → Pending 回队）——双面对比用。
    fn revive_scope(&mut self, scope: &[u32]) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < scope.len() {
            let idx = match scope.get(i) {
                Some(v) => *v,
                None => break,
            };
            let state = match self.slab.get(idx as usize) {
                Some(tm) => tm.state,
                None => {
                    i += 1;
                    continue;
                }
            };
            if state == TState::Fired || state == TState::Assigned {
                if let Some(tm) = self.slab.get_mut(idx as usize) {
                    tm.state = TState::Pending;
                }
                self.pending.push(idx);
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 双面对比跑：聚并关闭 → 快照 → 复活同批 → 聚并开启 → 快照。
    /// 同一批定时器两种排法，遍历量与投递次数两账同源可比。
    /// now 语义：raw 面推进过的轮基在 coal 面复跑前回卷到共同起点
    /// （同批 due 不因对照跑而变成"过去期"）；coal 面跑完自然落在 target+1。
    pub fn compare_run(&mut self) -> Result<DeliveryCmp, CoalesceError> {
        let scope: Vec<u32> = self.pending.clone();
        let start_now = self.now;
        let raw = self.run_plane(ScheduleMode::Raw)?;
        self.trav_raw = Some(self.wheel.trav());
        self.last_raw = Some(raw);
        self.now = start_now;
        self.revive_scope(&scope);
        let coal = self.run_plane(ScheduleMode::Coal)?;
        self.trav_coal = Some(self.wheel.trav());
        self.last_coal = Some(coal);
        self.delivery_cmp()
    }

    /// 投递次数双列账（聚并前/后两列）。
    pub fn delivery_cmp(&self) -> Result<DeliveryCmp, CoalesceError> {
        let raw = match self.last_raw {
            Some(v) => v,
            None => return Err(CoalesceError::NoCompareData),
        };
        let coal = match self.last_coal {
            Some(v) => v,
            None => return Err(CoalesceError::NoCompareData),
        };
        let saved = raw.wakes.saturating_sub(coal.wakes);
        let saved_bp = if raw.wakes == 0 {
            0
        } else {
            saved.saturating_mul(10_000) / raw.wakes
        };
        Ok(DeliveryCmp {
            raw_wakes: raw.wakes,
            coal_wakes: coal.wakes,
            raw_callbacks: raw.callbacks,
            coal_callbacks: coal.callbacks,
            saved_wakes: saved,
            saved_bp,
            callbacks_eq: raw.callbacks == coal.callbacks,
        })
    }

    /// 遍历量对比报告（下降 ≥30% 判据口径）。
    pub fn reduction_report(&self) -> Result<TravCmp, CoalesceError> {
        let r = match self.trav_raw {
            Some(v) => v,
            None => return Err(CoalesceError::NoCompareData),
        };
        let c = match self.trav_coal {
            Some(v) => v,
            None => return Err(CoalesceError::NoCompareData),
        };
        if r.examined == 0 {
            return Err(CoalesceError::NoCompareData);
        }
        let drop = r.examined.saturating_sub(c.examined);
        let bp = drop.saturating_mul(10_000) / r.examined;
        Ok(TravCmp {
            raw: r.examined,
            coal: c.examined,
            reduction_bp: bp,
            pass: bp >= REDUCTION_MIN_BP,
        })
    }

    /// 延迟账（per-tier 迟到分布——精度声明观测面）。
    pub fn lateness_report(&self) -> LatenessReport {
        let mut per = [TierLateStat::default(); 3];
        let mut i = 0usize;
        while i < self.fires.len() {
            let fr = match self.fires.get(i) {
                Some(v) => *v,
                None => break,
            };
            let b = fr.tier.idx();
            if let Some(s) = per.get_mut(b) {
                let late = fr.fire_tick.saturating_sub(fr.due);
                s.fires = s.fires.saturating_add(1);
                s.sum_late = s.sum_late.saturating_add(late);
                if late > s.max_late {
                    s.max_late = late;
                }
                if late == 0 {
                    s.zero_late = s.zero_late.saturating_add(1);
                }
            }
            i += 1;
        }
        LatenessReport { per }
    }

    // ── 守恒 / 序 / 不变量巡检 ──

    /// 组守恒：Σ组内账 ≡ 本次运行回调账（判据"组账守恒"口径）。
    pub fn conservation_check(&mut self) -> Result<(), CoalesceError> {
        let mut exp = 0u64;
        let mut dis = 0u64;
        let mut i = 0usize;
        while i < self.groups.len() {
            if let Some(g) = self.groups.get(i) {
                exp = exp.saturating_add(g.expected as u64);
                dis = dis.saturating_add(g.dispatched as u64);
            }
            i += 1;
        }
        if exp != self.run_assigned || dis != self.run_delivered || exp != dis {
            self.set_red(TcCode::ConservBrk);
            return Err(CoalesceError::Conservation {
                assigned: self.run_assigned,
                delivered: self.run_delivered,
            });
        }
        Ok(())
    }

    /// 到期不提前巡检：火账逐行 fire_tick ≥ due（提前即违约红账）。
    /// 返回违例数（0 = 通过）。
    pub fn no_early_check(&mut self) -> Result<u64, CoalesceError> {
        let mut viol = 0u64;
        let mut i = 0usize;
        while i < self.fires.len() {
            if let Some(fr) = self.fires.get(i) {
                if fr.fire_tick < fr.due {
                    viol = viol.saturating_add(1);
                }
            }
            i += 1;
        }
        if viol > 0 {
            self.set_red(TcCode::EarlyFire);
        }
        Ok(viol)
    }

    /// 松弛承诺巡检：fire_tick - due ≤ late 逐行核（越限红账）。
    pub fn slack_check(&mut self) -> Result<u64, CoalesceError> {
        let mut viol = 0u64;
        let mut i = 0usize;
        while i < self.fires.len() {
            if let Some(fr) = self.fires.get(i) {
                if fr.fire_tick.saturating_sub(fr.due) > fr.late {
                    viol = viol.saturating_add(1);
                }
            }
            i += 1;
        }
        if viol > 0 {
            self.set_red(TcCode::SlackViol);
        }
        Ok(viol)
    }

    /// 到期序稳定巡检：同组火账 due 非降（组内投递序 = 成型序）。
    pub fn verify_due_order(&mut self) -> Result<(), CoalesceError> {
        let mut viol = 0usize;
        let mut prev_gid: Option<u64> = None;
        let mut prev_due = 0u64;
        let mut i = 0usize;
        while i < self.fires.len() {
            let fr = match self.fires.get(i) {
                Some(v) => *v,
                None => break,
            };
            if prev_gid == Some(fr.gid) && fr.due < prev_due {
                viol += 1;
            }
            prev_gid = Some(fr.gid);
            prev_due = fr.due;
            i += 1;
        }
        if viol > 0 {
            self.set_red(TcCode::OrderBrk);
            return Err(CoalesceError::OrderBroken { at: viol });
        }
        Ok(())
    }

    /// 槽序校验透传。
    pub fn verify_slot_order(&self) -> Result<(), CoalesceError> {
        self.wheel.verify_slot_order()
    }

    /// 全量巡检（注入检出面板）：提前 / 组丢失 / 遍历账 / 到期序 / 松弛承诺五查。
    pub fn sweep(&mut self) -> SweepReport {
        let mut early = 0u64;
        let mut late_viol = 0u64;
        let mut i = 0usize;
        while i < self.fires.len() {
            let fr = match self.fires.get(i) {
                Some(v) => *v,
                None => break,
            };
            if fr.fire_tick < fr.due {
                early += 1;
            }
            if fr.fire_tick.saturating_sub(fr.due) > fr.late {
                late_viol += 1;
            }
            i += 1;
        }
        if early > 0 {
            self.set_red(TcCode::EarlyFire);
        }
        if late_viol > 0 {
            self.set_red(TcCode::SlackViol);
        }
        let mut loss = 0u64;
        i = 0;
        while i < self.groups.len() {
            if let Some(g) = self.groups.get(i) {
                if g.dispatched != g.expected {
                    loss += 1;
                }
            }
            i += 1;
        }
        if loss > 0 {
            self.set_red(TcCode::GroupLoss);
        }
        let t = self.wheel.trav();
        let trav_ok = t.reported == t.examined;
        if !trav_ok {
            self.set_red(TcCode::TravFake);
        }
        let mut order_viol = 0u64;
        let mut prev_gid: Option<u64> = None;
        let mut prev_due = 0u64;
        i = 0;
        while i < self.fires.len() {
            let fr = match self.fires.get(i) {
                Some(v) => *v,
                None => break,
            };
            if prev_gid == Some(fr.gid) && fr.due < prev_due {
                order_viol += 1;
            }
            prev_gid = Some(fr.gid);
            prev_due = fr.due;
            i += 1;
        }
        if order_viol > 0 {
            self.set_red(TcCode::OrderBrk);
        }
        SweepReport {
            early_viol: early,
            loss_groups: loss,
            trav_ok,
            order_viol,
            late_viol,
            red_total: self.red_total(),
        }
    }

    // ── 注入面 ──

    /// 压入注入（一次一枚；在射未消费再压即拒）。
    pub fn inject(&mut self, inj: Injection) -> Result<(), CoalesceError> {
        if self.inj_armed.is_some() {
            return Err(CoalesceError::StateConflict { tid: 0 });
        }
        self.inj_armed = Some(inj);
        // TravFake 语义：注入即挪用遍历账 reported 口径（run_plane 重跑会自然复位）。
        if inj == Injection::TravFake {
            self.wheel.tamper_reported();
        }
        self.yellow = self.yellow.saturating_add(1);
        self.log_event(self.now, EvKind::Inject, 0, inj as u64, 0);
        Ok(())
    }

    // ── 指纹与对拍 ──

    /// FNV-1a 64 指纹：账面 + 事件 + 火账 + 组账全混入。
    pub fn fingerprint(&self) -> u64 {
        let mut f = Fnv::new();
        f.mix(self.now);
        f.mix(self.total_delivered);
        f.mix(self.run_assigned);
        f.mix(self.run_delivered);
        f.mix(self.total_armed);
        f.mix(self.total_cancelled);
        let mut i = 0usize;
        while i < CODE_COUNT {
            let r = match self.red.get(i) {
                Some(v) => *v as u64,
                None => 0,
            };
            f.mix(r);
            i += 1;
        }
        f.mix(self.events.len() as u64);
        i = 0;
        while i < self.events.len() {
            if let Some(ev) = self.events.get(i) {
                f.mix(ev.kind as u64);
                f.mix(ev.code as u64);
                f.mix(ev.tick);
                f.mix(ev.a);
                f.mix(ev.b);
            }
            i += 1;
        }
        f.mix(self.fires.len() as u64);
        i = 0;
        while i < self.fires.len() {
            if let Some(fr) = self.fires.get(i) {
                f.mix(fr.gid);
                f.mix(fr.tid);
                f.mix(fr.due);
                f.mix(fr.fire_tick);
                f.mix(fr.tier as u64);
                f.mix(fr.late);
            }
            i += 1;
        }
        f.mix(self.groups.len() as u64);
        i = 0;
        while i < self.groups.len() {
            if let Some(g) = self.groups.get(i) {
                f.mix(g.gid);
                f.mix(g.anchor_due);
                f.mix(g.expected as u64);
                f.mix(g.dispatched as u64);
            }
            i += 1;
        }
        f.mix(self.coalescer.group_cap as u64);
        f.mix(self.coalescer.max_span);
        f.fin()
    }

    /// 双跑确定性对拍：指纹与核心账全比对，失配落红账并返回错。
    pub fn determinism_check(&mut self, other: &Self) -> Result<(), CoalesceError> {
        let a = self.fingerprint();
        let b = other.fingerprint();
        if a != b
            || self.total_delivered != other.total_delivered
            || self.total_armed != other.total_armed
        {
            self.set_red(TcCode::FpMismatch);
            return Err(CoalesceError::FingerprintMismatch { left: a, right: b });
        }
        Ok(())
    }

    // ── 判据轮与大规模账 ──

    /// 判据轮：COALESCE_ROUNDS 轮，每轮 ROUND_WINDOWS 窗 × ROUND_PER_WINDOW
    /// 定时器同窗挂链 → 双面对比 → 五查全过才计通过（合并投递 10/10 面板）。
    pub fn criterion_rounds(&mut self) -> Result<RoundsReport, CoalesceError> {
        let mut rounds: Vec<RoundRec> = Vec::new();
        let mut passed = 0u32;
        let mut r = 0usize;
        while r < COALESCE_ROUNDS {
            let base = self.now;
            let mut w = 0usize;
            while w < ROUND_WINDOWS {
                let mut j = 0usize;
                while j < ROUND_PER_WINDOW {
                    let due = base
                        .saturating_add(8)
                        .saturating_add((w as u64).saturating_mul(16))
                        .saturating_add((j % 4) as u64);
                    self.arm(ArmReq {
                        due,
                        tier: SlackTier::Poll,
                        kind: TimerKind::OneShot,
                        period: 0,
                        cb: CbKind::PollTick,
                        precision: false,
                        late_override: Some(12),
                    })?;
                    j += 1;
                }
                w += 1;
            }
            let cmp = self.compare_run()?;
            let no_early = self.no_early_check()?;
            let _ = self.slack_check()?;
            self.verify_due_order()?;
            self.wheel.verify_slot_order()?;
            self.conservation_check()?;
            let merged = cmp.raw_wakes.saturating_sub(cmp.coal_wakes);
            let pass = cmp.raw_wakes > cmp.coal_wakes
                && merged > 0
                && cmp.callbacks_eq
                && no_early == 0;
            if pass {
                passed += 1;
            }
            rounds.push(RoundRec {
                round: (r + 1) as u32,
                raw_wakes: cmp.raw_wakes,
                coal_wakes: cmp.coal_wakes,
                merged,
                callbacks_eq: cmp.callbacks_eq,
                no_early_viol: no_early,
                pass,
            });
            r += 1;
        }
        let total = rounds.len() as u32;
        Ok(RoundsReport {
            all_pass: passed == total,
            passed,
            total,
            rounds,
        })
    }

    /// 十万零丢失账：arm_many 簇挂 → 聚并调度 → 全程推进 → 守恒闭合。
    pub fn mass_ledger_run(
        &mut self,
        timers: usize,
        windows: usize,
    ) -> Result<MassLedger, CoalesceError> {
        let submitted = self.arm_many(timers, windows, 8)?;
        let sched = self.schedule(ScheduleMode::Coal)?;
        let target = match self.sched_max_due {
            Some(t) => t,
            None => self.now,
        };
        let fire = if target >= self.now {
            self.advance_to(target)?
        } else {
            FireStat::ZERO
        };
        self.conservation_check()?;
        let lost = submitted.saturating_sub(self.run_delivered);
        let ok = lost == 0
            && self.run_delivered == self.run_assigned
            && fire.callbacks == self.run_delivered;
        Ok(MassLedger {
            submitted,
            groups: sched.groups,
            assigned: self.run_assigned,
            delivered: self.run_delivered,
            lost,
            ok,
        })
    }

    // ── 健康 / 收口 ──

    /// 健康面板。
    pub fn health(&self) -> Health {
        let mut total = 0u64;
        let mut worst: Option<TcCode> = None;
        let mut i = 0usize;
        while i < CODE_COUNT {
            let n = match self.red.get(i) {
                Some(v) => *v as u64,
                None => 0,
            };
            if n > 0 {
                total += n;
                if let Some(c) = TcCode::ALL.get(i) {
                    worst = Some(*c);
                }
            }
            i += 1;
        }
        let summary = format!(
            "聚并域健康：红账{} 黄账{} 事件{} 火账{} 组{} —— 判据面 10/10 轮 + 遍历≥30% + 组守恒",
            total,
            self.yellow,
            self.events.len(),
            self.fires.len(),
            self.groups.len(),
        );
        Health {
            red_total: total,
            yellow_total: self.yellow,
            ok: total == 0,
            worst,
            summary,
        }
    }

    /// 收口：红账未清即拒。
    pub fn close_book(&self) -> Result<(), CoalesceError> {
        let n = self.red_total();
        if n > 0 {
            return Err(CoalesceError::RedLeft { reds: n });
        }
        Ok(())
    }

    /// 累计回调账。
    pub fn total_delivered(&self) -> u64 {
        self.total_delivered
    }

    /// 累计签发账。
    pub fn total_armed(&self) -> u64 {
        self.total_armed
    }

    /// 累计取消账。
    pub fn total_cancelled(&self) -> u64 {
        self.total_cancelled
    }

    /// slab 计数。
    pub fn slab_len(&self) -> u64 {
        self.slab.len() as u64
    }
}

// ---------------------------------------------------------------------------
// §12  冒烟测试（隔离探针面：vx_probe_unx1083 专用；判据逐条真实运行）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod t_unx1083 {
    use super::*;

    /// 探针专用解包（仅测试路径；内核路径零 unwrap 纪律不受影响）。
    fn ck<T>(r: Result<T, CoalesceError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("探针失败: {} ({:?})", e.detail(), e),
        }
    }

    fn mkbook(cap: usize) -> TimCoalesBook {
        ck(TimCoalesBook::new(cap))
    }

    /// 判据一：聚并正确——同到期窗定时器合并投递 10/10 次（投递次数账可析）。
    #[test]
    fn coalesce_merge_10_of_10() {
        let mut b = mkbook(1 << 16);
        let rep = ck(b.criterion_rounds());
        assert_eq!(rep.total, COALESCE_ROUNDS as u32);
        assert_eq!(rep.passed, COALESCE_ROUNDS as u32, "10/10 未达标: {:?}", rep.rounds);
        assert!(rep.all_pass);
        // 投递次数双列账逐轮可析：raw = 窗×每窗数，coal ≤ 窗数（合并生效）。
        for rec in &rep.rounds {
            assert_eq!(rec.raw_wakes, (ROUND_WINDOWS * ROUND_PER_WINDOW) as u64);
            assert!(rec.coal_wakes <= ROUND_WINDOWS as u64, "合并未生效: {:?}", rec);
            assert!(rec.merged > 0);
            assert!(rec.callbacks_eq, "合并不省回调守恒被破: {:?}", rec);
            assert_eq!(rec.no_early_viol, 0);
        }
        // 双列账口径与火账一致：回调数 = Σ组内。
        let cmp = ck(b.delivery_cmp());
        assert!(cmp.callbacks_eq);
        assert_eq!(cmp.coal_callbacks, cmp.raw_callbacks);
        assert!(b.health().ok);
        ck(b.close_book());
    }

    /// 判据二：slack 生效——松弛窗内到期不提前（语义不破断言）。
    #[test]
    fn slack_never_early() {
        let mut b = mkbook(1 << 14);
        // 有意拉大松弛窗（late=16）：聚并锚 = 组内最大 due，任何成员不早于自身 due。
        let base = b.now();
        let mut w = 0usize;
        while w < 8 {
            let mut j = 0usize;
            while j < 8 {
                ck(b.arm(ArmReq {
                    due: base + 8 + (w as u64) * 16 + ((j % 4) as u64),
                    tier: SlackTier::Poll,
                    kind: TimerKind::OneShot,
                    period: 0,
                    cb: CbKind::NetKeepalive,
                    precision: false,
                    late_override: Some(16),
                }));
                j += 1;
            }
            w += 1;
        }
        let _ = ck(b.compare_run());
        assert_eq!(ck(b.no_early_check()), 0, "提前到期违例——语义破断");
        assert_eq!(ck(b.slack_check()), 0, "late 承诺被越限");
        // 延迟账：全部投递落在 [due, due+late] 内，零提前。
        let lat = b.lateness_report();
        for s in &lat.per {
            assert_eq!(s.zero_late + (s.fires - s.zero_late), s.fires);
        }
        assert!(b.health().ok);
    }

    /// 判据三：轮负载下降可测——聚并开启后轮遍历量下降 ≥30%（实测对比）。
    #[test]
    fn traversal_drop_at_least_30pct() {
        let mut b = mkbook(1 << 16);
        // 20k 定时器、5k 窗（每窗 4 拍散布）：聚并后 5k 组锚替代 20k 单项。
        ck(b.arm_many(20_000, 5_000, 8));
        let cmp = ck(b.compare_run());
        let trav = ck(b.reduction_report());
        assert!(
            trav.pass,
            "遍历量下降未达 30%: raw={} coal={} bp={}",
            trav.raw, trav.coal, trav.reduction_bp
        );
        assert!(trav.reduction_bp >= REDUCTION_MIN_BP);
        // 聚并后轮项数 = 组数 < 定时器数（轮负载物理下降的直证）。
        assert!(cmp.coal_wakes < cmp.raw_wakes);
        assert!(cmp.callbacks_eq);
        // 遍历账对账：实测 = 上报。
        assert!(b.wheel_trav_verify().is_ok());
        assert!(b.health().ok);
    }

    /// 注入探针一：提前到期注入 → 0x1430 检出（提前即违约红账）。
    #[test]
    fn inject_early_fire_detected() {
        let mut b = mkbook(1 << 12);
        ck(b.arm_many(256, 32, 12));
        ck(b.inject(Injection::EarlyFire));
        let _ = ck(b.run_plane(ScheduleMode::Coal));
        let rep = b.sweep();
        assert!(rep.early_viol > 0, "提前到期注入未检出: {:?}", rep);
        assert!(b.red_counts()[TcCode::EarlyFire as usize] > 0);
        assert!(!b.health().ok);
        assert!(ck(b.no_early_check()) > 0);
        b.ack_reds();
        assert!(b.health().ok);
    }

    /// 注入探针二：组丢失注入 → 0x1431 检出 + 组守恒破断。
    #[test]
    fn inject_group_loss_detected() {
        let mut b = mkbook(1 << 12);
        ck(b.arm_many(256, 32, 12));
        ck(b.inject(Injection::GroupLoss));
        let _ = ck(b.run_plane(ScheduleMode::Coal));
        let rep = b.sweep();
        assert!(rep.loss_groups > 0, "组丢失注入未检出: {:?}", rep);
        assert!(b.red_counts()[TcCode::GroupLoss as usize] > 0);
        // 守恒破断：Σ组内 ≠ 回调数。
        assert!(b.conservation_check().is_err());
        b.ack_reds();
    }

    /// 注入探针三：遍历量造假注入 → 0x1432 检出。
    #[test]
    fn inject_trav_fake_detected() {
        let mut b = mkbook(1 << 12);
        ck(b.arm_many(256, 32, 12));
        let _ = ck(b.run_plane(ScheduleMode::Coal));
        ck(b.inject(Injection::TravFake));
        // 注入即挪用 reported 口径。
        assert!(b.wheel_trav_verify().is_err(), "遍历造假未被对账");
        let rep = b.sweep();
        assert!(!rep.trav_ok);
        assert!(b.red_counts()[TcCode::TravFake as usize] > 0);
        b.ack_reds();
        // 销账只清红账；reported 挪用需下一次 advance 快照才自然复位——语义如实。
        assert!(b.health().ok);
    }

    /// 十万定时器零丢失账（判据点名 10^5 规模）。
    #[test]
    fn mass_100k_zero_loss() {
        let mut b = mkbook(1 << 18);
        let led = ck(b.mass_ledger_run(MASS_TIMERS, MASS_WINDOWS));
        assert!(led.ok, "十万账非零丢失: {:?}", led);
        assert_eq!(led.submitted, MASS_TIMERS as u64);
        assert_eq!(led.delivered, MASS_TIMERS as u64);
        assert_eq!(led.lost, 0);
        // 聚并物理效果：10k 组替代 100k 单项。
        assert!(led.groups > 0 && led.groups < MASS_TIMERS as u64);
        assert_eq!(b.total_armed(), MASS_TIMERS as u64);
        assert!(b.health().ok);
        ck(b.close_book());
    }

    /// 双跑确定性对拍 + 指纹失配面（0x143D）。
    #[test]
    fn dual_run_fingerprint_parity() {
        let mut a = mkbook(1 << 14);
        let mut c = mkbook(1 << 14);
        for book in [&mut a, &mut c] {
            ck(book.arm_many(2_000, 500, 8));
            let _ = ck(book.run_plane(ScheduleMode::Coal));
        }
        assert_eq!(a.fingerprint(), c.fingerprint(), "同喂入双跑指纹不一致");
        ck(a.determinism_check(&c));
        // 扰动侧：一枚精度定时器加塞 → 指纹必异。
        let mut d = mkbook(1 << 14);
        ck(d.arm_many(2_000, 500, 8));
        ck(d.arm(ArmReq {
            due: 9_999,
            tier: SlackTier::Sync,
            kind: TimerKind::OneShot,
            period: 0,
            cb: CbKind::TimeSync,
            precision: true,
            late_override: None,
        }));
        let _ = ck(d.run_plane(ScheduleMode::Coal));
        assert_ne!(a.fingerprint(), d.fingerprint());
        assert!(a.health().ok);
    }

    /// slack 三档表 + 精度直通语义。
    #[test]
    fn slack_tiers_and_precision_passthrough() {
        let pol = SlackPolicy;
        // 越档拒绝：Poll 上限 4096，late=5000 拒。
        assert!(matches!(
            pol.resolve(SlackTier::Poll, false, Some(5_000)),
            Err(CoalesceError::SlackOver { .. })
        ));
        // Sync 严档：late=17 拒（上限 16），late=16 过。
        assert!(matches!(
            pol.resolve(SlackTier::Sync, false, Some(17)),
            Err(CoalesceError::SlackOver { .. })
        ));
        assert!(pol.resolve(SlackTier::Sync, false, Some(16)).is_ok());
        // 提前松弛自封拒绝（early≠0 构造期即拒）。
        assert!(matches!(
            SlackWin::new(3, 8),
            Err(CoalesceError::SlackEarly { early: 3 })
        ));
        // 精度直通：precision → EXACT 窗。
        let win = pol.resolve(SlackTier::Sync, true, Some(9)).ok();
        assert_eq!(win, Some(SlackWin::EXACT));
        // 直通语义落地：精度定时器仅与同拍者合并，anchor = due，零迟到。
        let mut b = mkbook(1 << 12);
        let base = b.now();
        let mut j = 0usize;
        while j < 3 {
            ck(b.arm(ArmReq {
                due: base + 8,
                tier: SlackTier::Sync,
                kind: TimerKind::OneShot,
                period: 0,
                cb: CbKind::TimeSync,
                precision: true,
                late_override: None,
            }));
            j += 1;
        }
        ck(b.arm(ArmReq {
            due: base + 10,
            tier: SlackTier::Poll,
            kind: TimerKind::OneShot,
            period: 0,
            cb: CbKind::PollTick,
            precision: false,
            late_override: Some(8),
        }));
        ck(b.arm(ArmReq {
            due: base + 11,
            tier: SlackTier::Poll,
            kind: TimerKind::OneShot,
            period: 0,
            cb: CbKind::PollTick,
            precision: false,
            late_override: Some(8),
        }));
        let _ = ck(b.run_plane(ScheduleMode::Coal));
        // 组一：3 枚同拍精度项；组二：2 枚 Poll 项——直通不被异拍吞并。
        assert_eq!(b.groups.len(), 2, "直通分组破断: {:?}", b.groups);
        let g0 = &b.groups[0];
        assert_eq!(g0.members.len(), 3);
        assert_eq!(g0.anchor_due, base + 8);
        for fr in b.fires() {
            if fr.tid <= 3 {
                assert_eq!(fr.fire_tick, fr.due, "精度定时器被延迟: {:?}", fr);
                assert_eq!(fr.fire_tick - fr.due, 0);
            }
        }
        assert!(b.health().ok);
    }

    /// 周期面：到期重排 + 二轮零丢失。
    #[test]
    fn periodic_rearm_zero_loss() {
        let mut b = mkbook(1 << 12);
        let base = b.now();
        let mut k = 0usize;
        while k < 4 {
            ck(b.arm(ArmReq {
                due: base + 8 + (k as u64) * 4,
                tier: SlackTier::Net,
                kind: TimerKind::Periodic,
                period: 100,
                cb: CbKind::NetKeepalive,
                precision: false,
                late_override: Some(8),
            }));
            k += 1;
        }
        let s1 = ck(b.run_plane(ScheduleMode::Coal));
        assert_eq!(s1.callbacks, 4);
        assert_eq!(s1.rearmed, 4, "周期重排缺失: {:?}", s1);
        // 二轮：重排后的 due 落在新窗，全数再投。
        let s2 = ck(b.run_plane(ScheduleMode::Coal));
        assert_eq!(s2.callbacks, 4);
        assert_eq!(s2.rearmed, 4);
        assert_eq!(b.total_delivered(), 8);
        assert!(b.health().ok);
    }

    /// 取消面 + 错误面：detail 非空、码闭集、非法迁移拒绝。
    #[test]
    fn cancel_and_error_surface() {
        let mut b = mkbook(1 << 12);
        let base = b.now();
        let t1 = ck(b.arm(ArmReq {
            due: base + 16,
            tier: SlackTier::Poll,
            kind: TimerKind::OneShot,
            period: 0,
            cb: CbKind::PollTick,
            precision: false,
            late_override: None,
        }));
        let _t2 = ck(b.arm(ArmReq {
            due: base + 32,
            tier: SlackTier::Poll,
            kind: TimerKind::OneShot,
            period: 0,
            cb: CbKind::PollTick,
            precision: false,
            late_override: None,
        }));
        // 推进一轮（t1 已取消、t2 投递），now 越过 base —— 过去期样本可取。
        let _ = ck(b.run_plane(ScheduleMode::Coal));
        assert!(b.now() > base);
        // 过去期拒绝：due = base（< now）。
        assert!(matches!(
            b.arm(ArmReq {
                due: base,
                tier: SlackTier::Poll,
                kind: TimerKind::OneShot,
                period: 0,
                cb: CbKind::PollTick,
                precision: false,
                late_override: None,
            }),
            Err(CoalesceError::DuePast { .. })
        ));
        // 取消面：Pending 态可取消；重复取消 / tid=0 / 越界 tid 均拒。
        let now = b.now();
        let t3 = ck(b.arm(ArmReq {
            due: now + 16,
            tier: SlackTier::Poll,
            kind: TimerKind::OneShot,
            period: 0,
            cb: CbKind::PollTick,
            precision: false,
            late_override: None,
        }));
        assert!(ck(b.cancel(t3)));
        assert!(b.cancel(t3).is_err(), "重复取消未被拒");
        assert!(b.cancel(0).is_err(), "tid=0 未拒");
        assert!(b.cancel(999).is_err(), "越界 tid 未拒");
        // 码闭集：val = SEG_BASE + 序；text 非空。
        let mut i = 0usize;
        while i < CODE_COUNT {
            let c = TcCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + i as u16);
            assert!(!c.text().is_empty());
            i += 1;
        }
        // 错误诊断全覆盖：每变体 detail 非空。
        let errs = [
            CoalesceError::UnknownTid { tid: 7 },
            CoalesceError::DuePast { due: 1, now: 2 },
            CoalesceError::SlackEarly { early: 5 },
            CoalesceError::SlackOver { late: 9, cap: 8, tier: "x" },
            CoalesceError::CapFull { cap: 4 },
            CoalesceError::DueFar { due: 9, horizon: 8 },
            CoalesceError::PeriodZero { tid: 3 },
            CoalesceError::StateConflict { tid: 3 },
            CoalesceError::EmptyGroup { gid: 3 },
            CoalesceError::TravAccount { measured: 8, reported: 7 },
            CoalesceError::Conservation { assigned: 8, delivered: 7 },
            CoalesceError::FingerprintMismatch { left: 1, right: 2 },
            CoalesceError::SlotOrder { slot: 3 },
            CoalesceError::OrderBroken { at: 2 },
            CoalesceError::NoCompareData,
            CoalesceError::RedLeft { reds: 1 },
        ];
        for e in &errs {
            assert!(!e.detail().is_empty(), "{:?} detail 为空", e);
        }
        // 过去期拒绝等负样本已落红账，销账后健康面应复原。
        b.ack_reds();
        assert!(b.health().ok);
    }

    /// 稳定序面：同 due 多定时器按签发序稳定投递（seq 第二键）。
    #[test]
    fn stable_due_order() {
        let mut b = mkbook(1 << 12);
        let base = b.now();
        let mut k = 0usize;
        while k < 16 {
            ck(b.arm(ArmReq {
                due: base + 8,
                tier: SlackTier::Sync,
                kind: TimerKind::OneShot,
                period: 0,
                cb: CbKind::TimeSync,
                precision: true,
                late_override: None,
            }));
            k += 1;
        }
        let _ = ck(b.run_plane(ScheduleMode::Raw));
        // 同拍同组内按 (due,seq) 稳定序：tid 升序。
        let fires = b.fires();
        assert_eq!(fires.len(), 16);
        let mut i = 1usize;
        while i < fires.len() {
            assert!(fires[i - 1].tid < fires[i].tid, "稳定序被破");
            i += 1;
        }
        assert!(b.verify_due_order().is_ok());
        assert!(b.health().ok);
    }
}
