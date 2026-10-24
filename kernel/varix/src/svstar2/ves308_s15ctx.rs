//! ves308_s15ctx — VE-F4068 · T04 上下文注入翻译
//! 四类上下文（界面描述/长度限制/复数性别形态/出现频次）+ 上下文随键走（与 F4062 三元组单源绑定）+ 伪本地化预演 + 超限触发折行策略提示 + 上下文缺转人工。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8B40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum CtxCode {
    Ok = 0,
    ContextMissing = SEG_BASE + 0x01,
    LengthOver = SEG_BASE + 0x02,
    BindBroken = SEG_BASE + 0x03,
    PseudoFail = SEG_BASE + 0x04,
}

/// 四类上下文。
#[derive(Clone)]
pub struct Context {
    pub ui_desc: String,
    pub max_chars: u32,
    pub plural_forms: u8,
    pub gender_forms: u8,
    pub freq: u32,
}

impl Context {
    pub fn complete(&self) -> bool {
        !self.ui_desc.is_empty() && self.max_chars > 0
    }
}

/// 键-上下文绑定表（与 F4062 三元组单源）。
pub struct CtxBindBook {
    pub binds: Vec<(String, Context)>,
}

impl CtxBindBook {
    pub fn new() -> Self { CtxBindBook { binds: Vec::new() } }

    pub fn bind(&mut self, key: &str, ctx: Context) -> Result<(), CtxCode> {
        if !ctx.complete() { return Err(CtxCode::ContextMissing); }
        for b in self.binds.iter() {
            if b.0 == key {
                return Err(CtxCode::BindBroken);
            }
        }
        self.binds.push((String::from(key), ctx));
        Ok(())
    }

    pub fn ctx_of(&self, key: &str) -> Option<&Context> {
        for b in self.binds.iter() {
            if b.0 == key { return Some(&b.1); }
        }
        None
    }

    /// 长度超限检查：译文超上下文限制 → 触发 T03 折行策略提示。
    pub fn length_check(&self, key: &str, translated: &str) -> Result<&'static str, CtxCode> {
        let ctx = self.ctx_of(key).ok_or(CtxCode::BindBroken)?;
        let n = translated.chars().count() as u32;
        if n > ctx.max_chars { return Ok("触发折行策略"); }
        Ok("通过")
    }

    /// 高风险标记：上下文缺失键转人工。
    pub fn risky_keys<'a>(&self, keys: &[&'a str]) -> Vec<&'a str> {
        let mut v = Vec::new();
        for k in keys.iter() {
            if self.ctx_of(k).is_none() { v.push(*k); }
        }
        v
    }

    /// 重抽取：绑定断的键清单。
    pub fn reextract(&self, valid_keys: &[&str]) -> Vec<String> {
        let mut v = Vec::new();
        for b in self.binds.iter() {
            if !valid_keys.contains(&b.0.as_str()) { v.push(b.0.clone()); }
        }
        v
    }
}

/// 伪本地化：把 ASCII 字母映射为带变音符形态以预演。
pub fn pseudo_localize(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            'a' => out.push('ä'),
            'e' => out.push('ë'),
            'o' => out.push('ö'),
            'u' => out.push('ü'),
            other => out.push(other),
        }
    }
    out
}

/// 伪本地化有效性预演：变形后长度膨胀不超 1.5 倍上下文。
pub fn pseudo_valid(ctx_max: u32, pseudo_len: u32) -> Result<(), CtxCode> {
    if pseudo_len * 2 > ctx_max * 3 { return Err(CtxCode::PseudoFail); }
    Ok(())
}

pub fn run_ves308_checks() -> CheckSet {
    let mut set = CheckSet::new("ves308_s15ctx");
    let mut cb = CtxBindBook::new();

    // 1) 上下文缺拒。
    set.add("C4068-01", cb.bind("k1", Context { ui_desc: String::new(), max_chars: 20, plural_forms: 2, gender_forms: 0, freq: 3 }) == Err(CtxCode::ContextMissing), "上下文缺拒");

    // 2) 正常绑定。
    let _ = cb.bind("k1", Context { ui_desc: String::from("主对话框标题"), max_chars: 20, plural_forms: 2, gender_forms: 0, freq: 3 });
    set.add("C4068-02", cb.ctx_of("k1").is_some(), "正常绑定");

    // 3) 重复绑定拒。
    set.add("C4068-03", cb.bind("k1", Context { ui_desc: String::from("x"), max_chars: 5, plural_forms: 1, gender_forms: 0, freq: 1 }) == Err(CtxCode::BindBroken), "重复绑定拒");

    // 4) 长度超限提示。
    set.add("C4068-04", cb.length_check("k1", "这个译文实在太长太长太长太长太长了") == Ok("触发折行策略"), "超限提示");

    // 5) 长度合规。
    set.add("C4068-05", cb.length_check("k1", "确定") == Ok("通过"), "长度合规");

    // 6) 绑定断检出。
    set.add("C4068-06", cb.length_check("ghost", "x") == Err(CtxCode::BindBroken), "绑定断检出");

    // 7) 高风险键转人工。
    let risky = cb.risky_keys(&["k1", "k2"]);
    set.add("C4068-07", risky == ["k2"], "高风险转人工");

    // 8) 重抽取清单：valid 缺 k1 时应报 k1。
    let re = cb.reextract(&["other"]);
    set.add("C4068-08", re == ["k1"], "重抽取清单");

    // 9) 伪本地化。
    set.add("C4068-09", pseudo_localize("apple") == "äpplë", "伪本地化");

    // 10) 伪本地化有效性。
    set.add("C4068-10", pseudo_valid(20, 12).is_ok() && pseudo_valid(10, 30) == Err(CtxCode::PseudoFail), "伪本地化预演");

    // 11) 段位自查。
    set.add("C4068-11", SEG_BASE == 0x8B40, "段位 0x8B40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_flow() {
        assert_eq!(pseudo_localize("aeou"), "äëöü");
        let mut cb = CtxBindBook::new();
        let _ = cb.bind("k", Context { ui_desc: String::from("d"), max_chars: 10, plural_forms: 1, gender_forms: 0, freq: 1 });
        assert!(cb.length_check("k", "短").is_ok());
    }
}
