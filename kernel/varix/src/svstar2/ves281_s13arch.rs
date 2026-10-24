//! ves281_s13arch — VE-F4041 · T03 多语言排版总架构
//! 四族排版引擎统一架构（Latin/CJK/阿拉伯/泰印度系——管线单源红线）+ 架构五段（分词→整形→折行→定位→度量，各段语言感知签名冻结）+ 与 N02 分工复述 + 降级显性。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x75C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ArchCode {
    Ok = 0,
    PipelineSplit = SEG_BASE + 0x01,
    LangUnknown = SEG_BASE + 0x02,
    StageMismatch = SEG_BASE + 0x03,
    SigFrozen = SEG_BASE + 0x04,
    SilentDegrade = SEG_BASE + 0x05,
}

/// 四族。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Family { Latin = 0, Cjk = 1, Arabic = 2, ThaiIndic = 3 }

impl Family {
    pub const ALL: [Family; 4] = [Family::Latin, Family::Cjk, Family::Arabic, Family::ThaiIndic];
    pub fn name(self) -> &'static str {
        match self {
            Family::Latin => "Latin", Family::Cjk => "CJK",
            Family::Arabic => "阿拉伯", Family::ThaiIndic => "泰印度系",
        }
    }
}

/// 语言路由：未收录语言 → Latin 降级 + 显性标记。
pub struct LangRouter {
    pub known: Vec<(&'static str, Family)>,
}

impl LangRouter {
    pub fn canonical() -> Self {
        LangRouter {
            known: vec![
                ("en", Family::Latin), ("fr", Family::Latin), ("de", Family::Latin),
                ("zh", Family::Cjk), ("ja", Family::Cjk), ("ko", Family::Cjk),
                ("ar", Family::Arabic), ("fa", Family::Arabic),
                ("th", Family::ThaiIndic), ("hi", Family::ThaiIndic),
            ],
        }
    }
    pub fn route(&self, lang: &str) -> (Family, bool) {
        for k in self.known.iter() {
            if k.0 == lang { return (k.1, false); }
        }
        (Family::Latin, true) // 降级显性
    }
}

/// 五段管线段名（签名冻结）。
pub const STAGES: [&str; 5] = ["分词", "整形", "折行", "定位", "度量"];

/// 管线单源：四族共享同一管线（段签名一致），差异仅在策略层。
pub struct Pipeline {
    /// 每族每段的策略签名（族, 段序, 策略名）。
    pub policies: Vec<(Family, u8, &'static str)>,
}

impl Pipeline {
    /// 单源断言：四族段序一致（0..5），任何族不得缺段或增段。
    pub fn single_source(&self) -> bool {
        for f in Family::ALL.iter() {
            let mut stage = 0u8;
            for p in self.policies.iter() {
                if p.0 == *f {
                    if p.1 != stage { return false; }
                    stage += 1;
                }
            }
            if stage != STAGES.len() as u8 { return false; }
        }
        true
    }
    pub fn policy_of(&self, f: Family, stage: u8) -> Option<&'static str> {
        for p in self.policies.iter() {
            if p.0 == f && p.1 == stage { return Some(p.2); }
        }
        None
    }
    pub fn canonical() -> Self {
        Pipeline {
            policies: vec![
                (Family::Latin, 0, "空格分词"), (Family::Latin, 1, "GSUB"), (Family::Latin, 2, "UAX14"), (Family::Latin, 3, "基线"), (Family::Latin, 4, "kern 度量"),
                (Family::Cjk, 0, "字素分词"), (Family::Cjk, 1, "等宽"), (Family::Cjk, 2, "禁则"), (Family::Cjk, 3, "方格"), (Family::Cjk, 4, "全角度量"),
                (Family::Arabic, 0, "词干分词"), (Family::Arabic, 1, "GSUB 阿族"), (Family::Arabic, 2, "整词折行"), (Family::Arabic, 3, "连写基线"), (Family::Arabic, 4, "连写度量"),
                (Family::ThaiIndic, 0, "音节分词"), (Family::ThaiIndic, 1, "组合标记"), (Family::ThaiIndic, 2, "音节断行"), (Family::ThaiIndic, 3, "锚定基线"), (Family::ThaiIndic, 4, "堆叠度量"),
            ],
        }
    }
}

/// 段间对拍：下游输入规格必须匹配上游输出规格。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StageSpec {
    pub emits: u8, // 输出规格代号
    pub accepts: u8,
}

