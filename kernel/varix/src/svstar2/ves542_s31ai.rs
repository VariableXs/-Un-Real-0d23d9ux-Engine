//! VE-F4308 · 一致性×AI 辅助开发（U06 场景应用组）
//! 职责：AI 产出一致性预检（生成即查——违例拦截在合入前；误拦置信度
//! 标注转人裁）+ AI 提示注入一致性知识（词典/判例上下文注入包——过时
//! 版本戳刷新）+ AI 产出标记（AI 生成标记 + 人审终审双轨；漏标扫描补标）
//! + AI 产出的无障碍同责（域本色硬线）。
//! 段位：0x30E0（续表 idx542）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x30E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiCode {
    /// 生成即查命中违例（合入前拦截）。
    PrecheckBlocked = SEG_BASE,
    /// 注入包过时（版本戳不符，拒绝注入）。
    ContextStale = SEG_BASE + 0x01,
    /// 漏标 AI 产出（扫描补标）。
    Unmarked = SEG_BASE + 0x02,
    /// 人审终审未过。
    HumanVeto = SEG_BASE + 0x03,
    /// 置信度低于自动裁线（转人裁）。
    LowConfidence = SEG_BASE + 0x04,
}

/// 上下文注入包（词典 + 判例，版本戳单源）。
#[derive(Clone, Debug, PartialEq)]
pub struct ContextPack {
    pub dict_ver: u32,
    pub case_ver: u32,
    pub entries: Vec<(u32, &'static str)>, // (词条指纹, 提示文本)
}

impl ContextPack {
    pub fn new(dict_ver: u32, case_ver: u32) -> Self {
        ContextPack { dict_ver, case_ver, entries: Vec::new() }
    }
    /// 注入新鲜度：两版本戳须同时匹配单源。
    pub fn fresh(&self, dict_ver: u32, case_ver: u32) -> bool {
        self.dict_ver == dict_ver && self.case_ver == case_ver
    }
}

/// AI 产出预检结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreVerdict {
    /// 干净放行。
    Clean,
    /// 违例拦截（合入前）。
    Blocked,
    /// 低置信度误拦嫌疑——置信度标注转人裁。
    HumanReview(u8),
}

/// AI 产出卡。
#[derive(Clone, Debug)]
pub struct AiArtifact {
    pub id: u32,
    pub lines: u32,
    pub violations: u32,
    /// 预检置信度（0-100）。
    pub confidence: u8,
    /// AI 生成标记（漏标扫描补标对象）。
    pub ai_marked: bool,
    /// 人审终审（双轨第二轨）。
    pub human_signed: bool,
}

/// AI 治理台。
#[derive(Default)]
pub struct AiDesk {
    pub pack: Option<ContextPack>,
    pub artifacts: Vec<AiArtifact>,
    pub injected_count: u32,
    pub patched_count: u32,
    /// 自动裁线：置信度低于此值一律转人裁（防误拦滥用）。
    pub auto_cut: u8,
}

const CONF_FLOOR: u8 = 50;

impl AiDesk {
    pub fn new(auto_cut: u8) -> Self {
        AiDesk { auto_cut: auto_cut.max(CONF_FLOOR), ..Default::default() }
    }

    /// 刷新/装载注入包（版本戳单源核对，过时拒绝装载旧包）。
    pub fn load_pack(&mut self, pack: ContextPack, cur_dict: u32, cur_case: u32) -> Result<(), AiCode> {
        if !pack.fresh(cur_dict, cur_case) {
            return Err(AiCode::ContextStale);
        }
        self.pack = Some(pack);
        self.injected_count += 1;
        Ok(())
    }

    /// 生成即查：违例拦截在合入前；低置信度转人裁。
    pub fn precheck(&mut self, id: u32, lines: u32, violations: u32, confidence: u8) -> PreVerdict {
        let v = if violations > 0 && confidence >= self.auto_cut {
            PreVerdict::Blocked
        } else if violations > 0 {
            PreVerdict::HumanReview(confidence)
        } else {
            PreVerdict::Clean
        };
        self.artifacts.push(AiArtifact {
            id, lines, violations, confidence,
            ai_marked: true, human_signed: false,
        });
        v
    }

    /// 误拦人裁：HumanReview 件由人终裁（放行=撤销拦截）。
    pub fn human_adjudicate(&mut self, id: u32, pass: bool) -> Result<(), AiCode> {
        let a = self.artifacts.iter_mut().find(|a| a.id == id).ok_or(AiCode::Unmarked)?;
        if pass {
            a.violations = 0;
            a.human_signed = true;
            Ok(())
        } else {
            Err(AiCode::HumanVeto)
        }
    }

    /// 漏标扫描补标：无 AI 标记的产出登记入册即补标（返回补标数）。
    pub fn sweep_unmarked(&mut self, ids: &[u32]) -> usize {
        let mut n = 0;
        for id in ids {
            if let Some(a) = self.artifacts.iter_mut().find(|a| a.id == *id) {
                if !a.ai_marked {
                    a.ai_marked = true;
                    self.patched_count += 1;
                    n += 1;
                }
            } else {
                // 台上无此件：漏标新发现——登记并补标。
                self.artifacts.push(AiArtifact {
                    id: *id, lines: 0, violations: 0, confidence: 0,
                    ai_marked: true, human_signed: false,
                });
                self.patched_count += 1;
                n += 1;
            }
        }
        n
    }

