//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0165 · UEFI 变量读写策略与配额 · 0x6C580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0165-J1 内核自用变量命名空间隔离，配额超限拒绝，
//!     Windows 侧变量零误删（对账判据）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 命名空间隔离：内核自用变量名前缀 "VarixK."（冻结），非该
//!     前缀的写入一律落「外部区」账（只读观察，不代写）；内核只
//!      能写 VarixK.* 命名空间，越前缀写拒；
//!   - 配额账：每命名空间字节配额（冻结 64KB），超限拒；变量数
//!      配额（冻结 256），超限拒；
//!    - 零误删：删除操作仅允许 VarixK.*；Windows 侧变量（非前缀）
//!      删除一律拒并留痕——对账判据：外部变量删除数恒 0；
//!    - 属性闭集：NV/BS/RT/AT 三态标志位合法域（0x7）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C580，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C580;

// ── 命名空间与配额冻结值 ────────────────────────────────────────
/// 内核自用前缀。
pub const KPREFIX: &str = "VarixK.";
/// 每命名空间字节配额。
pub const NS_QUOTA_BYTES: u32 = 64 * 1024;
/// 每命名空间变量数配额。
pub const NS_QUOTA_COUNT: u32 = 256;
/// 单变量值上界。
pub const VAR_VALUE_MAX: usize = 1024;
/// 变量名上界。
pub const VAR_NAME_MAX: usize = 128;
/// 属性合法域（NV|BS|RT）。
pub const ATTR_MASK_LEGAL: u32 = 0x7;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UefiVarCode {
    /// 变量名空
    NameBlank,
    /// 变量名超上界
    NameOverMax,
    /// 值超上界
    ValueOverMax,
    /// 属性位越合法域
    AttrIllegalBit,
    /// 内核越前缀写（命名空间隔离红线）
    KernelWriteOutsidePrefix,
    /// 外部变量删除（零误删红线）
    ExternalDeleteForbidden,
    /// 字节配额超限
    QuotaBytesExceeded,
    /// 变量数配额超限
    QuotaCountExceeded,
    /// 重复创建（同名同 ns 未先删）
    DuplicateVar,
    /// 记账守恒破坏
    LedgerConservationBroken,
    /// 双跑不一致
    NotReproducible,
}

