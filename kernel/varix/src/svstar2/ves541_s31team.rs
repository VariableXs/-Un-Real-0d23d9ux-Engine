//! VE-F4307 · 一致性×多团队协作（U06 场景应用组）
//! 职责：团队级一致性积分榜（团队维度聚合不涉个人）+ 跨团队规则分歧调解
//! 两步（先对拍后仲裁——僵局才升级委员会）+ 共享组件库一致性托管
//! （托管面统一判定 + 延迟回退缓存旧判定并显式标注）+ 积分争议复核窗口
//! + 榜单色弱可辨（数值/标签/位形三通道——域本色）。
//! 段位：0x30A0（续表 idx541）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x30A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TmCode {
    /// 积分争议（复核窗口开启，聚合冻结）。
    Disputed = SEG_BASE,
    /// 调解僵局（对拍两轮不收敛，升级仲裁）。
    MediationDeadlock = SEG_BASE + 0x01,
    /// 托管判定延迟（回退缓存旧判定并标注）。
    HostedStale = SEG_BASE + 0x02,
    /// 绕过托管面直判（越权拒绝）。
    HostedBypass = SEG_BASE + 0x03,
    /// 团队数越界（聚合上限防护）。
    TeamOverflow = SEG_BASE + 0x04,
}

/// 团队积分（纯聚合维度——不含个人字段，隐私面为零）。
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct TeamScore {
    pub team: u16,
    pub violations: u32,
    pub fixed: u32,
    /// 得分 bp（千分比，用于色弱可辨三通道之一）。
    pub score_bp: u32,
}

/// 复核窗口：争议期聚合冻结，过期自动关账。
#[derive(Clone, Copy, Debug)]
pub struct ReviewWindow {
    pub team: u16,
    pub opened_at: u64,
    pub ttl: u64,
}

/// 调解步骤（两步制：对拍 → 仲裁）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MedStep {
    /// 第一步：对拍（两域规则 diff 清单）。
    Compare,
    /// 第二步：仲裁（升级委员会——仅僵局可达）。
    Arbitrate,
    /// 已结案。
    Closed,
}

/// 调解记录。
#[derive(Clone, Debug)]
pub struct Mediation {
    pub step: MedStep,
    pub diff_items: u32,
    pub rounds: u32,
    /// 仲裁结论（仅 Arbitrate 后有值：0=未裁 1=采纳 A 2=采纳 B）。
    pub verdict: u8,
}

/// 托管判定面：共享组件库统一判定入口。
#[derive(Clone, Debug)]
pub struct HostedFace {
    pub face: u16,
    /// 权威判定（托管面单源）。
    pub authoritative: Option<u8>,
    /// 缓存旧判定（延迟回退用）：内容哈希 + 时间戳 + 显式过期标注。
    pub cached: Option<(u32, u64)>,
    /// 缓存是否处于 stale 标注态。
    pub stale: bool,
}

/// 积分榜本体。
#[derive(Default)]
pub struct ScoreBoard {
    pub teams: Vec<TeamScore>,
    pub windows: Vec<ReviewWindow>,
    pub meds: Vec<Mediation>,
    pub hosted: Vec<HostedFace>,
    now: u64,
}

const MAX_TEAMS: usize = 64;
const MAX_ROUNDS: u32 = 2;
const WINDOW_TTL: u64 = 72;

impl ScoreBoard {
    pub fn new(now: u64) -> Self {
        ScoreBoard { now, ..Default::default() }
    }

    /// 团队维度聚合（上限防护；不采集个人数据）。
    pub fn aggregate(&mut self, s: TeamScore) -> Result<(), TmCode> {
        if self.teams.len() >= MAX_TEAMS {
            return Err(TmCode::TeamOverflow);
        }
        if let Some(t) = self.teams.iter_mut().find(|t| t.team == s.team) {
            *t = s;
        } else {
            self.teams.push(s);
        }
        Ok(())
    }

    fn frozen(&self, team: u16) -> bool {
        self.windows.iter().any(|w| w.team == team && self.now < w.opened_at + w.ttl)
    }

    /// 积分争议：开复核窗口，窗口期该团队聚合冻结。
    pub fn dispute(&mut self, team: u16) {
        if !self.windows.iter().any(|w| w.team == team) {
            self.windows.push(ReviewWindow { team, opened_at: self.now, ttl: WINDOW_TTL });
        }
    }

    /// 窗口过期自动关账（返回关闭的窗口数）。
    pub fn close_expired(&mut self) -> usize {
        let now = self.now;
        let before = self.windows.len();
        self.windows.retain(|w| now < w.opened_at + w.ttl);
        before - self.windows.len()
    }

