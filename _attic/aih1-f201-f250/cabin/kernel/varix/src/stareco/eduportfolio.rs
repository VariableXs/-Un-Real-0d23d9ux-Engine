//! F145 教育/作品集友好 · 完整设计（STAR I 主册 G-D-20）。
//!
//! **判据（主册）**：首批公开 20 篇（四主题各 5）；脱敏三查通过率
//! 100%（注入敏感样本测试）。
//!
//! **设计要点（主册）**：三册图纸+设计案脱敏版公开读物化；脱敏管线
//! 构建期自动：实机编号/账号/路径用户段/内部密钥**四类硬删**；每版
//! 人工复核 20 处；文章分级（入门/进阶/深水区）；决策注解保留原
//! ADR 编号可溯；四主题（引导/存储/兼容/体验）；公开版随版本窗同步
//! （F138）；学生反馈通道进 F139。
//!
//! 本模块是脱敏管线的**判定核**：四类硬删检测（构建期自动）、分级
//! 登记表、四主题配额（各 5 篇——首批 20 篇判据）、抽查复核台账。

use crate::checks::CheckSet;
use crate::stareco::ebase::TraceId;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 四主题（首批公开 20 篇 = 各 5）。
pub const THEMES: [&str; 4] = ["boot", "storage", "compat", "experience"];
/// 每主题篇数。
pub const PER_THEME: usize = 5;
/// 文章分级。
pub const LEVELS: [&str; 3] = ["starter", "advanced", "deep"];
/// 每版人工复核处数。
pub const AUDIT_SAMPLES: usize = 20;

// ---------------------------------------------------------------------------
// 脱敏管线（四类硬删——构建期自动）
// ---------------------------------------------------------------------------

/// 四类硬删检测。任一命中即「未过脱敏」，公开版禁止生成——
/// 不是打码（打码可能漏），是整篇扣住（硬删纪律）。
pub fn desensitized(text: &str) -> bool {
    // ①实机编号：VARIX-SN- / 机号: 形态
    // ②账号：user= / 账号: 形态
    // ③路径用户段：C:\Users\ / /home/
    // ④内部密钥：KEY= / BEGIN … KEY
    const BANNED: [&str; 8] = [
        "VARIX-SN-", "机号:", "user=", "账号:", "C:\\Users\\", "/home/", "KEY=", "BEGIN RSA",
    ];
    !BANNED.iter().any(|b| text.contains(b))
}

// ---------------------------------------------------------------------------
// 文章登记
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Article {
    pub id: TraceId,
    pub title: &'static str,
    pub theme: &'static str,
    pub level: &'static str,
    /// 决策注解保留的原 ADR 编号（0 = 无决策注解——判据要求每篇带）。
    pub adr_no: u32,
    /// 脱敏状态（构建期管线产出）。
    pub clean: bool,
}

impl Article {
    pub fn publishable(&self) -> bool {
        self.clean && self.adr_no > 0 && THEMES.contains(&self.theme) && LEVELS.contains(&self.level)
    }
}

/// 公开读物登记表。
pub struct Portfolio {
    articles: [Option<Article>; 24],
    count: usize,
    seq: crate::stareco::ebase::SeqAlloc,
    /// 抽查复核台账：每版 20 处的复核记录（轮数, 发现数）。
    pub audit_rounds: u32,
    pub audit_findings: u32,
}

