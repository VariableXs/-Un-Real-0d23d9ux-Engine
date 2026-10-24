//! ves382_s21autotest — VE-F4142 · i18n 自动化测试
//! 测试栈三层（单测/集成/端到端）+ 断言库四类断言可复用（占位符/长度/方向/字形）+ 读屏文本断言（域本色）
//! + 并行分片执行（分片失衡→重分）+ 断言库版本漂移→对版 + 层间失配→对拍。语料合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDD40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtCode {
    Ok = 0,
    VersionDrift = SEG_BASE + 0x01,
    ShardImbalance = SEG_BASE + 0x02,
    LayerMismatch = SEG_BASE + 0x03,
}

/// 测试栈三层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TestLayer { Unit = 0, Integration = 1, E2E = 2 }

impl TestLayer {
    pub const ALL: [TestLayer; 3] = [TestLayer::Unit, TestLayer::Integration, TestLayer::E2E];
    pub fn name(self) -> &'static str {
        match self {
            TestLayer::Unit => "unit", TestLayer::Integration => "integration", TestLayer::E2E => "e2e",
        }
    }
}

/// 断言库四类 + 读屏（域本色第五类，归入库但单列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertKind {
    Placeholder = 0, Length = 1, Direction = 2, Glyph = 3, ScreenReader = 4,
}

impl AssertKind {
    /// 四类核心断言（可复用库本体）。
    pub const CORE: [AssertKind; 4] = [
        AssertKind::Placeholder, AssertKind::Length, AssertKind::Direction, AssertKind::Glyph];
    pub fn name(self) -> &'static str {
        match self {
            AssertKind::Placeholder => "placeholder", AssertKind::Length => "length",
            AssertKind::Direction => "direction", AssertKind::Glyph => "glyph",
            AssertKind::ScreenReader => "screen-reader",
        }
    }
}

/// 断言库：四类可复用 + 版本号（对版用）。
pub struct AssertLib {
    pub version: u32,
    pub kinds: Vec<AssertKind>,
}

impl AssertLib {
    pub fn new(version: u32) -> Self {
        AssertLib { version, kinds: AssertKind::CORE.iter().copied().collect() }
    }

    /// 挂读屏文本断言（域本色）。
    pub fn with_screen_reader(mut self) -> Self {
        if !self.kinds.contains(&AssertKind::ScreenReader) { self.kinds.push(AssertKind::ScreenReader); }
        self
    }

    /// 版本对版：漂移即报。
    pub fn check_version(&self, expect: u32) -> Result<(), AtCode> {
        if self.version == expect { Ok(()) } else { Err(AtCode::VersionDrift) }
    }

    pub fn has(&self, k: AssertKind) -> bool { self.kinds.contains(&k) }
}

/// 测试栈：三层登记 + 层间对拍。
pub struct TestStack {
    pub layers: Vec<(TestLayer, u32)>, // (层, 用例数)
}

impl TestStack {
    pub fn new() -> Self { TestStack { layers: Vec::new() } }

    pub fn put(&mut self, l: TestLayer, n: u32) {
        match self.layers.iter_mut().find(|x| x.0 == l) {
            Some(x) => x.1 = n,
            None => self.layers.push((l, n)),
        }
    }

    pub fn count(&self, l: TestLayer) -> u32 {
        self.layers.iter().find(|x| x.0 == l).map(|x| x.1).unwrap_or(0)
    }

    pub fn complete(&self) -> bool { TestLayer::ALL.iter().all(|l| self.count(*l) > 0) }

    /// 层间对拍：集成数 ≥ 单测涉及链路抽样、端到端 ≥ 1 才算不失配（简化为递减性质单测>0 且 e2e>0）。
    pub fn layer_sane(&self) -> bool { self.complete() }
}

/// 并行分片器：均衡分片（失衡→重分）。
pub struct Sharder {
    pub shards: Vec<u32>, // 每分片用例数
}

impl Sharder {
    pub fn split(total: u32, n: u32) -> Self {
        let base = total / n;
        let rem = total % n;
        Sharder { shards: (0..n).map(|i| base + if i < rem { 1 } else { 0 }).collect() }
    }

    pub fn total(&self) -> u32 { self.shards.iter().sum() }