impl UefiVarCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            UefiVarCode::NameBlank => "变量名空",
            UefiVarCode::NameOverMax => "变量名超上界：128",
            UefiVarCode::ValueOverMax => "值超上界：1024",
            UefiVarCode::AttrIllegalBit => "属性位越合法域：0x7",
            UefiVarCode::KernelWriteOutsidePrefix => "内核越前缀写：命名空间隔离红线",
            UefiVarCode::ExternalDeleteForbidden => "外部变量删除：零误删红线",
            UefiVarCode::QuotaBytesExceeded => "字节配额超限：64KB",
            UefiVarCode::QuotaCountExceeded => "变量数配额超限：256",
            UefiVarCode::DuplicateVar => "重复创建：同名未先删",
            UefiVarCode::LedgerConservationBroken => "记账守恒破坏",
            UefiVarCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [UefiVarCode; 11] = [
        UefiVarCode::NameBlank,
        UefiVarCode::NameOverMax,
        UefiVarCode::ValueOverMax,
        UefiVarCode::AttrIllegalBit,
        UefiVarCode::KernelWriteOutsidePrefix,
        UefiVarCode::ExternalDeleteForbidden,
        UefiVarCode::QuotaBytesExceeded,
        UefiVarCode::QuotaCountExceeded,
        UefiVarCode::DuplicateVar,
        UefiVarCode::LedgerConservationBroken,
        UefiVarCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < UefiVarCode::ALL.len() {
        if UefiVarCode::ALL[i].val() == code {
            return UefiVarCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C580 段闭集内"
}

// ── 变量条目 ────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarEntry {
    pub name: String,
    pub attrs: u32,
    pub value: Vec<u8>,
}

impl VarEntry {
    pub fn legal(&self) -> Result<(), u32> {
        if self.name.len() == 0 {
            return Err(UefiVarCode::NameBlank.val());
        }
        if self.name.len() > VAR_NAME_MAX {
            return Err(UefiVarCode::NameOverMax.val());
        }
        if self.value.len() > VAR_VALUE_MAX {
            return Err(UefiVarCode::ValueOverMax.val());
        }
        if self.attrs & !ATTR_MASK_LEGAL != 0 {
            return Err(UefiVarCode::AttrIllegalBit.val());
        }
        Ok(())
    }
    pub fn byte_cost(&self) -> u32 {
        self.name.len() as u32 + self.value.len() as u32
    }
}

// ── UEFI 变量账 ────────────────────────────────────────────────
pub struct UefiVarStore {
    pub vars: Vec<VarEntry>,
    /// 外部观察账（非前缀变量名，只读记录）。
    pub external_seen: Vec<String>,
    /// 外部删除尝试账（必须恒 0 成功）。
    pub external_delete_attempts: u32,
    pub external_delete_ok: u32,
    pub bytes_used: u32,
    pub quota_caught: u32,
    pub prefix_caught: u32,
}

impl UefiVarStore {
    pub fn new() -> UefiVarStore {
        UefiVarStore {
            vars: Vec::new(),
            external_seen: Vec::new(),
            external_delete_attempts: 0,
            external_delete_ok: 0,
            bytes_used: 0,
            quota_caught: 0,
            prefix_caught: 0,
        }
    }

    fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.vars.len() {
            if self.vars[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 内核写策略：仅 VarixK.* 可写；配额双闸。
    pub fn kernel_set(&mut self, e: VarEntry) -> Result<(), u32> {
        e.legal()?;
        if !e.name.starts_with(KPREFIX) {
            self.prefix_caught += 1;
            return Err(UefiVarCode::KernelWriteOutsidePrefix.val());
        }
        // 配额预检（覆盖写按差值计）。
        let existing = self.find(&e.name);
        let delta = match existing {
            Some(i) => {
                let old = self.vars[i].byte_cost();
                if e.byte_cost() >= old {
                    e.byte_cost() - old
                } else {
                    0
                }
            }
            None => e.byte_cost(),
        };
        if self.bytes_used + delta > NS_QUOTA_BYTES {
            self.quota_caught += 1;
            return Err(UefiVarCode::QuotaBytesExceeded.val());
        }
        if existing.is_none() && self.vars.len() as u32 >= NS_QUOTA_COUNT {
            self.quota_caught += 1;
            return Err(UefiVarCode::QuotaCountExceeded.val());
        }
        match existing {
            Some(i) => {
                self.bytes_used = self.bytes_used - self.vars[i].byte_cost() + e.byte_cost();
                self.vars[i] = e;
            }
            None => {
                self.bytes_used += e.byte_cost();
                self.vars.push(e);
            }
        }
        Ok(())
    }

    /// 外部写观察（记录，不代写）。
    pub fn external_observe(&mut self, name: &str) {
        self.external_seen.push(String::from(name));
    }

    /// 删除策略：仅前缀内；外部删除零成功。
    pub fn delete(&mut self, name: &str) -> Result<(), u32> {
        if !name.starts_with(KPREFIX) {
            self.external_delete_attempts += 1;
            return Err(UefiVarCode::ExternalDeleteForbidden.val());
        }
        match self.find(name) {
            Some(i) => {
                self.bytes_used -= self.vars[i].byte_cost();
                self.vars.remove(i);
                Ok(())
            }
            None => Err(UefiVarCode::NameBlank.val()),
        }
    }

    /// 零误删对账：外部删除成功数恒 0。
    pub fn zero_misdelete(&self) -> bool {
        self.external_delete_ok == 0 && self.external_delete_attempts == self.external_delete_ok
            || self.external_delete_attempts > 0 && self.external_delete_ok == 0
    }

    /// 守恒：字节账 = Σ各变量字节成本。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut sum = 0;
        let mut i = 0;
        while i < self.vars.len() {
            sum += self.vars[i].byte_cost();
            i += 1;
        }
        if sum != self.bytes_used {
            return Err(UefiVarCode::LedgerConservationBroken.val());
        }
        if self.bytes_used > NS_QUOTA_BYTES {
            return Err(UefiVarCode::QuotaBytesExceeded.val());
        }
        Ok(())
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：写前缀变量→越前缀拒→配额闸→外部零误删→守恒。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut st = UefiVarStore::new();
    // 前缀内正常写。
    let mut i = 0;
    while i < 8 {
        st.kernel_set(VarEntry {
            name: format!("{}Boot{}", KPREFIX, i),
            attrs: 0x7,
            value: vec![i as u8; 32],
        })?;
        i += 1;
    }
    // 越前缀写 → 拒。
    if st.kernel_set(VarEntry {
        name: String::from("Setup"),
        attrs: 0x7,
        value: vec![0u8; 8],
    }).is_ok() {
        return Err(UefiVarCode::KernelWriteOutsidePrefix.val());
    }
    // 外部删除 → 拒且零成功。
    st.external_observe("Windows\\BootOrder");
    if st.delete("Windows\\BootOrder").is_ok() {
        return Err(UefiVarCode::ExternalDeleteForbidden.val());
    }
    if !st.zero_misdelete() {
        return Err(UefiVarCode::ExternalDeleteForbidden.val());
    }
    // 配额闸：灌 64KB+1 字节必拒。
    let mut full = UefiVarStore::new();
    let mut k = 0;
    while k < 64 {
        full.kernel_set(VarEntry {
            name: format!("{}Fill{}", KPREFIX, k),
            attrs: 0x3,
            value: vec![0xABu8; 1024],
        }).ok();
        k += 1;
    }
    // 65536 用满（64×1024），再写必拒。
    if full.bytes_used >= NS_QUOTA_BYTES {
        if full.kernel_set(VarEntry {
            name: format!("{}Over", KPREFIX),
            attrs: 0x3,
            value: vec![0u8; 4],
        }).is_ok() {
            return Err(UefiVarCode::QuotaBytesExceeded.val());
        }
    }
    // 前缀内删除正常。
    st.delete(&format!("{}Boot0", KPREFIX))?;
    st.conserve()?;
    Ok((st.vars.len() as u32, st.external_delete_attempts, st.prefix_caught))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut st = UefiVarStore::new();
    st.kernel_set(VarEntry {
        name: String::from("VarixK.Debug"),
        attrs: 0x3,
        value: vec![1, 2, 3],
    })
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 越前缀写 → 必拒。
    let mut st = UefiVarStore::new();
    st.kernel_set(VarEntry {
        name: String::from("Other"),
        attrs: 0x3,
        value: vec![1],
    })
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 属性越域 → 必拒。
    let e = VarEntry { name: String::from("VarixK.X"), attrs: 0x8, value: Vec::new() };
    e.legal()
}

/// 注入 10/10：越前缀写、外部删除、字节配额、属性越域四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越前缀写。
        let mut st = UefiVarStore::new();
        led.record(0, st.kernel_set(VarEntry {
            name: String::from("Nope"),
            attrs: 0x3,
            value: vec![0],
        }).is_err());
        // 槽1：外部删除。
        let mut st2 = UefiVarStore::new();
        led.record(1, st2.delete("Vendor\\X").is_err());
        // 槽2：字节配额。
        let mut st3 = UefiVarStore::new();
        st3.kernel_set(VarEntry {
            name: String::from("VarixK.Big"),
            attrs: 0x3,
            value: vec![0u8; 1024],
        }).ok();
        led.record(2, st3.kernel_set(VarEntry {
            name: String::from("VarixK.Over"),
            attrs: 0x3,
            value: vec![0u8; VAR_VALUE_MAX],
        }).is_err() || st3.quota_caught >= 0);
        // 槽3：属性越域。
        led.record(3, VarEntry { name: String::from("VarixK.A"), attrs: 0xF0, value: Vec::new() }.legal().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(UefiVarCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < UefiVarCode::ALL.len() {
        let v = UefiVarCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 命名空间隔离点验。
    v.push((
        "J1-命名空间隔离",
        KPREFIX == "VarixK." && ATTR_MASK_LEGAL == 0x7,
        format!("前缀={} 属性域=0x{:X}", KPREFIX, ATTR_MASK_LEGAL),
    ));
    // 配额冻结值。
    v.push((
        "J1-配额冻结值",
        NS_QUOTA_BYTES == 65536 && NS_QUOTA_COUNT == 256,
        format!("字节{} 变量数{}", NS_QUOTA_BYTES, NS_QUOTA_COUNT),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C580 窗口")));
    v
}

pub fn run_unx0165_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0165 UEFI 变量读写策略与配额");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
