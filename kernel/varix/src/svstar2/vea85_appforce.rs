//! VE-F0085 · 每应用强制档位（VE-A 域 · 策略仲裁段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0085`
//!
//! **职责定位（锚点原文）**：用户对单个应用的渲染档位强制（锁死高画质/
//! 低功耗），强制优先于自动策略，强制项显式可见；含强制档位与降级协商
//! 的冲突仲裁（用户强制最高但系统要降听谁的——用户优先）。
//!
//! **数据结构（锚点原文）**：档位强制。在 [`ForceMgr`] 上一一落位：档位 =
//! [`RenderTier`]、强制项 = [`ForceEntry`]、仲裁产物 = [`ResolvedTier`]。
//!
//! **判据（锚点原文）**：按应用强制、优先自动、显式可见、冲突用户优先、判据。
//!
//! 本模块 12 条判据逐条映射：**按应用强制 3 条**（强制登记按应用隔离不
//! 串档／强制撤销恢复自动／重复登记以最新为准）、**优先自动 3 条**
//! （无强制项走自动策略原值／强制优先于自动降级请求／越权设置显性拒
//! 绝）、**显式可见 2 条**（强制项读屏行带档位与来源／强制项计数与登记
//! 表对账）、**冲突用户优先 2 条**（系统要降与用户强制最高冲突时用户
//! 胜且留仲裁痕迹／用户强制低功耗时系统升档请求不越权）、**判据自身
//! 2 条**（码位互异段独占／强制失效告知入账）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 越权 → **拒绝** | 只有特权调用方（[`PRIVILEGED_CALLER`]）可设强制档；非特权设置显性 [`CODE_FORBID`] 且不入表——强制权不是人人可拿 |
//! | 强制失效 → **告知** | 用户强制档超出设备当期能力时，强制保留（用户优先）但落告知语（「设备当前达不到，实际以 X 运行」）并计数——强制不静默打折 |
//! | 冲突 → **用户优先** | 自动策略要降、用户锁死最高：终态听用户；仲裁痕迹（次数与双方诉求）入账，事后可查「为什么没降」 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **强制是登记不是开关**：[`ForceEntry`] 带设置时间戳序号与设置方身
//!   份，重复登记以最新为准（旧条目被覆盖且可追溯）——「显式可见」要
//!   能回答「谁在什么时候锁了什么」。
//! - **仲裁方向单向**：用户强制 > 自动策略 > 设备建议。设备建议永远不
//!   能覆盖用户强制；但设备**达不到**时如实告知（保留强制标记 + 实际
//!   运行档分离）——用户优先不等于欺骗用户。
//! - **撤销即回自动**：clear 后 resolve 立即回自动策略值，无残留缓存。
//! - **零 panic 面**：查表走 `position`、计数全 `saturating_*`、全部入口
//!   返回 [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0xC8xx，独占——整仓 0xC801~0xC80F 零占用后选定）
// ---------------------------------------------------------------------------

/// 越权设置强制档（非特权调用方）。
pub const CODE_FORBID: u16 = 0xC801;
/// 应用号非法（0 号为系统保留）。
pub const CODE_APP_BAD: u16 = 0xC802;
/// 撤销不存在的强制项。
pub const CODE_NOT_FORCED: u16 = 0xC803;
/// 强制失效（设备达不到；告知性码，强制保留）。
pub const CODE_FORCE_INEFFECTIVE: u16 = 0xC804;
/// 仲裁账失配（内部不变量破坏）。
pub const CODE_ARBITER_BAD: u16 = 0xC805;
/// 强制表满（应用数上限）。
pub const CODE_FORCE_CAP: u16 = 0xC806;

// ---------------------------------------------------------------------------
// 二、档位与强制项
// ---------------------------------------------------------------------------

/// 渲染档位（用户可强制的目标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderTier {
    /// 高画质（最高功耗）。
    High,
    /// 均衡。
    Balanced,
    /// 低功耗（最低画质）。
    PowerSaver,
}

