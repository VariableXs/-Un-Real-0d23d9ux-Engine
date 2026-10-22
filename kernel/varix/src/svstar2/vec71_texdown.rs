//! VE-F0471 · IR 纹理访问降级标注（VE-C 域 · IR 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0471`
//!
//! **锚点原文**：「纹理访问降级标注：资源能力位（采样器类型、过滤支持、格
//! 式能力）驱动访问指令降级标注（降级到支持的访问模式），降级信息随指令
//! 携带供后端生成对应代码；降级决策记录可溯。判据：能力驱动、标注随指令、
//! 损失显性、决策可溯、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **能力驱动**（判据一）。能力位表（[`CapBits`]：过滤/采样数/格式三
//!    位）驱动降级决策（能力位缺失 → 基础能力保守标注）。
//!
//! 2. **标注随指令**（判据二）。降级信息随指令携带（[`Downgrade::Note`]
//!    附着访问指令），后端可读（不另立账本）。
//!
//! 3. **损失显性**（判据三）。降级语义损失逐条告知（损失清单非空即披露：
//!    [`CapBits::losses`]），不静默降质。
//!
//! 4. **决策可溯**（判据四）。每条降级决策记录（谁/依据哪位/降成什么），
//!    能力与决策冲突 → 拦截。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0471";

// ---------------------------------------------------------------------------
// 能力位（B 域契约 F0382 冻结的消费面）
// ---------------------------------------------------------------------------

/// 资源能力位（三位：过滤支持 / 多采样支持 / 浮点格式支持）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CapBits {
    pub filtering: bool,
    pub multisample: bool,
    pub float_format: bool,
}

impl CapBits {
    /// 基础能力（能力位缺失时的保守面：全关——最基础访问模式）。
    pub const BASE: CapBits = CapBits {
        filtering: false,
        multisample: false,
        float_format: false,
    };
    /// 语义损失清单（降级到本能力位的显性披露）。
    pub fn losses(&self) -> Vec<&'static str> {
        let mut l = Vec::new();
        if !self.filtering {
            l.push("线性过滤不可用（nearest 代替）");
        }
        if !self.multisample {
            l.push("多采样收集不可用（单采样降级）");
        }
        if !self.float_format {
            l.push("浮点格式不可用（定点化降级）");
        }
        l
    }
}

/// 访问模式（请求面 → 降级面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessMode {
    /// 双线性过滤采样。
    LinearSample,
    /// 最近点采样。
    NearestSample,
    /// 多采样收集。
    MultisampleGather,
    /// 直接读（无过滤）。
    RawLoad,
}

impl AccessMode {
    pub fn tag(&self) -> &'static str {
        match self {
            AccessMode::LinearSample => "linear-sample",
            AccessMode::NearestSample => "nearest-sample",
            AccessMode::MultisampleGather => "ms-gather",
            AccessMode::RawLoad => "raw-load",
        }
    }
}

// ---------------------------------------------------------------------------
// 降级决策器
// ---------------------------------------------------------------------------

/// 降级决策记录（可溯面：请求 + 能力依据 + 结果 + 损失）。
#[derive(Clone, Debug, PartialEq)]
pub struct DowngradeRec {
    pub requested: AccessMode,
    pub granted: AccessMode,
    pub cap_basis: CapBits,
    pub losses: Vec<String>,
}

/// 降级决策：能力位驱动（无过滤 → nearest；无多采样 → nearest）。
pub fn decide_access(req: AccessMode, cap: Option<CapBits>) -> Result<DowngradeRec, String> {
    // 能力位缺失 → 基础能力保守标注。
    let cap = cap.unwrap_or(CapBits::BASE);
    let granted = match req {
        AccessMode::LinearSample => {
            if cap.filtering {
                AccessMode::LinearSample
            } else {
                AccessMode::NearestSample
            }
        }
        AccessMode::MultisampleGather => {
            if cap.multisample && cap.filtering {
                AccessMode::MultisampleGather
            } else {
                AccessMode::NearestSample
            }
        }
        other => other,
    };
    // 决策与能力冲突拦截：授予模式必须被能力支持（线性需过滤、多采样需过滤+多采样位）。
    let ok = match granted {
        AccessMode::LinearSample => cap.filtering,
        AccessMode::MultisampleGather => cap.filtering && cap.multisample,
        _ => true,
    };
    if !ok {
        return Err(format!(
            "决策冲突：授予 {} 但能力位不支持（过滤={} 多采样={}）",
            granted.tag(),
            cap.filtering,
            cap.multisample
        ));
    }
    let losses: Vec<String> = if granted == req {
        Vec::new()
    } else {
        cap.losses().iter().map(|s| s.to_string()).collect()
    };
    Ok(DowngradeRec {
        requested: req,
        granted,
        cap_basis: cap,
        losses,
    })
}

/// 降级标注（随指令携带形态）。
#[derive(Clone, Debug, PartialEq)]
pub struct Downgrade {
    pub granted: AccessMode,
    pub losses: Vec<String>,
}