pub fn stage_pair_ok(up: StageSpec, down: StageSpec) -> bool {
    up.emits == down.accepts
}

/// 与 N02 分工：N02 执行、T03 策略深化——分工复述单源。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Owner { N02Exec = 0, T03Policy = 1 }

pub struct Division {
    pub entries: Vec<(&'static str, Owner)>,
}

impl Division {
    pub fn canonical() -> Self {
        Division {
            entries: vec![
                ("管线执行", Owner::N02Exec),
                ("断行策略", Owner::T03Policy),
                ("整形策略", Owner::T03Policy),
                ("度量执行", Owner::N02Exec),
            ],
        }
    }
    pub fn retell(&self, other: &[(&'static str, Owner)]) -> bool {
        if other.len() != self.entries.len() { return false; }
        for e in self.entries.iter() {
            let mut found = false;
            for r in other.iter() {
                if r.0 == e.0 { found = r.1 == e.1; }
            }
            if !found { return false; }
        }
        true
    }
}

pub fn run_ves281_checks() -> CheckSet {
    let mut set = CheckSet::new("ves281_s13arch");
    let router = LangRouter::canonical();
    let pipe = Pipeline::canonical();

    // 1) 四族路由齐。
    set.add("C4041-01", router.route("zh") == (Family::Cjk, false)
        && router.route("ar") == (Family::Arabic, false)
        && router.route("th") == (Family::ThaiIndic, false), "四族路由");

    // 2) 未收录语言 Latin 降级显性。
    let (f, degr) = router.route("xx");
    set.add("C4041-02", f == Family::Latin && degr, "降级显性");

    // 3) 管线单源：四族段序一致。
    set.add("C4041-03", pipe.single_source(), "管线单源");

    // 4) 分叉检出：缺段即红。
    let mut split = Pipeline::canonical();
    split.policies.retain(|p| !(p.0 == Family::Latin && p.1 == 3));
    set.add("C4041-04", !split.single_source(), "缺段分叉检出");

    // 5) 策略查表。
    set.add("C4041-05", pipe.policy_of(Family::Cjk, 2) == Some("禁则")
        && pipe.policy_of(Family::Latin, 2) == Some("UAX14"), "策略查表");

    // 6) 五段签名冻结。
    set.add("C4041-06", STAGES == ["分词", "整形", "折行", "定位", "度量"], "五段签名");

    // 7) 段间失配检出。
    let up = StageSpec { emits: 2, accepts: 0 };
    let down = StageSpec { emits: 1, accepts: 1 };
    set.add("C4041-07", !stage_pair_ok(up, down), "段间失配");

    // 8) 段间匹配通过。
    let up2 = StageSpec { emits: 1, accepts: 1 };
    let down2 = StageSpec { emits: 1, accepts: 1 };
    set.add("C4041-08", stage_pair_ok(up2, down2), "段间匹配");

    // 9) 分工复述单源。
    let div = Division::canonical();
    let retell: Vec<(&str, Owner)> = div.entries.clone();
    set.add("C4041-09", div.retell(&retell), "分工复述一致");

    // 10) 分工分叉检出。
    let mut bad = div.entries.clone();
    bad[0].1 = Owner::T03Policy;
    set.add("C4041-10", !div.retell(&bad), "分工分叉检出");

    // 11) 静默降级拒：路由器对未知语言必须显性。
    let (f2, d2) = router.route("");
    set.add("C4041-11", f2 == Family::Latin && d2, "空语言显性降级");

    // 12) 四族名双射。
    let mut names: Vec<&str> = Vec::new();
    for f in Family::ALL.iter() { names.push(f.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4041-12", !ndup && names.len() == 4, "四族名双射");

    // 13) 策略条目总量 = 4 族 × 5 段。
    set.add("C4041-13", pipe.policies.len() == 20, "策略 20 条");

    // 14) 段位自查。
    set.add("C4041-14", SEG_BASE == 0x75C0, "段位 0x75C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_and_pipeline() {
        let r = LangRouter::canonical();
        assert_eq!(r.route("ja"), (Family::Cjk, false));
        assert_eq!(r.route("zz"), (Family::Latin, true));
        assert!(Pipeline::canonical().single_source());
    }
}