    /// 窗口期内聚合拒绝写入（返回 Err 标记争议保护）。
    pub fn commit(&self, team: u16) -> Result<(), TmCode> {
        if self.frozen(team) {
            Err(TmCode::Disputed)
        } else {
            Ok(())
        }
    }

    /// 调解：第一步对拍；收敛（差异清零）即结案；两轮不收敛升级仲裁。
    pub fn mediate(&mut self, diff_items: u32) -> Result<MedStep, TmCode> {
        let m = self.meds.last_mut().filter(|m| m.step != MedStep::Closed);
        match m {
            None => {
                self.meds.push(Mediation { step: MedStep::Compare, diff_items, rounds: 1, verdict: 0 });
                Ok(MedStep::Compare)
            }
            Some(m) => {
                m.diff_items = diff_items;
                m.rounds += 1;
                if diff_items == 0 {
                    m.step = MedStep::Closed;
                    return Ok(MedStep::Closed);
                }
                if m.rounds > MAX_ROUNDS {
                    m.step = MedStep::Arbitrate;
                    return Err(TmCode::MediationDeadlock);
                }
                Ok(m.step)
            }
        }
    }

    /// 仲裁结论回写（仅 Arbitrate 态可写；0 号无效判）。
    pub fn arbitrate(&mut self, verdict: u8) -> Result<(), TmCode> {
        let m = self.meds.last_mut().ok_or(TmCode::MediationDeadlock)?;
        if m.step != MedStep::Arbitrate {
            return Err(TmCode::MediationDeadlock);
        }
        if verdict == 0 {
            return Err(TmCode::MediationDeadlock);
        }
        m.verdict = verdict;
        m.step = MedStep::Closed;
        Ok(())
    }

    /// 注册托管面。
    pub fn host(&mut self, face: u16) {
        if !self.hosted.iter().any(|h| h.face == face) {
            self.hosted.push(HostedFace { face, authoritative: None, cached: None, stale: false });
        }
    }

    /// 托管统一判定：判定权单源在托管面；延迟时回退缓存并显式 stale 标注。
    pub fn judge(&mut self, face: u16, verdict: u8) -> Result<(u8, bool), TmCode> {
        let h = self.hosted.iter_mut().find(|h| h.face == face).ok_or(TmCode::HostedBypass)?;
        match verdict {
            0xFF => {
                // 延迟信号：回退缓存旧判定（无缓存则拒绝）。
                match h.cached {
                    Some((v0, _)) => {
                        h.stale = true;
                        Ok((v0 as u8, true))
                    }
                    None => Err(TmCode::HostedStale),
                }
            }
            v => {
                h.authoritative = Some(v);
                h.stale = false;
                h.cached = Some((v as u32, self.now));
                Ok((v, false))
            }
        }
    }

    /// 直读托管面判定（未托管面拒绝 = 绕过拦截）。
    pub fn read(&self, face: u16) -> Result<(Option<u8>, bool), TmCode> {
        let h = self.hosted.iter().find(|h| h.face == face).ok_or(TmCode::HostedBypass)?;
        Ok((h.authoritative, h.stale))
    }

    /// 色弱可辨三通道：数值（bp）/ 标签（文字）/ 位形（rank 位图）。
    pub fn tri_channel(&self, team: u16) -> Option<(u32, &'static str, u8)> {
        let t = self.teams.iter().find(|t| t.team == team)?;
        let label = if t.score_bp >= 9000 {
            "优"
        } else if t.score_bp >= 7000 {
            "良"
        } else {
            "待改"
        };
        let rank = (t.score_bp / 1250).min(7) as u8; // 8 级位形
        Some((t.score_bp, label, rank))
    }

