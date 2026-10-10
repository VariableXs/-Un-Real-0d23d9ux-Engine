//! CGPU-F1925 · 显示模式管理（CGPU-M 域 · 显示输出 · 批次 M01 · 单 05）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1925`
//!
//! 模式管理：模式管理（模式列表/当前模式/切换——模式 API（模式枚举复用
//! F1923 投影——投影复用；切换（模式切换（F1941 深化预告——预告；验证
//! （模式验证（黑屏防护——防护红线复用家族——黑屏红线；测试（API/投影/
//! 预告/防护四组）。判据：模式 API、投影复用、黑屏红线、预告、四组、判据。
//!
//! # 要点一：模式 API 三件——列表/当前/切换
//!
//! [`ModeManager`] 持有候选投影集（列表）、当前模式、以及单向切换序列
//! 状态机；三件各有其位：列表只读自投影、当前只经 commit 改写、切换只走
//! [`SwitchState`] 单向序列（Idle→Validated→Committed→ack→Idle）。
//!
//! # 要点二：投影复用 F1923——不重建第二套能力过滤
//!
//! [`PROJECTION_REUSE`]：候选集 = `cgm03_edid::project(caps, want)` 纯裁剪
//! 输出——本单零能力判断代码，表外拒绝（[`codes::MODE_NOT_PROJECTED`]）
//! 的依据全部来自投影集；投影源以 caps 指纹挂钩（[`codes::PROJECTION_STALE`]：
//! 指纹漂移即失效重读，复用 cgm03 缓存语义）。
//!
//! # 要点三：黑屏防护是独立于投影的第二道闸（防护红线复用家族）
//!
//! 投影集只保证「模式在能力范围内」，不保证「模式可用不黑屏」：低于
//! [`SAFE_REFRESH_FLOOR_MILLIHZ`] 的模式即使能被投影出来也拒绝
//! （[`codes::BLACKOUT_RISK`]）——黑屏红线：防护是显性码拒绝，不是静默
//! 降级，更不是先切了再看。
//!
//! # 要点四：F1941 深化预告——本单只做冷切换
//!
//! [`F1941_PREVIEW`]：本单切换为冷切换（校验→提交→确认，单向序列）；
//! F1941 模式热切换在同一验证面之上加无缝切换——预告归档，深化点为
//! 切换序列复用；热切换语义本单不实现（表上面就没有热切换入口）。
//!
//! # 要点五：诊断码延续 0x54xx 域段
//!
//! cgm01 占 0x5401~0x5407；cgm02 占 0x5408~0x540C；cgm03 占
//! 0x540D~0x5412；cgm04 占 0x5413~0x5418；本单占 0x5419~0x541E；
//! 判据防自判死断言互异。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界：变长访问先长度对账，失败路径
//! 走 `Result` 与显性 VmCode。

use crate::cgpu::cgm01_display::VmCode;
use crate::cgpu::cgm03_edid::{project, EdidCaps, Mode};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（cgm05 续占 0x5419..0x541E）
// ---------------------------------------------------------------------------

/// M 域诊断码（cgm05 段）。
pub mod codes {
    use super::VmCode;

    /// 投影候选表为空（无任何可列模式）。
    pub const MODE_TABLE_EMPTY: VmCode = VmCode(0x5419);

    /// 请求模式不在投影集内（表外拒绝——投影复用面的唯一裁决依据）。
    pub const MODE_NOT_PROJECTED: VmCode = VmCode(0x541A);

    /// 黑屏风险（低于安全刷新下限——防护红线独立闸）。
    pub const BLACKOUT_RISK: VmCode = VmCode(0x541B);

    /// 切换序列违约（状态机非法迁移——含热切换未开放面）。
    pub const SEQUENCE_BROKEN: VmCode = VmCode(0x541C);

    /// 投影源过期（caps 指纹漂移——失效重读，复用 cgm03 缓存语义）。
    pub const PROJECTION_STALE: VmCode = VmCode(0x541D);

    /// 待确认提交违约（Committed 态之外请求确认）。
    pub const ACK_INVALID: VmCode = VmCode(0x541E);
}

// ---------------------------------------------------------------------------
// 二、复用声明与常量
// ---------------------------------------------------------------------------

/// 安全刷新下限（毫赫兹）：低于此值的模式即使可投影也拒——黑屏防护闸。
/// 24 000 mHz = 24 Hz；判据侧独立常量对拍。
pub const SAFE_REFRESH_FLOOR_MILLIHZ: u32 = 24_000u32;

/// 切换历史账封顶（滚动淘汰最旧；供 F1930 显示遥测复用——遥测只读不写）。
pub const SWITCH_LOG_CAP: usize = 16;

