//! ves304_s15mt — VE-F4064 · T04 机器翻译协同
//! 引擎适配层（插件接口冻结+可插拔）+ 质量预判置信分档（高置信直通/低置信强制人工）+ 机翻标识诚实红线 + 备引擎切换显性通告 + 请求脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8940;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum MtCode {
    Ok = 0,
    EngineDown = SEG_BASE + 0x01,
    InterfaceDrift = SEG_BASE + 0x02,
    IdentityLeak = SEG_BASE + 0x03,
    NoFallback = SEG_BASE + 0x04,
    Mislabel = SEG_BASE + 0x05,
}

/// 置信分档。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence { High = 2, Medium = 1, Low = 0 }

impl Confidence {
    pub fn from_score(score: u32) -> Confidence {
        if score >= 90 { Confidence::High }
        else if score >= 60 { Confidence::Medium }
        else { Confidence::Low }
    }
    /// 高置信直通抽检 / 低置信强制人工。
    pub fn route(self) -> &'static str {
        match self {
            Confidence::High => "直通抽检",
            Confidence::Medium => "人工复核",
            Confidence::Low => "强制人工",
        }
    }
}

/// 引擎适配层（插件接口冻结）。
pub struct MtEngine {
    pub name: &'static str,
    pub alive: bool,
    pub api_ver: u32,
}

pub struct MtHub {
    pub engines: Vec<MtEngine>,
    pub active: usize,
}

impl MtHub {
    pub fn new() -> Self { MtHub { engines: Vec::new(), active: 0 } }

    pub fn register(&mut self, name: &'static str, api_ver: u32) -> Result<(), MtCode> {
        if api_ver != 1 { return Err(MtCode::InterfaceDrift); }
        self.engines.push(MtEngine { name, alive: true, api_ver });
        Ok(())
    }

    /// 翻译请求：请求语料脱敏（不含身份信息）。
    pub fn request_ok(text: &str) -> Result<(), MtCode> {
        if text.contains("user_id") || text.contains("@corp.com") {
            return Err(MtCode::IdentityLeak);
        }
        Ok(())
    }

    /// 引擎调用：不可用切备引擎+显性通告。
    pub fn translate(&mut self, text: &str, score: u32) -> Result<(String, &'static str, Confidence), MtCode> {
        Self::request_ok(text)?;
        for i in 0..self.engines.len() {
            let idx = (self.active + i) % self.engines.len();
            if self.engines[idx].alive {
                if idx != self.active {
                    self.active = idx; // 切备引擎
                }
                let conf = Confidence::from_score(score);
                return Ok((String::from(text), self.engines[idx].name, conf));
            }
        }
        Err(MtCode::EngineDown)
    }

    /// 全引擎不可用 → 回退全人工。
    pub fn kill_all(&mut self) {
        for e in self.engines.iter_mut() { e.alive = false; }
    }
    pub fn fallback_manual(&self) -> Result<&'static str, MtCode> {
        if self.engines.iter().any(|e| e.alive) { return Err(MtCode::NoFallback); }
        Ok("全人工")
    }
}

/// 机翻标识诚实红线：机翻产出必挂标识，人工不可标机翻。
pub fn label_check(is_mt: bool, labeled_mt: bool) -> Result<(), MtCode> {
    if is_mt && !labeled_mt { return Err(MtCode::Mislabel); }
    if !is_mt && labeled_mt { return Err(MtCode::Mislabel); }
    Ok(())
}

pub fn run_ves304_checks() -> CheckSet {
    let mut set = CheckSet::new("ves304_s15mt");
    let mut hub = MtHub::new();

    // 1) 引擎注册。
    let _ = hub.register("alpha", 1);
    let _ = hub.register("beta", 1);
    set.add("C4064-01", hub.engines.len() == 2, "引擎注册");

    // 2) 接口漂移拒。
    set.add("C4064-02", hub.register("gamma", 2) == Err(MtCode::InterfaceDrift), "接口漂移拒");

    // 3) 置信分档路由。
    set.add("C4064-03", Confidence::from_score(95).route() == "直通抽检"
        && Confidence::from_score(70).route() == "人工复核"
        && Confidence::from_score(30).route() == "强制人工", "置信路由");

    // 4) 请求脱敏。
    set.add("C4064-04", MtHub::request_ok("Hello world").is_ok()
        && MtHub::request_ok("user_id=42") == Err(MtCode::IdentityLeak), "请求脱敏");

    // 5) 正常翻译。
    let r = hub.translate("Hello world", 92);
    set.add("C4064-05", r.is_ok() && r.clone().unwrap().2 == Confidence::High, "正常翻译");

    // 6) 备引擎切换显性。
    hub.engines[hub.active].alive = false;
    let r2 = hub.translate("Second try", 80);
    set.add("C4064-06", r2.is_ok() && r2.unwrap().1 == "beta", "备引擎切换");

    // 7) 全灭回退全人工。
    hub.kill_all();
    set.add("C4064-07", hub.fallback_manual() == Ok("全人工"), "回退全人工");

    // 8) 无引擎翻译拒。
    let r3 = hub.translate("again", 50);
    set.add("C4064-08", r3 == Err(MtCode::EngineDown), "无引擎拒");

    // 9) 机翻标识诚实红线。
    set.add("C4064-09", label_check(true, true).is_ok() && label_check(true, false) == Err(MtCode::Mislabel)
        && label_check(false, true) == Err(MtCode::Mislabel), "机翻标识");

    // 10) 置信序。
    set.add("C4064-10", Confidence::High > Confidence::Medium && Confidence::Medium > Confidence::Low, "置信序");

    // 11) 段位自查。
    set.add("C4064-11", SEG_BASE == 0x8940, "段位 0x8940");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mt_flow() {
        let mut hub = MtHub::new();
        let _ = hub.register("a", 1);
        let r = hub.translate("hi", 95);
        assert!(r.is_ok());
        assert_eq!(r.unwrap().2, Confidence::High);
    }
}