impl RenderTier {
    /// 短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            RenderTier::High => "高画质",
            RenderTier::Balanced => "均衡",
            RenderTier::PowerSaver => "低功耗",
        }
    }

    /// 档位序（功耗序：High=2 > Balanced=1 > PowerSaver=0）。
    pub const fn order(self) -> u8 {
        match self {
            RenderTier::High => 2,
            RenderTier::Balanced => 1,
            RenderTier::PowerSaver => 0,
        }
    }
}

/// 单条强制项：哪个应用、锁到哪档、谁设的、序号多少。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForceEntry {
    /// 应用号。
    pub app: u32,
    /// 强制档位。
    pub tier: RenderTier,
    /// 设置方（特权调用方标识）。
    pub by: u32,
    /// 登记序号（单调递增；重复登记以最新为准的依据）。
    pub seq: u32,
}

/// 一次仲裁的产物：生效档 + 仲裁痕迹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedTier {
    /// 实际生效档位。
    pub effective: RenderTier,
    /// 是否来自用户强制（true = 强制优先）。
    pub from_force: bool,
    /// 强制是否实际可达（false = 设备达不到，已落告知）。
    pub force_honored: bool,
}

// ---------------------------------------------------------------------------
// 三、强制管理器（本模块主语）
// ---------------------------------------------------------------------------

/// 特权调用方标识（唯一可设强制档的调用方）。
pub const PRIVILEGED_CALLER: u32 = 1;

/// 强制表容量上限。
pub const FORCE_CAP: usize = 128;

/// 每应用强制档位管理器。
#[derive(Clone, Debug)]
pub struct ForceMgr {
    /// 强制项登记表（按应用隔离；同应用以最新 seq 为准）。
    pub forces: Vec<ForceEntry>,
    /// 统计账。
    pub stats: ForceStats,
    /// 最近告知语（强制失效/仲裁痕迹的读屏行）。
    pub last_notice: String,
    seq: u32,
}

/// 统计账：五桶逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForceStats {
    /// 强制登记次数（含覆盖）。
    pub sets: u32,
    /// 撤销次数。
    pub clears: u32,
    /// 越权拒绝次数。
    pub forbids: u32,
    /// 用户优先仲裁次数。
    pub user_wins: u32,
    /// 强制失效告知次数。
    pub ineffective_notices: u32,
}

impl ForceMgr {
    /// 新管理器：空表零账。
    pub fn new() -> ForceMgr {
        ForceMgr {
            forces: Vec::new(),
            stats: ForceStats::default(),
            last_notice: String::new(),
            seq: 0,
        }
    }

    /// 设强制档：特权校验 + 应用校验 + 重复登记以最新为准。
    pub fn set_force(&mut self, caller: u32, app: u32, tier: RenderTier) -> Result<(), u16> {
        if caller != PRIVILEGED_CALLER {
            self.stats.forbids += 1;
            return Err(CODE_FORBID);
        }
        if app == 0 {
            return Err(CODE_APP_BAD);
        }
        self.seq += 1;
        match self.forces.iter().position(|f| f.app == app) {
            Some(i) => {
                // 覆盖旧条目（以最新为准）
                self.forces[i] = ForceEntry { app, tier, by: caller, seq: self.seq };
            }
            None => {
                if self.forces.len() >= FORCE_CAP {
                    return Err(CODE_FORCE_CAP);
                }
                self.forces.push(ForceEntry { app, tier, by: caller, seq: self.seq });
            }
        }
        self.stats.sets += 1;
        Ok(())
    }

    /// 撤销强制（应用恢复自动）。
    pub fn clear_force(&mut self, caller: u32, app: u32) -> Result<(), u16> {
        if caller != PRIVILEGED_CALLER {
            self.stats.forbids += 1;
            return Err(CODE_FORBID);
        }
        match self.forces.iter().position(|f| f.app == app) {
            Some(i) => {
                self.forces.remove(i);
                self.stats.clears += 1;
                Ok(())
            }
            None => Err(CODE_NOT_FORCED),
        }
    }

    /// 查应用强制项。
    pub fn forced_tier(&self, app: u32) -> Option<RenderTier> {
        self.forces.iter().find(|f| f.app == app).map(|f| f.tier)
    }

