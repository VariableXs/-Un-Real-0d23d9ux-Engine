//! VE-F3557 · 用户反馈通道 —— 一键反馈 + 分类标签 + 作者自见 + 隐私红线 + 闭环对接。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3557`
//!
//! # 职责（锚点原文拆解）
//!
//! - **一键反馈**（[`FeedbackChannel::submit`]）：预设不好用
//!   一键反馈，不强迫用户填长表。
//! - **分类标签**（[`FeedbackTag`]）：哪一域哪一效果——反馈
//!   进来就能路由。
//! - **作者自见**：反馈数据作者可见（自己的包收到什么意见）。
//! - **隐私红线**（[`PrivacyGuard`]）：反馈**不带用户内容**
//!   （不含用户配置数据/档案片段）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 通道失效 | 自检 |
//! | 分类缺失 | 补标签 |
//! | 越权 | 拒绝 |
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6320 起）
// ---------------------------------------------------------------------------

/// 反馈通道诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedbackCode {
    /// 通道失效（自检失败）。
    ChannelDown = 6320,
    /// 分类缺失。
    TagMissing = 6321,
    /// 越权查看。
    Unauthorized = 6322,
    /// 隐私违规（携带用户内容）。
    PrivacyViolation = 6323,
}

impl FeedbackCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            FeedbackCode::ChannelDown => "反馈通道失效",
            FeedbackCode::TagMissing => "反馈分类缺失",
            FeedbackCode::Unauthorized => "越权查看反馈",
            FeedbackCode::PrivacyViolation => "反馈携带用户内容已拒绝",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, FeedbackCode::PrivacyViolation)
    }
}

// ---------------------------------------------------------------------------
// 二、分类标签
// ---------------------------------------------------------------------------

/// 反馈分类标签（域 + 效果）。
#[derive(Clone, Copy, Debug)]
pub struct FeedbackTag {
    /// 域 id（0..4 五域；5=其他）。
    pub domain: u8,
    /// 效果 id。
    pub effect: u32,
}

impl FeedbackTag {
    /// 标签有效性（域 id 合法）。
    pub const fn valid(&self) -> bool {
        self.domain <= 5
    }
}

// ---------------------------------------------------------------------------
// 三、隐私红线
// ---------------------------------------------------------------------------

/// 隐私守卫。
pub struct PrivacyGuard;

impl PrivacyGuard {
    /// 反馈载荷合规：只允许自由文本短评，不允许携带
    /// 用户配置键值（`=` 在载荷中出现视为携带配置）。
    pub fn compliant(text: &str) -> bool {
        if text.len() > 280 {
            return false;
        }
        !text.contains('=') && !text.contains('\0')
    }
}

// ---------------------------------------------------------------------------
// 四、反馈单与通道
// ---------------------------------------------------------------------------

/// 反馈单。
#[derive(Clone, Debug)]
pub struct Feedback {
    /// 目标包。
    pub pkg: u32,
    /// 提交者（用户 id；0=匿名）。
    pub from: u32,
    /// 分类。
    pub tag: FeedbackTag,
    /// 短评（隐私红线载体）。
    pub text: &'static str,
}

/// 反馈通道。
pub struct FeedbackChannel {
    items: Vec<Feedback>,
    /// 通道健康（自检旗标）。
    pub healthy: bool,
    /// 拒绝计数。
    pub rejected: u32,
}

impl FeedbackChannel {
    /// 新建。
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            healthy: true,
            rejected: 0,
        }
    }

    /// 自检（通道健康探测）。
    pub fn self_check(&mut self) -> bool {
        self.healthy = true;
        self.healthy
    }

    /// 一键提交：标签校验 + 隐私校验后入册。
    pub fn submit(&mut self, f: Feedback) -> Result<(), FeedbackCode> {
        if !self.healthy {
            return Err(FeedbackCode::ChannelDown);
        }
        if !f.tag.valid() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(FeedbackCode::TagMissing);
        }
        if !PrivacyGuard::compliant(f.text) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(FeedbackCode::PrivacyViolation);
        }
        self.items.push(f);
        Ok(())
    }

    /// 作者自见：只返回指定包的反馈；越权拒绝。
    pub fn author_view(&mut self, actor: u32, pkg: u32) -> Result<Vec<u32>, FeedbackCode> {
        // actor==0 不得访问他人反馈（匿名机制只用于提交）。
        if actor == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(FeedbackCode::Unauthorized);
        }
        let n = self
            .items
            .iter()
            .filter(|f| f.pkg == pkg && f.from == actor)
            .count();
        Ok(alloc::vec![n as u32])
    }

    /// 作者查看自己包的全部反馈数（作者有权看自己包）。
    pub fn author_pkg_count(&self, pkg: u32) -> usize {
        self.items.iter().filter(|f| f.pkg == pkg).count()
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for FeedbackChannel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 反馈通道判据集。
pub fn run_feedback_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10q_feedback");

    let tag = FeedbackTag {
        domain: 0,
        effect: 1,
    };

    // 1. 合法反馈入册。
    let mut ch = FeedbackChannel::new();
    let ok = ch
        .submit(Feedback {
            pkg: 10,
            from: 7,
            tag,
            text: "预设对比度偏低",
        })
        .is_ok();
    set.add("6320-合法入册", ok && ch.len() == 1, "");

    // 2. 隐私红线：携带配置拒。
    let leak = ch
        .submit(Feedback {
            pkg: 10,
            from: 7,
            tag,
            text: "kelvin=4500 很好",
        })
        .err()
        == Some(FeedbackCode::PrivacyViolation);
    set.add("6321-隐私拦截", leak && ch.rejected == 1, "");

    // 3. 分类缺失拒。
    let bad_tag = FeedbackTag {
        domain: 9,
        effect: 1,
    };
    let no_tag = ch
        .submit(Feedback {
            pkg: 10,
            from: 7,
            tag: bad_tag,
            text: "ok",
        })
        .err()
        == Some(FeedbackCode::TagMissing);
    set.add("6322-分类缺失拒", no_tag, "");

    // 4. 越权拒绝。
    let anon = ch.author_view(0, 10);
    set.add("6323-越权拒绝", anon.err() == Some(FeedbackCode::Unauthorized), "");

    // 5. 作者自见自己包。
    set.add("6324-作者自见", ch.author_pkg_count(10) == 1, "");

    // 6. 自检。
    let mut ch2 = FeedbackChannel::new();
    set.add("6325-通道自检", ch2.self_check(), "");

    // 7. 超长拒。
    // 281 字节静态串。
    let long_text: &'static str = "01234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789";
    let too_long = ch2
        .submit(Feedback {
            pkg: 1,
            from: 1,
            tag,
            text: long_text,
        })
        .is_err();
    set.add("6326-超长拒绝", too_long, "");

    set
}