/// 投影复用声明（F1923——本单零能力判断代码）。
pub const PROJECTION_REUSE: &str =
    "模式枚举复用 F1923 投影：候选集 = cgm03_edid::project(caps, want) 纯裁剪输出，本单不重建第二套能力过滤";

/// 遥测复用声明（F1930 显示遥测——切换历史账遥测面只读不写）。
pub const TELEMETRY_REUSE: &str =
    "切换历史账供 F1930 显示遥测复用：遥测面只读 history()，不产生第二套切换账——账本唯一";

/// F1941 深化预告（本单只做冷切换；热切换语义本单不实现）。
pub const F1941_PREVIEW: &str =
    "模式热切换深化预告：本单只做冷切换（校验→提交→确认，序列单向）；F1941 模式热切换在同一验证面之上加无缝切换——预告归档，深化点为切换序列复用";

// ---------------------------------------------------------------------------
// 三、切换序列状态机（闭集三态，单向）
// ---------------------------------------------------------------------------

/// 切换历史记录（一笔冷切换 = 一条：单调序号 + 从/到模式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchRecord {
    /// 记录序号（自 1 起单调）。
    pub seq: u64,
    /// 切换前模式。
    pub from: Mode,
    /// 切换后模式。
    pub to: Mode,
}

/// 切换序列状态闭集：空闲 / 已校验 / 已提交。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitchState {
    /// 空闲（可接新请求）。
    Idle,
    /// 已校验（等待提交）。
    Validated,
    /// 已提交（等待确认）。
    Committed,
}

// ---------------------------------------------------------------------------
// 四、模式管理器
// ---------------------------------------------------------------------------

/// 显示模式管理器：列表/当前/切换三件 API。
#[derive(Debug, Clone)]
pub struct ModeManager {
    caps_fp: u64,
    want: Vec<Mode>,
    projected: Vec<Mode>,
    current: Mode,
    pending: Option<Mode>,
    state: SwitchState,
    log: Vec<SwitchRecord>,
    log_seq: u64,
}

impl ModeManager {
    /// 建管：投影复用 F1923——候选集为 `project(caps, want)` 纯裁剪输出；
    /// 空表 [`codes::MODE_TABLE_EMPTY`]；当前模式取投影集中刷新率最高者
    /// （确定性：并列取先出现者）。
    pub fn new(caps: &EdidCaps, caps_fp: u64, want: &[Mode]) -> Result<ModeManager, VmCode> {
        let projected = project(caps, want);
        if projected.is_empty() {
            return Err(codes::MODE_TABLE_EMPTY);
        }
        let mut best = 0usize;
        let mut i = 1usize;
        while i < projected.len() {
            if projected[i].refresh_millihz > projected[best].refresh_millihz {
                best = i;
            }
            i += 1usize;
        }
        let current = projected[best];
        Ok(ModeManager {
            caps_fp,
            want: want.to_vec(),
            projected,
            current,
            pending: None,
            state: SwitchState::Idle,
            log: Vec::new(),
            log_seq: 0u64,
        })
    }

    /// 投影源指纹漂移检测：不匹配即重投影（失效重读，绝不静默用旧表）。
    pub fn revalidate(&mut self, caps: &EdidCaps, caps_fp: u64) -> Result<(), VmCode> {
        if self.caps_fp == caps_fp {
            return Ok(());
        }
        let projected = project(caps, &self.want);
        if projected.is_empty() {
            return Err(codes::MODE_TABLE_EMPTY);
        }
        self.caps_fp = caps_fp;
        self.projected = projected;
        Ok(())
    }

    /// 模式列表（只读投影集）。
    pub fn list(&self) -> &[Mode] {
        &self.projected
    }

    /// 当前模式。
    pub fn current(&self) -> Mode {
        self.current
    }

    /// 切换序列状态。
    pub fn state(&self) -> SwitchState {
        self.state
    }

    /// 切换历史只读视图（封顶滚动，最旧先出；遥测面只读）。
    pub fn history(&self) -> &[SwitchRecord] {
        &self.log
    }

    /// 待提交模式（仅 Validated 态有值）。
    pub fn pending(&self) -> Option<Mode> {
        self.pending
    }

    /// 请求切换（冷切换第一步：Idle→Validated）。
    ///
    /// 三闸依序：序列闸（非 Idle 即 [`codes::SEQUENCE_BROKEN`]）→
    /// 投影闸（表外即 [`codes::MODE_NOT_PROJECTED`]）→ 黑屏闸
    /// （低于 [`SAFE_REFRESH_FLOOR_MILLIHZ`] 即 [`codes::BLACKOUT_RISK`]）。
    pub fn request_switch(&mut self, want: Mode) -> Result<(), VmCode> {
        if self.state != SwitchState::Idle {
            return Err(codes::SEQUENCE_BROKEN);
        }
        let mut in_table = false;
        let mut i = 0usize;
        while i < self.projected.len() {
            let m = self.projected[i];
            if m.h == want.h && m.v == want.v && m.refresh_millihz == want.refresh_millihz {
                in_table = true;
            }
            i += 1usize;
        }
        if !in_table {
            return Err(codes::MODE_NOT_PROJECTED);
        }
        if want.refresh_millihz < SAFE_REFRESH_FLOOR_MILLIHZ {
            return Err(codes::BLACKOUT_RISK);
        }
        self.pending = Some(want);
        self.state = SwitchState::Validated;
        Ok(())
    }