    /// 失衡判定：max - min > 1 即失衡（分片应尽量均匀）。
    pub fn imbalanced(&self) -> bool {
        match (self.shards.iter().max(), self.shards.iter().min()) {
            (Some(&mx), Some(&mn)) => mx.saturating_sub(mn) > 1,
            _ => false,
        }
    }

    /// 重分：按最大分片承载量重新切分（保持总量）。
    pub fn resplit(&mut self, cap: u32) -> bool {
        if cap == 0 || self.shards.is_empty() { return false; }
        let total = self.total();
        let n = (total + cap - 1) / cap;
        *self = Sharder::split(total, n);
        true
    }
}

/// 集成测试用例（切换/打包/驻留全链最小驱动）。
pub fn integration_case(locale: &str, packed: bool, resident_ok: bool) -> bool {
    !locale.is_empty() && packed && resident_ok
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves382_checks() -> CheckSet {
    let mut set = CheckSet::new("ves382");

    // C382-01 三层栈建齐。
    let mut st = TestStack::new();
    st.put(TestLayer::Unit, 120);
    st.put(TestLayer::Integration, 18);
    st.put(TestLayer::E2E, 6);
    chk(&mut set, "C382-01", st.complete() && st.layer_sane(), "三层齐备");

    // C382-02 四类核心断言库可复用。
    let lib = AssertLib::new(7);
    chk(&mut set, "C382-02",
        AssertKind::CORE.iter().all(|k| lib.has(*k)) && lib.kinds.len() == 4, "四类断言齐");

    // C382-03 读屏文本断言（域本色）挂载。
    let lib2 = lib.with_screen_reader();
    chk(&mut set, "C382-03", lib2.has(AssertKind::ScreenReader) && lib2.kinds.len() == 5, "读屏断言挂载");

    // C382-04 版本对版：一致过 / 漂移报。
    chk(&mut set, "C382-04", lib2.check_version(7).is_ok()
        && lib2.check_version(8) == Err(AtCode::VersionDrift), "对版 7 过漂 8 报");

    // C382-05 分片均衡：13 例 4 片 → 4/3/3/3。
    let sh = Sharder::split(13, 4);
    chk(&mut set, "C382-05", !sh.imbalanced() && sh.total() == 13 && sh.shards == vec![4, 3, 3, 3], "分片均衡");

    // C382-06 失衡检出与重分：cap=2 → 7 片。
    let mut bad = Sharder { shards: vec![10, 1, 1] };
    chk(&mut set, "C382-06", bad.imbalanced() && bad.resplit(2) && !bad.imbalanced()
        && bad.total() == 12 && bad.shards.len() == 6, "重分 12→6 片 cap2");

    // C382-07 重分后总量守恒且每片不超 cap。
    chk(&mut set, "C382-07", bad.shards.iter().all(|x| *x <= 2), "每片 ≤ cap");

    // C382-08 零 cap 重分拒绝。
    let mut z = Sharder::split(4, 2);
    chk(&mut set, "C382-08", !z.resplit(0), "零 cap 拒绝");

    // C382-09 集成全链：语种非空+打包+驻留 → 过；断链 → 不过。
    chk(&mut set, "C382-09", integration_case("zh-CN", true, true)
        && !integration_case("", true, true) && !integration_case("ar", false, true), "全链三要素");

    // C382-10 层间失配：缺 e2e → 不完整。
    let mut miss = TestStack::new();
    miss.put(TestLayer::Unit, 5);
    miss.put(TestLayer::Integration, 2);
    chk(&mut set, "C382-10", !miss.complete() && miss.count(TestLayer::E2E) == 0, "缺 e2e 失配");

    // C382-11 覆盖登记幂等：同层重复 put 只更新计数。
    st.put(TestLayer::Unit, 121);
    chk(&mut set, "C382-11", st.count(TestLayer::Unit) == 121 && st.layers.len() == 3, "幂等更新");

    // C382-12 三层名单单源。
    chk(&mut set, "C382-12", TestLayer::ALL.len() == 3
        && TestLayer::ALL[0].name() == "unit" && TestLayer::ALL[2].name() == "e2e", "三层单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves382_smoke() {
        let set = run_ves382_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves382 red: {:?}", red);
    }

    #[test]
    fn ves382_shard_exact() {
        let s = Sharder::split(8, 4);
        assert_eq!(s.shards, vec![2, 2, 2, 2]);
        assert!(!s.imbalanced());
    }
}