impl Portfolio {
    pub fn new() -> Portfolio {
        Portfolio {
            articles: [None; 24],
            count: 0,
            seq: crate::stareco::ebase::SeqAlloc::new(),
            audit_rounds: 0,
            audit_findings: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 收录一篇：脱敏过 + ADR 注解在册 + 主题分级合法。
    pub fn admit(&mut self, day: u32, title: &'static str, theme: &'static str, level: &'static str, adr_no: u32, clean: bool) -> Result<TraceId, &'static str> {
        let probe = Article {
            id: TraceId::new("EDU", day, 1),
            title,
            theme,
            level,
            adr_no,
            clean,
        };
        if !probe.publishable() {
            return Err("未过脱敏或缺决策注解");
        }
        if self.count >= 24 {
            return Err("portfolio full");
        }
        let s = self.seq.take(day);
        let id = TraceId::new("EDU", day, s);
        self.articles[self.count] = Some(Article { id, ..probe });
        self.count += 1;
        Ok(id)
    }

    /// 首批 20 篇判据：四主题各 5。
    pub fn first_batch_complete(&self) -> bool {
        THEMES.iter().all(|t| {
            self.articles[..self.count].iter().flatten().filter(|a| a.theme == *t).count() >= PER_THEME
        })
    }

    pub fn count_theme(&self, theme: &str) -> usize {
        self.articles[..self.count].iter().flatten().filter(|a| a.theme == theme).count()
    }

    /// 分级分布（入门/进阶/深水区——学习曲线分层的数据源）。
    pub fn count_level(&self, level: &str) -> usize {
        self.articles[..self.count].iter().flatten().filter(|a| a.level == level).count()
    }

    /// 抽查复核：一轮 20 处，`findings` 为命中数（>0 = 版本回炉）。
    pub fn audit_round(&mut self, findings: u32) {
        self.audit_rounds += 1;
        self.audit_findings += findings;
    }

    /// 本版干净 = 最近一轮抽查零命中。
    pub fn last_audit_clean(&self) -> bool {
        self.audit_rounds > 0 && self.audit_findings == 0
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F145_TAG: &str = "stareco-F145-eduportfolio";

pub fn run_eduportfolio_checks() -> CheckSet {
    let mut set = CheckSet::new(F145_TAG);
    let day = 20260926;

    // 脱敏四类硬删：注入敏感样本全部命中
    set.add("f145 clean text passes", desensitized("普通技术叙述，无敏感物"), "clean");
    set.add("f145 serial banned", !desensitized("本机 VARIX-SN-0042"), "machine serial");
    set.add("f145 machine tag banned", !desensitized("机号: Y7000-07"), "machine tag");
    set.add("f145 account banned", !desensitized("user=variable 登录"), "account");
    set.add("f145 account zh banned", !desensitized("账号: var"), "account zh");
    set.add("f145 win userpath banned", !desensitized("日志 C:\\Users\\me\\a.log"), "win path");
    set.add("f145 unix home banned", !desensitized("存于 /home/me/"), "unix path");
    set.add("f145 key banned", !desensitized("KEY=0xdeadbeef"), "internal key");
    set.add("f145 pem banned", !desensitized("-----BEGIN RSA PRIVATE"), "pem head");

    // 收录门禁：缺 ADR 注解 / 未脱敏 / 非法主题 都拒
    let mut pf = Portfolio::new();
    set.add("f145 no-adr rejected", pf.admit(day, "t", "boot", "starter", 0, true).is_err(), "annotation required");
    set.add("f145 dirty rejected", pf.admit(day, "t", "boot", "starter", 7, false).is_err(), "desensitize first");
    set.add("f145 bad theme rejected", pf.admit(day, "t", "gossip", "starter", 7, true).is_err(), "four themes only");
    set.add("f145 bad level rejected", pf.admit(day, "t", "boot", "expert", 7, true).is_err(), "three levels only");

    // 首批 20 篇：四主题各 5（分级错落——学习曲线分层）
    let titles = [
        "引导的三道闸", "Limine 交接面", "UEFI 与 NVRAM 纪律", "诊断旗标设计", "引导红线案例",
        "exFAT 直写扇区学", "断电百次判据", "页缓存水位三档", "快照滚动调度", "存储灾备 SOP",
        "兼容层分层", "差异表诚实学", "安装器三族判例", "运行时直装族", "迁移窗纪律",
        "帧率账本", "合成器脏区", "动效总谱", "体验日志章法", "异常显性化",
    ];
    for (i, t) in titles.iter().enumerate() {
        let theme = THEMES[i / PER_THEME];
        let level = LEVELS[i % LEVELS.len()];
        pf.admit(day, t, theme, level, 100 + i as u32, true).expect("admit");
    }
    set.add("f145 first batch 20", pf.len() == 20, "20 articles");
    set.add(
        "f145 four themes five each",
        pf.first_batch_complete() && THEMES.iter().all(|t| pf.count_theme(t) == 5),
        "4×5 quota",
    );
    set.add(
        "f145 levels spread",
        pf.count_level("starter") > 0 && pf.count_level("advanced") > 0 && pf.count_level("deep") > 0,
        "layered curve",
    );

    // 抽查复核：一轮 20 处
    pf.audit_round(0);
    set.add("f145 audit round clean", pf.last_audit_clean(), "20 samples no hit");
    let mut dirty_pf = Portfolio::new();
    dirty_pf.audit_round(3);
    set.add("f145 audit findings block", !dirty_pf.last_audit_clean(), "findings → rework");
    set.add("f145 audit sample size", AUDIT_SAMPLES == 20, "per-version law");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_boundary() {
        let mut pf = Portfolio::new();
        for i in 0..4 {
            pf.admit(20260101, "t", "boot", "starter", 1 + i, true).unwrap();
        }
        assert!(!pf.first_batch_complete());
        pf.admit(20260101, "t5", "boot", "advanced", 9, true).unwrap();
        assert!(pf.count_theme("boot") == 5);
    }
}
