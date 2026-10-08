//! 深化层四 · F138 版本发布节奏公开（2026-09-27 深化批次四 · g 层）。
//!
//! 发布说明渲染器（分类分组 + 预算）、渠道内容矩阵、紧急窗使用审计
//! （30 天间隔 + 必须带原因）、贡献者致谢名册生成。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 发布说明渲染：条目按分类分组，组序固定 [Breaking/Security/Added/Fixed]
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoteKind {
    Breaking,
    Security,
    Added,
    Fixed,
}

/// 校验：正文非空且 ≤200 字节；渲染 = 分组标题 + 条目（组序固定）。
pub fn render_notes(entries: &[(NoteKind, &'static str)]) -> Result<alloc::vec::Vec<&'static str>, &'static str> {
    const ORDER: [NoteKind; 4] =
        [NoteKind::Breaking, NoteKind::Security, NoteKind::Added, NoteKind::Fixed];
    const TITLES: [&str; 4] = ["## 破坏性变更", "## 安全", "## 新增", "## 修复"];
    let mut out: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for (i, kind) in ORDER.iter().enumerate() {
        let group: alloc::vec::Vec<&'static str> = entries
            .iter()
            .filter(|(k, _)| k == kind)
            .map(|(_, t)| *t)
            .collect();
        if group.is_empty() {
            continue;
        }
        for t in &group {
            if t.trim().is_empty() {
                return Err("条目正文为空");
            }
            if t.len() > 200 {
                return Err("条目超 200 字节预算");
            }
        }
        out.push(TITLES[i]);
        out.extend(group);
    }
    if out.is_empty() {
        return Err("发布说明为空：不发布空说明");
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 渠道内容矩阵：公告渠道 × 内容类型白名单（未列入即不发）
// ---------------------------------------------------------------------------

pub const CH_RELEASE_NOTES: u8 = 1;
pub const CH_SECURITY: u8 = 2;
pub const CH_BLOG: u8 = 4;

/// 内容类型位图 × 渠道位图 → 允许裁决（组合查询全含才放行）。
pub fn channel_allows(channel: u8, content: u8) -> bool {
    content != 0 && channel & content == content
}

// ---------------------------------------------------------------------------
// 紧急窗使用审计：必须带原因 + 两次使用间隔 ≥30 天
// ---------------------------------------------------------------------------

pub const EMERGENCY_MIN_GAP: u32 = 30;

pub struct EmergencyAudit {
    /// (日, 原因)
    uses: alloc::vec::Vec<(u32, &'static str)>,
}

impl EmergencyAudit {
    pub fn new() -> EmergencyAudit {
        EmergencyAudit { uses: alloc::vec::Vec::new() }
    }

    pub fn use_window(&mut self, day: u32, reason: &'static str) -> Result<(), &'static str> {
        if reason.trim().is_empty() {
            return Err("紧急窗必须带原因：零静默");
        }
        if let Some((last, _)) = self.uses.last() {
            if day < *last {
                return Err("时间线倒置");
            }
            if day - last < EMERGENCY_MIN_GAP {
                return Err("紧急窗 30 天内复用：滥用拦截");
            }
        }
        self.uses.push((day, reason));
        Ok(())
    }

    pub fn uses(&self) -> usize {
        self.uses.len()
    }
}

// ---------------------------------------------------------------------------
// 致谢名册：合并 PR 的作者 → 去重计数 → 计数降序（平局名字序）
// ---------------------------------------------------------------------------

pub fn thanks_roll(authors: &[&'static str]) -> alloc::vec::Vec<(&'static str, u32)> {
    let mut agg: alloc::vec::Vec<(&'static str, u32)> = alloc::vec::Vec::new();
    for a in authors {
        match agg.iter_mut().find(|(n, _)| n == a) {
            Some(e) => e.1 += 1,
            None => agg.push((a, 1)),
        }
    }
    for i in 1..agg.len() {
        let k = agg[i];
        let mut j = i;
        while j > 0
            && (agg[j - 1].1 < k.1 || (agg[j - 1].1 == k.1 && agg[j - 1].0 > k.0))
        {
            agg[j] = agg[j - 1];
            j -= 1;
        }
        agg[j] = k;
    }
    agg
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F138G_TAG: &str = "stareco-F138-deep4";

pub fn run_f138_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F138G_TAG);

    // 发布说明
    let notes = [
        (NoteKind::Fixed, "修复窗口吸附落点"),
        (NoteKind::Breaking, "fs_open 改签名"),
        (NoteKind::Security, "TLS 库升级"),
        (NoteKind::Added, "跳转清单"),
    ];
    let rendered = render_notes(&notes).expect("ok");
    set.add(
        "f138g group order",
        rendered[0] == "## 破坏性变更" && rendered[2] == "## 安全" && rendered.len() == 8,
        "组序固定 [破坏/安全/新增/修复]",
    );
    set.add("f138g empty entry", render_notes(&[(NoteKind::Added, " ")]).is_err(), "空正文拒绝");
    set.add("f138g empty notes", render_notes(&[]).is_err(), "空说明不发布");

    // 渠道矩阵
    set.add("f138g channel ok", channel_allows(CH_RELEASE_NOTES, CH_RELEASE_NOTES), "说明渠道发说明");
    set.add(
        "f138g channel combo",
        channel_allows(CH_SECURITY | CH_BLOG, CH_SECURITY),
        "组合渠道含安全位",
    );
    set.add("f138g channel deny", !channel_allows(CH_RELEASE_NOTES, CH_SECURITY), "说明渠道不发安全公告");
    set.add("f138g channel zero", !channel_allows(CH_BLOG, 0), "零内容拒绝");

    // 紧急窗审计
    let mut ea = EmergencyAudit::new();
    set.add("f138g no reason", ea.use_window(100, "").is_err(), "无原因拒绝");
    let _ = ea.use_window(100, "0day 修复");
    set.add("f138g gap short", ea.use_window(120, "又一起").is_err(), "20 天复用拦截");
    set.add("f138g gap ok", ea.use_window(131, "证书轮换").is_ok(), "31 天放行");
    set.add("f138g count", ea.uses() == 2, "使用计数");

    // 致谢名册
    let roll = thanks_roll(&["beta", "alpha", "beta", "gamma", "beta"]);
    set.add(
        "f138g roll",
        roll == alloc::vec![("beta", 3), ("alpha", 1), ("gamma", 1)],
        "去重计数降序平局名字序",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn notes_only_breaking() {
        let r = render_notes(&[(NoteKind::Breaking, "x 改名")]).unwrap();
        assert_eq!(r, alloc::vec!["## 破坏性变更", "x 改名"]);
    }

    #[test]
    fn emergency_first_use_free() {
        let mut ea = EmergencyAudit::new();
        assert!(ea.use_window(0, "首用").is_ok());
        assert_eq!(ea.uses(), 1);
    }
}
