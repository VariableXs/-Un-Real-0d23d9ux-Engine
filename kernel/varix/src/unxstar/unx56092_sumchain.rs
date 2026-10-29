//! UNX-F56092 采集数据完整性校验器（校验和链）（0x60B60 段）
//!
//! 锚点：UNX-F56092-J1 校验和链全量（采集批逐批落账：批键 × 数据指纹 ×
//! 链前指纹三元组）；J2 链完整性核（逐批指纹复算一致且链前指针闭合，
//! 抽改任一批全链报警）；J3 校验审计链（批序单调、链尾封印、封账只读）
//! 三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·完整性面）。
//!
//! 结构：采集批校验链（批键 → 数据指纹 → 链前指纹逐批落账）+ 指纹复算
//! 核（FNV-1a 逐批重算）+ 抽改报警核（任一批指纹与链不符即全链不完整）
//! + 链尾封印语义（封账时链尾固化可复验）+ 注入核（空批键/指纹复算
//! 漂移/链前错位/重复批键/封账改批/空链验链/链尾漂移必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60B60 独占 0x20）。
pub const SEG_BASE: u32 = 0x60B60;
pub const IC_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 批键为空
pub const IC_FP_MISS: u32 = SEG_BASE + 0x02; // 指纹复算漂移
pub const IC_CHAIN_BREAK: u32 = SEG_BASE + 0x03; // 链前错位（链断裂）
pub const IC_DUP: u32 = SEG_BASE + 0x04; // 重复批键
pub const IC_SEALED: u32 = SEG_BASE + 0x05; // 校验账已封账拒批
pub const IC_EMPTY: u32 = SEG_BASE + 0x06; // 空链验链
pub const IC_TAIL_MISS: u32 = SEG_BASE + 0x07; // 链尾封印漂移