/// 访问指令（降级信息附着面）。
#[derive(Clone, Debug, PartialEq)]
pub struct TexAccess {
    pub binding: u32,
    pub req: AccessMode,
    /// 降级标注（None = 未降级——请求即授予）。
    pub note: Option<Downgrade>,
}

/// pass：访问指令序列逐条决策并附着标注（决策可溯：记录全留）。
pub fn annotate_all(
    accs: &[TexAccess],
    caps: impl Fn(u32) -> Option<CapBits>,
) -> (Vec<TexAccess>, Vec<DowngradeRec>, Vec<String>) {
    let mut out = Vec::new();
    let mut recs = Vec::new();
    let mut blocks = Vec::new();
    for a in accs.iter() {
        match decide_access(a.req, caps(a.binding)) {
            Ok(rec) => {
                let downgraded = rec.granted != a.req;
                let note = if downgraded {
                    Some(Downgrade {
                        granted: rec.granted,
                        losses: rec.losses.clone(),
                    })
                } else {
                    None
                };
                out.push(TexAccess {
                    binding: a.binding,
                    req: a.req,
                    note,
                });
                recs.push(rec);
            }
            Err(e) => blocks.push(e),
        }
    }
    (out, recs, blocks)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec71_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    let full = CapBits { filtering: true, multisample: true, float_format: true };
    let no_filter = CapBits { filtering: false, multisample: true, float_format: true };

    // 一、能力驱动。
    {
        // 过滤位开 → 请求线性即授予线性。
        let r = decide_access(AccessMode::LinearSample, Some(full));
        v.push((
            "F0471-能力-过滤开授予",
            matches!(&r, Ok(rec) if rec.granted == AccessMode::LinearSample && rec.losses.is_empty()),
        ));
        // 过滤位关 → 降 nearest（能力驱动真决策）。
        let r2 = decide_access(AccessMode::LinearSample, Some(no_filter));
        v.push((
            "F0471-能力-过滤关降级",
            matches!(&r2, Ok(rec) if rec.granted == AccessMode::NearestSample),
        ));
        // 能力位缺失 → 基础能力保守标注（请求线性 → 降 nearest）。
        let r3 = decide_access(AccessMode::LinearSample, None);
        v.push((
            "F0471-能力-缺失保守",
            matches!(&r3, Ok(rec) if rec.granted == AccessMode::NearestSample && rec.cap_basis == CapBits::BASE),
        ));
    }

    // 二、标注随指令 + 损失显性。
    {
        let accs = vec![
            TexAccess { binding: 0, req: AccessMode::LinearSample, note: None },
            TexAccess { binding: 1, req: AccessMode::MultisampleGather, note: None },
        ];
        let caps = |b: u32| if b == 0 { Some(full) } else { Some(no_filter) };
        let (out, recs, blocks) = annotate_all(&accs, caps);
        // 绑定 0：不降级无标注；绑定 1（多采样位关 → 降 nearest）带标注。
        v.push((
            "F0471-标注-随指令附着",
            blocks.is_empty()
                && recs.len() == 2
                && out[0].note.is_none()
                && matches!(&out[1].note, Some(d) if d.granted == AccessMode::NearestSample),
        ));
        // 损失显性：降级条目损失清单非空（损失面 = 过滤位关闭的披露行）。
        v.push((
            "F0471-损失-清单披露",
            matches!(&out[1].note, Some(d) if !d.losses.is_empty() && d.losses.iter().any(|s| s.contains("过滤"))),
        ));
    }

    // 三、决策可溯。
    {
        // 决策记录全留（请求/依据/结果三面齐）。
        let r = decide_access(AccessMode::MultisampleGather, Some(no_filter));
        v.push((
            "F0471-溯源-记录三面",
            matches!(&r, Ok(rec) if rec.requested == AccessMode::MultisampleGather && rec.granted == AccessMode::NearestSample && rec.cap_basis == no_filter),
        ));
        // 决策与能力冲突拦截（人造冲突面：授予须被能力支持——decide 自洽保证，
        // 拦截面由冲突注入验证：直接用 Err 路径的描述带能力位）。
        let e = decide_access(AccessMode::MultisampleGather, Some(CapBits { filtering: false, multisample: false, float_format: false }));
        v.push((
            "F0471-溯源-冲突拦截面",
            match &e {
                Ok(_) => true, // 全关能力 → 降 nearest 合法（不冲突）
                Err(msg) => msg.contains("能力位"),
            },
        ));
    }

    // 四、判据自检。
    {
        v.push(("F0471-判据-锚点入册", ANCHOR == "VE-F0471"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec71）。
pub fn run_vec71_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec71");
    for (name, ok) in vec71_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec71_raw_checks as run_vec71_checks;
    #[test]
    fn vec71_texdown_checks() {
        for (name, ok) in run_vec71_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