    /// 提交切换（第二步：Validated→Committed，current 只在此改写）。
    pub fn commit_switch(&mut self) -> Result<Mode, VmCode> {
        if self.state != SwitchState::Validated {
            return Err(codes::SEQUENCE_BROKEN);
        }
        let m = match self.pending {
            Some(m) => m,
            None => return Err(codes::SEQUENCE_BROKEN),
        };
        self.log_seq = match self.log_seq.checked_add(1u64) {
            Some(v) => v,
            None => return Err(codes::SEQUENCE_BROKEN),
        };
        let rec = SwitchRecord {
            seq: self.log_seq,
            from: self.current,
            to: m,
        };
        if self.log.len() >= SWITCH_LOG_CAP && !self.log.is_empty() {
            self.log.remove(0usize);
        }
        self.log.push(rec);
        self.current = m;
        self.state = SwitchState::Committed;
        Ok(m)
    }

    /// 确认提交（第三步：Committed→Idle，序列闭合）。
    pub fn ack_commit(&mut self) -> Result<(), VmCode> {
        if self.state != SwitchState::Committed {
            return Err(codes::ACK_INVALID);
        }
        self.pending = None;
        self.state = SwitchState::Idle;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、读屏摘要（手写字节装配，零 panic 零分配格式化）
// ---------------------------------------------------------------------------

impl ModeManager {
    /// 当前模式 + 序列状态读屏摘要。
    ///
    /// 布局：`M<w>x<v>@<refresh_millihz>|S<state 0/1/2>|L<n>`；输出写入
    /// `out` 并返回写入长度，缓冲不足则截断（与 [`crate::checks`]
    /// render 同纪律）。
    pub fn render_summary(&self, out: &mut [u8]) -> usize {
        let mut n = 0usize;
        n += push_byte(out, n, b'M');
        n += push_u16_dec(out, n, self.current.h);
        n += push_byte(out, n, b'x');
        n += push_u16_dec(out, n, self.current.v);
        n += push_byte(out, n, b'@');
        n += push_u32_dec(out, n, self.current.refresh_millihz);
        n += push_byte(out, n, b'|');
        n += push_byte(out, n, b'S');
        let st = match self.state {
            SwitchState::Idle => b'0',
            SwitchState::Validated => b'1',
            SwitchState::Committed => b'2',
        };
        n += push_byte(out, n, st);
        n += push_byte(out, n, b'|');
        n += push_byte(out, n, b'L');
        n += push_u16_dec(out, n, self.projected.len() as u16);
        if n > out.len() {
            out.len()
        } else {
            n
        }
    }
}

/// 单字节写入（缓冲不足丢弃，返回计划长度——调用方最后统一截断）。
fn push_byte(out: &mut [u8], n: usize, b: u8) -> usize {
    if n < out.len() {
        out[n] = b;
    }
    1usize
}

/// u16 十进制写入（0 无特例；除法前不查零——除数恒为 10）。
fn push_u16_dec(out: &mut [u8], n: usize, mut v: u16) -> usize {
    let mut tmp = [0u8; 5usize];
    let mut k = 5usize;
    if v == 0u16 {
        k -= 1usize;
        tmp[k] = b'0';
    }
    while v > 0u16 {
        k -= 1usize;
        tmp[k] = b'0' + (v % 10u16) as u8;
        v /= 10u16;
    }
    let mut w = 0usize;
    while k < 5usize {
        w += push_byte(out, n + w, tmp[k]);
        k += 1usize;
    }
    w
}

/// u32 十进制写入（同 u16 纪律）。
fn push_u32_dec(out: &mut [u8], n: usize, mut v: u32) -> usize {
    let mut tmp = [0u8; 10usize];
    let mut k = 10usize;
    if v == 0u32 {
        k -= 1usize;
        tmp[k] = b'0';
    }
    while v > 0u32 {
        k -= 1usize;
        tmp[k] = b'0' + (v % 10u32) as u8;
        v /= 10u32;
    }
    let mut w = 0usize;
    while k < 10usize {
        w += push_byte(out, n + w, tmp[k]);
        k += 1usize;
    }
    w
}