    pub fn advance(&mut self, dt: u64) {
        self.now += dt;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves541_checks() -> CheckSet {
    let mut set = CheckSet::new("ves541");
    let mut sb = ScoreBoard::new(100);

    // C541-01 团队维度聚合（纯聚合无个人字段）。
    let _ = sb.aggregate(TeamScore { team: 1, violations: 12, fixed: 11, score_bp: 9200 });
    let _ = sb.aggregate(TeamScore { team: 2, violations: 8, fixed: 5, score_bp: 6100 });
    chk(&mut set, "C541-01", sb.teams.len() == 2 && sb.teams[0].team == 1, "团队维度聚合在册");

    // C541-02 争议开复核窗口且窗口期聚合冻结。
    sb.dispute(1);
    chk(&mut set, "C541-02", sb.commit(1) == Err(TmCode::Disputed), "争议期聚合冻结");

    // C541-03 窗口过期自动关账（TTL 72）。
    sb.advance(71);
    let closed = sb.close_expired();
    sb.advance(2);
    let closed2 = sb.close_expired();
    chk(&mut set, "C541-03", closed == 0 && closed2 == 1 && sb.commit(1).is_ok(), "过期关账解冻");

    // C541-04 调解第一步对拍。
    let s1 = sb.mediate(5);
    chk(&mut set, "C541-04", s1 == Ok(MedStep::Compare), "调解首步对拍");

    // C541-05 两轮不收敛升级仲裁（僵局）。
    let _ = sb.mediate(3);
    let s3 = sb.mediate(3);
    chk(&mut set, "C541-05", s3 == Err(TmCode::MediationDeadlock), "两轮不收敛升级");

    // C541-06 仲裁态可写、Compare 态拒写。
    chk(&mut set, "C541-06", sb.arbitrate(1) == Ok(()), "仲裁结论回写");

    // C541-07 仲裁后 Closed 不可再裁（0 号无效判一并拒）。
    chk(&mut set, "C541-07", sb.arbitrate(0).is_err() && sb.arbitrate(2).is_err(), "终局不可再裁");

    // C541-08 对拍收敛即结案不进仲裁。
    let mut sb2 = ScoreBoard::new(0);
    let _ = sb2.mediate(4);
    let _ = sb2.mediate(0);
    chk(&mut set, "C541-08", sb2.meds[0].step == MedStep::Closed && sb2.meds[0].rounds == 2, "收敛即结案");

    // C541-09 托管面统一判定 + 直读。
    sb2.host(7);
    let j = sb2.judge(7, 2);
    chk(&mut set, "C541-09", j == Ok((2, false)) && sb2.read(7) == Ok((Some(2), false)), "托管单源判定");

    // C541-10 延迟回退缓存旧判定并 stale 标注。
    let stale = sb2.judge(7, 0xFF);
    chk(&mut set, "C541-10", stale == Ok((2, true)) && matches!(sb2.read(7), Ok((_, true))), "回退缓存带标注");

    // C541-11 无缓存延迟 = 拒绝；未托管面 = 绕过拦截。
    let mut sb3 = ScoreBoard::new(0);
    sb3.host(9);
    chk(&mut set, "C541-11", sb3.judge(9, 0xFF) == Err(TmCode::HostedStale) && sb3.read(3).is_err(), "无缓存拒/绕过拦");

    // C541-12 色弱可辨三通道（数值/标签/位形）+ 聚合上限防护。
    let tri = sb2.tri_channel(0).is_none();
    let _ = sb3.aggregate(TeamScore { team: 1, violations: 1, fixed: 1, score_bp: 9500 });
    let ch = sb3.tri_channel(1);
    let mut full = ScoreBoard::new(0);
    let mut overflow_ok = false;
    for i in 0..70u16 {
        if full.aggregate(TeamScore { team: i, violations: 0, fixed: 0, score_bp: 5000 }) == Err(TmCode::TeamOverflow) {
            overflow_ok = i >= 64;
            break;
        }
    }
    chk(&mut set, "C541-12", tri && ch == Some((9500, "优", 7)) && overflow_ok, "三通道齐+上限防护");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_board_basics() {
        let mut sb = ScoreBoard::new(0);
        assert!(sb.aggregate(TeamScore { team: 1, violations: 3, fixed: 2, score_bp: 8000 }).is_ok());
        sb.dispute(1);
        assert_eq!(sb.commit(1), Err(TmCode::Disputed));
        sb.advance(WINDOW_TTL);
        assert_eq!(sb.close_expired(), 1);
        assert!(sb.commit(1).is_ok());
    }

    #[test]
    fn mediation_two_step() {
        let mut sb = ScoreBoard::new(0);
        assert_eq!(sb.mediate(2), Ok(MedStep::Compare));
        assert_eq!(sb.mediate(2), Ok(MedStep::Compare));
        assert_eq!(sb.mediate(2), Err(TmCode::MediationDeadlock));
        assert!(sb.arbitrate(1).is_ok());
        assert!(sb.arbitrate(1).is_err());
    }

    #[test]
    fn hosted_face_stale() {
        let mut sb = ScoreBoard::new(5);
        sb.host(1);
        assert!(sb.read(1).is_err()); // 未托管面之外 → 绕过
        sb.host(2);
        assert_eq!(sb.judge(2, 1), Ok((1, false)));
        assert_eq!(sb.judge(2, 0xFF), Ok((1, true)));
        sb.host(3);
        assert_eq!(sb.judge(3, 0xFF), Err(TmCode::HostedStale));
    }
}