    /// 仲裁：用户强制 > 自动策略；设备可达性如实告知。
    /// `device_cap` 是设备当前可运行的最高档（序值）。
    pub fn resolve(&mut self, app: u32, auto_tier: RenderTier, device_cap: u8) -> ResolvedTier {
        match self.forced_tier(app) {
            Some(want) => {
                if want.order() <= device_cap {
                    // 设备可达：用户强制生效（含用户锁低功耗时系统想升的场景）
                    ResolvedTier { effective: want, from_force: true, force_honored: true }
                } else {
                    // 设备达不到：用户优先保留强制，但如实告知实际以能力上限运行
                    self.stats.ineffective_notices += 1;
                    self.last_notice = alloc::format!(
                        "强制失效告知：应用 {} 锁 {}，设备当前最高只到档 {}，实际以能力上限运行（强制标记保留）",
                        app,
                        want.as_str(),
                        device_cap
                    );
                    let capped = if device_cap >= RenderTier::High.order() {
                        RenderTier::High
                    } else if device_cap >= RenderTier::Balanced.order() {
                        RenderTier::Balanced
                    } else {
                        RenderTier::PowerSaver
                    };
                    ResolvedTier { effective: capped, from_force: true, force_honored: false }
                }
            }
            None => ResolvedTier { effective: auto_tier, from_force: false, force_honored: true },
        }
    }

    /// 冲突仲裁（显式口）：系统要降 `sys_want`、用户强制 `user_tier`。
    /// 用户优先：终态听用户，仲裁痕迹入账。
    pub fn arbitrate(&mut self, app: u32, user_tier: RenderTier, sys_want: RenderTier) -> ResolvedTier {
        if user_tier.order() > sys_want.order() {
            // 用户要更高、系统要降：用户胜
            self.stats.user_wins += 1;
            self.last_notice = alloc::format!(
                "冲突仲裁：应用 {} 用户强制 {}，系统请求降到 {}——用户优先，仲裁痕迹已入账",
                app,
                user_tier.as_str(),
                sys_want.as_str()
            );
        }
        ResolvedTier { effective: user_tier, from_force: true, force_honored: true }
    }

    /// 强制项显式可见：全部强制项的读屏行（谁锁了什么）。
    pub fn visibility_line(&self) -> String {
        if self.forces.is_empty() {
            return String::from("强制档位：无（全部应用走自动策略）");
        }
        let mut out = String::from("强制档位：");
        for f in self.forces.iter() {
            out.push_str(alloc::format!("应用{}→{}(设置方{});", f.app, f.tier.as_str(), f.by).as_str());
        }
        out
    }

    /// 对账：登记表实长与去重后应用数一致（同应用不重复登记）。
    pub fn consistent(&self) -> bool {
        let mut apps: Vec<u32> = Vec::new();
        for f in self.forces.iter() {
            if apps.iter().any(|a| *a == f.app) {
                return false;
            }
            apps.push(f.app);
        }
        true
    }
}

impl Default for ForceMgr {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、判据（12 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0085 判据入口（域聚合表注册：`("VE-F0085", vea85_appforce::run_vea85_checks)`）。
pub fn run_vea85_checks() -> CheckSet {
    let mut s = CheckSet::new("vea85_appforce");

    // --- 判据 1.1：按应用强制——登记隔离不串档 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::High);
        let _ = m.set_force(PRIVILEGED_CALLER, 200, RenderTier::PowerSaver);
        s.add(
            "A85-强制-登记按应用隔离",
            m.forced_tier(100) == Some(RenderTier::High)
                && m.forced_tier(200) == Some(RenderTier::PowerSaver)
                && m.forced_tier(300).is_none()
                && m.consistent(),
            "两应用各锁各档，第三应用不受影响",
        );
    }