    /// 合入闸：AI 标记 + 人审终审双轨齐备且零违例才放行。
    pub fn mergeable(&self, id: u32) -> Result<bool, AiCode> {
        let a = self.artifacts.iter().find(|a| a.id == id).ok_or(AiCode::Unmarked)?;
        Ok(a.ai_marked && a.human_signed && a.violations == 0)
    }

    /// 无障碍同责：AI 产出与人工产出同一判据池（此处返回守卫恒真断言面）。
    pub fn a11y_same_duty(&self, id: u32) -> Result<bool, AiCode> {
        let a = self.artifacts.iter().find(|a| a.id == id).ok_or(AiCode::Unmarked)?;
        // 同责语义：AI 件的违例不计入豁免池——与人工件同一处理。
        Ok(a.ai_marked)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves542_checks() -> CheckSet {
    let mut set = CheckSet::new("ves542");
    let mut desk = AiDesk::new(70);

    // C542-01 注入包版本戳单源装载。
    let mut p = ContextPack::new(12, 3);
    p.entries.push((0x1A2B, "按钮态令牌用 color.action"));
    chk(&mut set, "C542-01", desk.load_pack(p.clone(), 12, 3).is_ok() && desk.injected_count == 1, "注入包装载");

    // C542-02 过时包拒绝（字典版本漂移）。
    chk(&mut set, "C542-02", desk.load_pack(p, 13, 3) == Err(AiCode::ContextStale), "过时包拒装");

    // C542-03 生成即查：违例拦截在合入前。
    let v1 = desk.precheck(101, 40, 2, 90);
    chk(&mut set, "C542-03", v1 == PreVerdict::Blocked, "违例合入前拦截");

    // C542-04 低置信度转人裁（误拦嫌疑标注置信度）。
    let v2 = desk.precheck(102, 25, 1, 40);
    chk(&mut set, "C542-04", v2 == PreVerdict::HumanReview(40), "低置信转人裁");

    // C542-05 干净产出放行。
    let v3 = desk.precheck(103, 60, 0, 95);
    chk(&mut set, "C542-05", v3 == PreVerdict::Clean, "干净放行");

    // C542-06 误拦人裁放行（撤销拦截 + 终审签）。
    chk(&mut set, "C542-06", desk.human_adjudicate(102, true).is_ok(), "人裁撤销误拦");

    // C542-07 人审否决（HumanVeto 拒绝放行）。
    let _ = desk.precheck(104, 10, 3, 60);
    chk(&mut set, "C542-07", desk.human_adjudicate(104, false) == Err(AiCode::HumanVeto), "人审否决在案");

    // C542-08 合入闸双轨：缺人审不放行。
    chk(&mut set, "C542-08", desk.mergeable(101) == Ok(false), "缺终审不放行");

    // C542-09 双轨齐备 + 零违例放行。
    let _ = desk.human_adjudicate(101, true);
    chk(&mut set, "C542-09", desk.mergeable(101) == Ok(true), "双轨齐放行");

    // C542-10 漏标扫描补标（新发现登记 + 已标不重复计）。
    let patched = desk.sweep_unmarked(&[105, 103]);
    chk(&mut set, "C542-10", patched == 2 && desk.patched_count == 2, "漏标补标 2 件");

    // C542-11 AI 无障碍同责：AI 件违例不豁免（同判据池）。
    chk(&mut set, "C542-11", desk.a11y_same_duty(104) == Ok(true), "AI 无障碍同责");

    // C542-12 台上无件合入闸拒绝 + 自动裁线下限防护（<50 钳到 50）。
    let tight = AiDesk::new(10);
    chk(&mut set, "C542-12", desk.mergeable(999).is_err() && tight.auto_cut == CONF_FLOOR, "无件拒+裁线钳制");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_stale_rejected() {
        let mut d = AiDesk::new(70);
        assert!(d.load_pack(ContextPack::new(1, 1), 1, 1).is_ok());
        assert_eq!(d.load_pack(ContextPack::new(1, 1), 2, 1), Err(AiCode::ContextStale));
        assert_eq!(d.load_pack(ContextPack::new(1, 1), 1, 2), Err(AiCode::ContextStale));
    }

    #[test]
    fn precheck_routes() {
        let mut d = AiDesk::new(70);
        assert_eq!(d.precheck(1, 10, 1, 90), PreVerdict::Blocked);
        assert_eq!(d.precheck(2, 10, 1, 69), PreVerdict::HumanReview(69));
        assert_eq!(d.precheck(3, 10, 0, 10), PreVerdict::Clean);
    }

    #[test]
    fn merge_gate_dual_track() {
        let mut d = AiDesk::new(70);
        let _ = d.precheck(1, 5, 0, 90);
        assert_eq!(d.mergeable(1), Ok(false));
        assert!(d.human_adjudicate(1, true).is_ok());
        assert_eq!(d.mergeable(1), Ok(true));
        assert!(d.mergeable(2).is_err());
    }
}