/// 数据指纹：FNV-1a 32 位（对数据字节流）。
pub fn data_fingerprint(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 链前指纹合成：前批链值 XOR 本批数据指纹。
pub fn chain_mix(prev_chain: u32, fp: u32) -> u32 {
    prev_chain ^ fp
}

/// 链种子（首批链前值）。
pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 校验批：批键 × 数据指纹 × 链前指纹 × 链值。
#[derive(Clone)]
pub struct IntegBatch {
    pub key: String,
    pub fp: u32,
    pub prev_chain: u32,
    pub chain: u32,
}

/// 校验总账。
pub struct IntegBook {
    pub batches: Vec<IntegBatch>,
    pub sealed: bool,
    /// 封账时固化的链尾（封印）。
    pub tail_seal: Option<u32>,
}

impl IntegBook {
    pub fn new() -> Self {
        IntegBook { batches: Vec::new(), sealed: false, tail_seal: None }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.batches.len() {
            if self.batches[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 逐批落链：空键拒 + 重复拒 + 指纹复算三闸，链前自动接续。
    pub fn append(&mut self, key: &str, data: &[u8]) -> Result<u32, u32> {
        if self.sealed {
            return Err(IC_SEALED);
        }
        if key.is_empty() {
            return Err(IC_KEY_EMPTY);
        }
        if self.find(key).is_some() {
            return Err(IC_DUP);
        }
        let fp = data_fingerprint(data);
        let prev_chain = match self.batches.last() {
            None => CHAIN_SEED,
            Some(b) => b.chain,
        };
        let chain = chain_mix(prev_chain, fp);
        self.batches.push(IntegBatch { key: key.to_string(), fp, prev_chain, chain });
        Ok(chain)
    }

    /// J2：全链完整性核（逐批复算指纹 + 链前闭合 + 链值重推）。
    pub fn verify(&self, data_of: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<bool, u32> {
        if self.batches.is_empty() {
            return Err(IC_EMPTY);
        }
        let mut expect_chain = CHAIN_SEED;
        for b in self.batches.iter() {
            let data = match data_of(&b.key) {
                Some(d) => d,
                None => return Ok(false),
            };
            if data_fingerprint(&data) != b.fp {
                return Ok(false);
            }
            if b.prev_chain != expect_chain {
                return Ok(false);
            }
            expect_chain = chain_mix(expect_chain, b.fp);
            if b.chain != expect_chain {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// J2：抽改报警核（任一批指纹与登记不符即 false）。
    pub fn tamper_alarm(&self, data_of: &dyn Fn(&str) -> Option<Vec<u8>>) -> bool {
        match self.verify(data_of) {
            Ok(true) => false,
            _ => true,
        }
    }

    /// J3：链尾封印（封账固化；封后链尾可复验）。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(IC_SEALED);
        }
        if self.batches.is_empty() {
            return Err(IC_EMPTY);
        }
        let tail = self.batches.last().unwrap_or_else(|| panic!("tail must exist")).chain;
        self.tail_seal = Some(tail);
        self.sealed = true;
        Ok(tail)
    }

    /// 封印复验（未封账或漂移显性拒绝）。
    pub fn seal_verify(&self) -> Result<bool, u32> {
        match self.tail_seal {
            None => Err(IC_TAIL_MISS),
            Some(t) => Ok(self.batches.last().map(|b| b.chain == t).unwrap_or(false)),
        }
    }

    /// J1：校验链全量（逐批键非空 + 指纹非零）。
    pub fn batches_ok(&self) -> bool {
        !self.batches.is_empty()
            && self.batches.iter().all(|b| !b.key.is_empty() && b.fp != 0)
    }
}

/// 标准夹具：3 批采集数据（固定字节流）。
pub fn standard_book() -> IntegBook {
    let mut b = IntegBook::new();
    let plans: [(&str, &[u8]); 3] = [
        ("mem|wave1|rss", b"rss-samples-batch-1"),
        ("mem|wave1|avail", b"avail-samples-batch-2"),
        ("mem|wave1|cached", b"cached-samples-batch-3"),
    ];
    for p in plans.iter() {
        if b.append(p.0, p.1).is_err() {
            panic!("standard fixture append must succeed");
        }
    }
    b
}

/// 夹具数据回放器（与夹具同源字节流）。
pub fn fixture_data(key: &str) -> Option<Vec<u8>> {
    match key {
        "mem|wave1|rss" => Some(b"rss-samples-batch-1".to_vec()),
        "mem|wave1|avail" => Some(b"avail-samples-batch-2".to_vec()),
        "mem|wave1|cached" => Some(b"cached-samples-batch-3".to_vec()),
        _ => None,
    }
}

/// 注入核：7 注入必抓（空批键/重复批键/数据抽改报警/封后改批/空链验链/封印漂移/回放缺数据）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空批键
    let mut b = IntegBook::new();
    if b.append("", b"x") == Err(IC_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 重复批键
    if b.append("k", b"x").is_ok() && b.append("k", b"y") == Err(IC_DUP) {
        caught += 1;
    }
    // 3) 数据抽改报警
    let mut dirty = standard_book();
    dirty.batches[1].fp ^= 1;
    if dirty.tamper_alarm(&fixture_data) {
        caught += 1;
    }
    // 4) 回放缺数据报警
    if standard_book().tamper_alarm(&|_k| None) {
        caught += 1;
    }
    // 5) 空链验链
    if IntegBook::new().verify(&fixture_data) == Err(IC_EMPTY) {
        caught += 1;
    }
    // 6) 封印漂移必抓（封后动链尾 → 复验失败）
    let mut sb = standard_book();
    let _ = sb.seal();
    sb.batches[2].chain ^= 0xFF;
    if sb.seal_verify() == Ok(false) {
        caught += 1;
    }
    // 7) 封后改批拒
    if sb.append("late", b"z") == Err(IC_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 校验账渲染导出（逐批一行）。
pub fn render_book(b: &IntegBook) -> String {
    let mut out = String::new();
    for x in b.batches.iter() {
        out.push_str(&format!("{}|{:08x}|{:08x}\n", x.key, x.fp, x.chain));
    }
    if let Some(t) = b.tail_seal {
        out.push_str(&format!("SEAL|{:08x}\n", t));
    }
    out
}

/// 渲染完整性核：逐批三段 + 封印行两段。
pub fn render_complete(text: &str, sealed: bool) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return false;
    }
    let n = lines.len();
    for (i, ln) in lines.iter().enumerate() {
        let parts: Vec<&str> = ln.split('|').collect();
        if i + 1 == n && sealed {
            if parts.len() != 2 || !lines[i].starts_with("SEAL|") {
                return false;
            }
        } else if parts.len() != 3 {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56092_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56092");
    // J1-1 校验链全量（三批逐批合法）
    let b = standard_book();
    let ok = b.batches_ok() && b.batches.len() == 3;
    s.add("56092-J1-校验链全量", ok, if ok { "" } else { "校验批残缺" });
    // J1-2 指纹复算一致（夹具回放逐批一致）
    let ok = b.verify(&fixture_data) == Ok(true);
    s.add("56092-J1-指纹复算一致", ok, if ok { "" } else { "指纹漂移" });
    // J1-3 指纹确定性（同数据同指纹、异数据异指纹）
    let ok = data_fingerprint(b"abc") == data_fingerprint(b"abc")
        && data_fingerprint(b"abc") != data_fingerprint(b"abd");
    s.add("56092-J1-指纹确定性", ok, if ok { "" } else { "指纹碰撞" });
    // J2-1 全链闭合（链值重推一致）
    let mut expect = CHAIN_SEED;
    for x in b.batches.iter() {
        expect = chain_mix(expect, x.fp);
    }
    let ok = expect == b.batches[2].chain;
    s.add("56092-J2-全链闭合", ok, if ok { "" } else { "链闭合失败" });
    // J2-2 抽改报警（改一 批指纹全链报警）
    let mut dirty = standard_book();
    dirty.batches[0].fp ^= 0xFFFF;
    let ok = dirty.tamper_alarm(&fixture_data);
    s.add("56092-J2-抽改报警", ok, if ok { "" } else { "报警失效" });
    // J2-3 回放一致不报警
    let ok = !standard_book().tamper_alarm(&fixture_data);
    s.add("56092-J2-回放一致", ok, if ok { "" } else { "误报警" });
    // J2-4 链前错位必抓（动链前指纹）
    let mut b2 = standard_book();
    b2.batches[1].prev_chain ^= 1;
    let ok = b2.verify(&fixture_data) == Ok(false);
    s.add("56092-J2-链前错位必抓", ok, if ok { "" } else { "错位漏抓" });
    // J3-1 链尾封印语义（封账固化 + 复验通过）
    let mut b3 = standard_book();
    let t = b3.seal();
    let ok = match t {
        Ok(t) => b3.sealed && b3.seal_verify() == Ok(true) && t == b3.batches[2].chain,
        Err(_) => false,
    };
    s.add("56092-J3-链尾封印", ok, if ok { "" } else { "封印语义漂移" });
    // J3-2 封印稳定（同夹具两次封印同值）
    let t1 = {
        let mut t = standard_book();
        t.seal().unwrap_or(0)
    };
    let t2 = {
        let mut t = standard_book();
        t.seal().unwrap_or(0)
    };
    let ok = t1 == t2 && t1 != 0;
    s.add("56092-J3-封印稳定", ok, if ok { "" } else { "封印漂移" });
    // J3-3 封后改批拒
    let ok = b3.append("late", b"z") == Err(IC_SEALED);
    s.add("56092-J3-封后改批拒", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出齐（三批 + 封印行）
    let text = render_book(&b3);
    let ok = render_complete(&text, true) && text.contains("SEAL|");
    s.add("56092-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56092-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56092_all_checks_green() {
        let set = run_unx56092_checks();
        assert!(set.all_passed(), "UNX-F56092 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56092_seal_stable() {
        let mut a = standard_book();
        let mut c = standard_book();
        assert_eq!(a.seal(), c.seal());
    }
}