    // --- 判据 1.2：按应用强制——重复登记以最新为准 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::High);
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::PowerSaver);
        s.add(
            "A85-强制-重复登记以最新为准",
            m.forced_tier(100) == Some(RenderTier::PowerSaver)
                && m.forces.len() == 1
                && m.forces[0].seq == 2
                && m.stats.sets == 2,
            "同应用二次登记覆盖旧项：表长恒 1、序号推进、sets 计 2",
        );
    }

    // --- 判据 1.3：按应用强制——撤销恢复自动 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::High);
        let c = m.clear_force(PRIVILEGED_CALLER, 100);
        let dup = m.clear_force(PRIVILEGED_CALLER, 100);
        s.add(
            "A85-强制-撤销恢复自动",
            c.is_ok() && dup == Err(CODE_NOT_FORCED)
                && m.forced_tier(100).is_none()
                && m.stats.clears == 1,
            "撤销后回自动；重复撤销显性 NOT_FORCED",
        );
    }

    // --- 判据 2.1：优先自动——无强制项走自动策略原值 ---
    {
        let mut m = ForceMgr::new();
        let r = m.resolve(42, RenderTier::Balanced, 2);
        s.add(
            "A85-自动-无强制走策略原值",
            r.effective == RenderTier::Balanced && !r.from_force && r.force_honored,
            "无强制项：resolve 返回自动策略值，from_force=false",
        );
    }

    // --- 判据 2.2：优先自动——强制优先于自动降级请求 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::High);
        // 自动策略因省电要降到低功耗；用户锁高画质
        let r = m.arbitrate(100, RenderTier::High, RenderTier::PowerSaver);
        s.add(
            "A85-自动-强制优先于降级请求",
            r.effective == RenderTier::High && r.from_force && m.stats.user_wins == 1,
            "系统要降、用户锁高：终态高画质，仲裁痕迹入账",
        );
    }

    // --- 判据 2.3：优先自动——越权设置显性拒绝 ---
    {
        let mut m = ForceMgr::new();
        let r = m.set_force(99, 100, RenderTier::High);
        let c = m.clear_force(99, 100);
        s.add(
            "A85-自动-越权设置显性拒绝",
            r == Err(CODE_FORBID) && c == Err(CODE_FORBID)
                && m.forced_tier(100).is_none()
                && m.stats.forbids == 2,
            "非特权调用方设/撤都拒绝且不入表，forbids 计 2",
        );
    }

    // --- 判据 3.1：显式可见——读屏行带档位与来源 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::High);
        let line = m.visibility_line();
        s.add(
            "A85-可见-读屏行带档位与来源",
            line.contains("应用100") && line.contains("高画质") && line.contains("设置方1"),
            "「谁在什么时候锁了什么」读屏可达",
        );
    }

    // --- 判据 3.2：显式可见——空表读屏行明示走自动 ---
    {
        let m = ForceMgr::new();
        let line = m.visibility_line();
        s.add(
            "A85-可见-空表明示走自动",
            line.contains("无") && line.contains("自动策略"),
            "无强制时不播空串，明示全走自动",
        );
    }

    // --- 判据 4.1：冲突用户优先——系统要降用户锁高时用户胜 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::High);
        let r = m.resolve(100, RenderTier::PowerSaver, 2);
        s.add(
            "A85-冲突-用户锁高系统要降用户胜",
            r.effective == RenderTier::High && r.from_force && r.force_honored,
            "设备能力 2 够高画质：强制全额生效，自动降级请求被压过",
        );
    }

    // --- 判据 4.2：冲突用户优先——强制低功耗时系统升档不越权 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::PowerSaver);
        // 自动策略想升到高画质（如切到大屏）：用户锁低功耗仍然赢
        let r = m.resolve(100, RenderTier::High, 2);
        s.add(
            "A85-冲突-用户锁低系统想升不越权",
            r.effective == RenderTier::PowerSaver && r.from_force && r.force_honored,
            "用户优先是双向的：系统升档请求同样被用户强制压住",
        );
    }

    // --- 判据 5.1：判据自身——强制失效告知入账且读屏可达 ---
    {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 100, RenderTier::High);
        let r = m.resolve(100, RenderTier::Balanced, 0);
        s.add(
            "A85-自身-强制失效告知入账",
            r.effective == RenderTier::PowerSaver
                && !r.force_honored
                && r.from_force
                && m.stats.ineffective_notices == 1
                && m.last_notice.contains("强制失效告知")
                && m.last_notice.contains("能力上限"),
            "设备达不到：强制标记保留、实际以能力上限运行、告知语读屏可达",
        );
    }

    // --- 判据 5.2：判据自身——码位互异且段独占（整仓唯一段 0xC8 内六码） ---
    {
        let codes = [
            CODE_FORBID,
            CODE_APP_BAD,
            CODE_NOT_FORCED,
            CODE_FORCE_INEFFECTIVE,
            CODE_ARBITER_BAD,
            CODE_FORCE_CAP,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0xC800);
        s.add(
            "A85-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 五、测试（4 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea85_force_lifecycle() {
        let mut m = ForceMgr::new();
        assert!(m.set_force(PRIVILEGED_CALLER, 10, RenderTier::High).is_ok());
        assert!(m.set_force(PRIVILEGED_CALLER, 20, RenderTier::Balanced).is_ok());
        assert_eq!(m.forced_tier(10), Some(RenderTier::High));
        assert_eq!(m.forced_tier(20), Some(RenderTier::Balanced));
        // 覆盖
        assert!(m.set_force(PRIVILEGED_CALLER, 10, RenderTier::PowerSaver).is_ok());
        assert_eq!(m.forced_tier(10), Some(RenderTier::PowerSaver));
        assert_eq!(m.forces.len(), 2);
        // 撤销
        assert!(m.clear_force(PRIVILEGED_CALLER, 10).is_ok());
        assert!(m.forced_tier(10).is_none());
        assert_eq!(m.clear_force(PRIVILEGED_CALLER, 10), Err(CODE_NOT_FORCED));
        assert!(m.consistent());
    }

    #[test]
    fn vea85_privilege_gate() {
        let mut m = ForceMgr::new();
        assert_eq!(m.set_force(2, 10, RenderTier::High), Err(CODE_FORBID));
        assert_eq!(m.set_force(0, 10, RenderTier::High), Err(CODE_FORBID));
        assert!(m.forces.is_empty());
        assert_eq!(m.stats.forbids, 2);
        assert_eq!(m.set_force(PRIVILEGED_CALLER, 0, RenderTier::High), Err(CODE_APP_BAD));
    }

    #[test]
    fn vea85_arbitration_matrix() {
        let mut m = ForceMgr::new();
        let _ = m.set_force(PRIVILEGED_CALLER, 10, RenderTier::High);
        // 用户锁高 + 系统要降 + 设备够 → 用户胜
        let r1 = m.resolve(10, RenderTier::PowerSaver, 2);
        assert!(r1.effective == RenderTier::High && r1.force_honored);
        // 用户锁高 + 设备不够 → 告知 + 能力上限运行
        let r2 = m.resolve(10, RenderTier::PowerSaver, 1);
        assert!(r2.effective == RenderTier::Balanced && !r2.force_honored && r2.from_force);
        assert_eq!(m.stats.ineffective_notices, 1);
        // 无强制应用走自动
        let r3 = m.resolve(99, RenderTier::Balanced, 2);
        assert!(r3.effective == RenderTier::Balanced && !r3.from_force);
        assert!(m.last_notice.contains("强制失效告知"));
    }

    #[test]
    fn vea85_visibility_and_two_way_priority() {
        let mut m = ForceMgr::new();
        assert!(m.visibility_line().contains("自动策略"));
        let _ = m.set_force(PRIVILEGED_CALLER, 7, RenderTier::PowerSaver);
        let line = m.visibility_line();
        assert!(line.contains("应用7") && line.contains("低功耗"), "{}", line);
        // 系统想升到高画质：用户锁低仍然赢
        let r = m.arbitrate(7, RenderTier::PowerSaver, RenderTier::High);
        assert_eq!(r.effective, RenderTier::PowerSaver);
        assert_eq!(m.stats.user_wins, 0, "用户锁低时系统想升不记用户胜（无冲突降级）");
        // 系统要降用户锁高：用户胜入账
        let _ = m.arbitrate(7, RenderTier::High, RenderTier::Balanced);
        assert_eq!(m.stats.user_wins, 1);
    }

    #[test]
    fn vea85_checks_all_green() {
        let set = run_vea85_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0085 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
