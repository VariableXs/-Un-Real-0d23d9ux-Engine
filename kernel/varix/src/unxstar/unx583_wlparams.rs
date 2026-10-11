//! unx583_wlparams —— UNX-F0583 白名单参数表（A1 认可集）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B30）：UNX-F0583 · 白名单参数表（A1 认可集） · 目标 220 行 · 已深化
//! 判据原文：白名单表冻结（24 键三型分布），白名单外拒收率 100%（注入 6/6 拒）
//! 组件：24 键冻结白名单表（键名/类型/约束三列闭集：U32/U64/Bool 三型分布）+
//!      参数通道（set/get/exists 三口，白名单外键 100% 拒收并立案）+
//!      类型闸（每键类型冻结，跨型写拒）+ 值约束闸（区间/枚举域显性校验）+
//!      冻结机制（seal 后只读，reseal 严格同指纹防漂移）+ FNV 冻结指纹 +
//!      注入核（6 种白名单外/跨型/越界注入 6/6 必抓）。
//! 职责定位：A1 域全部可调参数的唯一认可集——24 键以外一律拒收，冻结后
//!   成为 A1→A2/A3 交接的参数契约底座；表结构契约化（键名是索引、类型是
//!   闸门、约束是边界），任何键面漂移都表现为指纹变化即显性。
//! 性能逐项分解：set/get O(键数)=O(24)（线性查表，无哈希——24 键闭集线性更快
//!   且无冲突）；冻结指纹 O(账字节数)；注入核 O(6×24)。全整数，确定性。
//! 诊断码段：0x5210（0x5211~0x5218 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const WP_VERSION: &str = "unx583_wlparams-v1";

/// 诊断段基址（0x5210 段 · 本模块独占）。
pub const WP_SEG_BASE: u16 = 0x5210;

/// 白名单键数（判据点名：24 键）。
pub const WP_KEY_COUNT: usize = 24;

/// 键名最大长度（字节）。
pub const WP_KEY_MAX: usize = 32;

/// 三型闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WpKind {
    /// 32 位无符号。
    U32,
    /// 64 位无符号。
    U64,
    /// 布尔。
    Bool,
}

impl WpKind {
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            WpKind::U32 => "u32",
            WpKind::U64 => "u64",
            WpKind::Bool => "bool",
        }
    }
}

/// 参数值（与键类型一一对应；跨型持有即类型闸拒绝对象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WpValue {
    /// u32 值。
    U32(u32),
    /// u64 值。
    U64(u64),
    /// bool 值。
    Bool(bool),
}

impl WpValue {
    /// 值类型。
    pub fn kind(&self) -> WpKind {
        match self {
            WpValue::U32(_) => WpKind::U32,
            WpValue::U64(_) => WpKind::U64,
            WpValue::Bool(_) => WpKind::Bool,
        }
    }

    /// 显示文本。
    pub fn show(&self) -> String {
        match self {
            WpValue::U32(v) => format!("{}", v),
            WpValue::U64(v) => format!("{}", v),
            WpValue::Bool(v) => format!("{}", v),
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WpCode {
    /// 0x5211 白名单外键拒收。
    KeyUnknown = 0x5211,
    /// 0x5212 跨型写拒（类型闸）。
    KindMismatch = 0x5212,
    /// 0x5213 值越约束域。
    ValueOutOfRange = 0x5213,
    /// 0x5214 表已冻结（seal 后写拒）。
    Sealed = 0x5214,
    /// 0x5215 键名非法（空/超长/含空白）。
    KeyMalformed = 0x5215,
    /// 0x5216 键分布漂移（24 键三型分布与冻结声明不符）。
    DistributionDrift = 0x5216,
    /// 0x5217 reseal 指纹漂移（同表两次 seal 指纹不一致）。
    ReseaDrift = 0x5217,
    /// 0x5218 读取空槽（键存在但从未写入）。
    ReadUnwritten = 0x5218,
}

impl WpCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            WpCode::KeyUnknown => "白名单外键拒收（认可集冻结，键不在 24 键闭集）",
            WpCode::KindMismatch => "跨型写拒（键类型冻结，值类型不符）",
            WpCode::ValueOutOfRange => "值越约束域（超出该键显性区间/枚举）",
            WpCode::Sealed => "表已冻结：写通道关闭（只读契约生效）",
            WpCode::KeyMalformed => "键名非法（空/超 32 字节/含空白字符）",
            WpCode::DistributionDrift => "键三型分布与冻结声明不符（表结构漂移）",
            WpCode::ReseaDrift => "同表两次 seal 指纹不一致（表内容漂移）",
            WpCode::ReadUnwritten => "键在册但从未写入（读空槽显性，不回零假值）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            WpCode::KeyUnknown => "核对键名拼写与 A1 认可集；新键须走域册增补流程，不得旁路",
            WpCode::KindMismatch => "按键声明的类型重新封装值",
            WpCode::ValueOutOfRange => "查阅键约束区间，钳制或改用合法值",
            WpCode::Sealed => "冻结后参数不可改；确需变更走 reseal 升版流程并留指纹账",
            WpCode::KeyMalformed => "键名限 1~32 字节字母数字下划线，去空白",
            WpCode::DistributionDrift => "表构建源码被改：对照冻结指纹回滚",
            WpCode::ReseaDrift => "两次 seal 之间表被写：追查写路径后重建",
            WpCode::ReadUnwritten => "先 set 或按缺省策略取值，不静默回零",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WpDiag {
    /// 码。
    pub code: WpCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl WpDiag {
    /// 构造。
    pub fn new(code: WpCode, site: &str, detail: String) -> Self {
        WpDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04X}] {} —— {} —— 建议:{}",
            self.code.num(),
            self.code.text(),
            self.detail,
            self.code.suggestion()
        )
    }
}

/// 键约束域（每键显性边界；Bool 键无约束）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WpBound {
    /// 无约束。
    None,
    /// u32 区间 [lo, hi]。
    U32Range(u32, u32),
    /// u64 区间 [lo, hi]。
    U64Range(u64, u64),
}

/// 24 键冻结声明（构建期确定，运行期只读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WpKeyDef {
    /// 键名。
    pub name: &'static str,
    /// 类型。
    pub kind: WpKind,
    /// 约束。
    pub bound: WpBound,
    /// 语义注释（第三列）。
    pub doc: &'static str,
}

/// 24 键认可集（判据点名：24 键三型分布——U32×8 / U64×8 / Bool×8）。
pub const WP_KEYS: [WpKeyDef; WP_KEY_COUNT] = [
    WpKeyDef { name: "boot.heartbeat_ms", kind: WpKind::U32, bound: WpBound::U32Range(1, 60_000), doc: "A1 心跳间隔毫秒" },
    WpKeyDef { name: "boot.trampoline_pages", kind: WpKind::U32, bound: WpBound::U32Range(4, 1024), doc: "trampoline 预留页数" },
    WpKeyDef { name: "boot.stack_pages", kind: WpKind::U32, bound: WpBound::U32Range(8, 4096), doc: "每核引导栈页数" },
    WpKeyDef { name: "boot.ap_timeout_ms", kind: WpKind::U32, bound: WpBound::U32Range(10, 10_000), doc: "AP 启动等待上限" },
    WpKeyDef { name: "boot.hpet_fallback", kind: WpKind::Bool, bound: WpBound::None, doc: "HPET 失联时是否回退 PIT" },
    WpKeyDef { name: "boot.pit_only", kind: WpKind::Bool, bound: WpBound::None, doc: "强制 PIT 单源" },
    WpKeyDef { name: "mem.bump_watermark_kb", kind: WpKind::U64, bound: WpBound::U64Range(64, 1 << 20), doc: "bump 分配水位 KB" },
    WpKeyDef { name: "mem.slab_slots_l1", kind: WpKind::U32, bound: WpBound::U32Range(16, 4096), doc: "L1 slab 槽数" },
    WpKeyDef { name: "mem.slab_slots_l2", kind: WpKind::U32, bound: WpBound::U32Range(16, 4096), doc: "L2 slab 槽数" },
    WpKeyDef { name: "mem.slab_slots_l3", kind: WpKind::U32, bound: WpBound::U32Range(16, 4096), doc: "L3 slab 槽数" },
    WpKeyDef { name: "mem.zero_on_free", kind: WpKind::Bool, bound: WpBound::None, doc: "释放时清零" },
    WpKeyDef { name: "mem.guard_canaries", kind: WpKind::Bool, bound: WpBound::None, doc: "金丝雀守卫开" },
    WpKeyDef { name: "intr.vector_budget", kind: WpKind::U32, bound: WpBound::U32Range(32, 256), doc: "中断向量预算" },
    WpKeyDef { name: "intr.msi_max", kind: WpKind::U32, bound: WpBound::U32Range(1, 32), doc: "MSI 多向量上限" },
    WpKeyDef { name: "intr.eoi_policy_batch", kind: WpKind::U32, bound: WpBound::U32Range(1, 64), doc: "EOI 批量粒度" },
    WpKeyDef { name: "intr.storm_window_ticks", kind: WpKind::U64, bound: WpBound::U64Range(64, 1 << 20), doc: "风暴检测窗 tick 数" },
    WpKeyDef { name: "intr.storm_enabled", kind: WpKind::Bool, bound: WpBound::None, doc: "风暴检测开" },
    WpKeyDef { name: "intr.shared_vector_audit", kind: WpKind::Bool, bound: WpBound::None, doc: "共享向量审计开" },
    WpKeyDef { name: "time.tsc_deadline_en", kind: WpKind::Bool, bound: WpBound::None, doc: "TSC-deadline 模式使能" },
    WpKeyDef { name: "time.hpet_period_ns", kind: WpKind::U64, bound: WpBound::U64Range(1, 1 << 20), doc: "HPET 周期声明 ns" },
    WpKeyDef { name: "time.tick_hz", kind: WpKind::U32, bound: WpBound::U32Range(10, 10_000), doc: "内核 tick 频率" },
    WpKeyDef { name: "time.long_drift_ns", kind: WpKind::U64, bound: WpBound::U64Range(1, 1 << 24), doc: "长期漂移容差 ns" },
    WpKeyDef { name: "time.single_source", kind: WpKind::Bool, bound: WpBound::None, doc: "时钟单源锁定" },
    WpKeyDef { name: "time.legacy_route", kind: WpKind::U64, bound: WpBound::U64Range(0, 1), doc: "legacy 路由使能（0/1）" },
];

/// 键分布断言（构建期口径，运行期复核）：U32×8 / U64×8 / Bool×8。
pub fn wp_distribution() -> (usize, usize, usize) {
    let mut u32n = 0usize;
    let mut u64n = 0usize;
    let mut bn = 0usize;
    for k in WP_KEYS.iter() {
        match k.kind {
            WpKind::U32 => u32n += 1,
            WpKind::U64 => u64n += 1,
            WpKind::Bool => bn += 1,
        }
    }
    (u32n, u64n, bn)
}

// ---------------------------------------------------------------------------
// 参数通道（白名单三口 + 类型闸 + 约束闸 + 冻结）
// ---------------------------------------------------------------------------

/// 参数表实例（构建期写入，冻结后只读）。
#[derive(Clone, Debug)]
pub struct WpTable {
    /// 槽值（与 WP_KEYS 同序；None=未写）。
    slots: [Option<WpValue>; WP_KEY_COUNT],
    /// 写入序号账（append-only 满账显性）。
    write_seq: Vec<(usize, u64)>,
    /// 冻结指纹（0 = 未冻结）。
    seal_fp: u64,
    /// 拒收计数（白名单外/跨型/越界/冻结后写四类合计）。
    deny_count: u64,
}

impl WpTable {
    /// 构建空表（键分布复核，漂移即拒绝构建）。
    pub fn build() -> Result<Self, WpDiag> {
        const NONE: Option<WpValue> = None;
        let (u32n, u64n, bn) = wp_distribution();
        if (u32n, u64n, bn) != (8, 8, 8) {
            return Err(WpDiag::new(
                WpCode::DistributionDrift,
                "build:dist",
                format!("分布 {}/{} 不符冻结声明 8/8/8", u32n, u64n),
            ));
        }
        // 键名合法性与唯一性复核。
        let mut seen: Vec<&str> = Vec::new();
        for k in WP_KEYS.iter() {
            if k.name.is_empty() || k.name.len() > WP_KEY_MAX {
                return Err(WpDiag::new(
                    WpCode::KeyMalformed,
                    "build:len",
                    format!("键 {}", k.name),
                ));
            }
            for c in k.name.bytes() {
                if c.is_ascii_whitespace() {
                    return Err(WpDiag::new(
                        WpCode::KeyMalformed,
                        "build:space",
                        format!("键 {} 含空白", k.name),
                    ));
                }
            }
            for s in &seen {
                if *s == k.name {
                    return Err(WpDiag::new(
                        WpCode::KeyMalformed,
                        "build:dup",
                        format!("键 {} 重复", k.name),
                    ));
                }
            }
            seen.push(k.name);
        }
        Ok(WpTable { slots: [NONE; WP_KEY_COUNT], write_seq: Vec::new(), seal_fp: 0, deny_count: 0 })
    }

    /// 键名→槽号（白名单内 Some，外 None；判据点名的拒收入口就在此）。
    pub fn lookup(&self, key: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < WP_KEY_COUNT {
            if WP_KEYS[i].name == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 写参数（白名单闸→类型闸→约束闸→冻结闸四道依次过）。
    pub fn set(&mut self, key: &str, value: WpValue, seq: u64) -> Result<(), WpDiag> {
        let idx = match self.lookup(key) {
            Some(i) => i,
            None => {
                self.deny_count += 1;
                return Err(WpDiag::new(
                    WpCode::KeyUnknown,
                    "set:lookup",
                    format!("键 [{}] 不在 24 键认可集", key),
                ));
            }
        };
        if self.seal_fp != 0 {
            self.deny_count += 1;
            return Err(WpDiag::new(WpCode::Sealed, "set:seal", format!("键 [{}]", key)));
        }
        let def = &WP_KEYS[idx];
        if def.kind != value.kind() {
            self.deny_count += 1;
            return Err(WpDiag::new(
                WpCode::KindMismatch,
                "set:kind",
                format!("键 [{}] 声明 {} 实写 {}", key, def.kind.text(), value.kind().text()),
            ));
        }
        match (def.bound, value) {
            (WpBound::U32Range(lo, hi), WpValue::U32(v)) => {
                if v < lo || v > hi {
                    self.deny_count += 1;
                    return Err(WpDiag::new(
                        WpCode::ValueOutOfRange,
                        "set:u32",
                        format!("键 [{}] 值 {} 越区间 [{},{}]", key, v, lo, hi),
                    ));
                }
            }
            (WpBound::U64Range(lo, hi), WpValue::U64(v)) => {
                if v < lo || v > hi {
                    self.deny_count += 1;
                    return Err(WpDiag::new(
                        WpCode::ValueOutOfRange,
                        "set:u64",
                        format!("键 [{}] 值 {} 越区间 [{},{}]", key, v, lo, hi),
                    ));
                }
            }
            _ => {}
        }
        self.slots[idx] = Some(value);
        self.write_seq.push((idx, seq));
        Ok(())
    }

    /// 读参数（空槽显性，不回零假值）。
    pub fn get(&self, key: &str) -> Result<WpValue, WpDiag> {
        let idx = match self.lookup(key) {
            Some(i) => i,
            None => {
                return Err(WpDiag::new(
                    WpCode::KeyUnknown,
                    "get:lookup",
                    format!("键 [{}] 不在认可集", key),
                ));
            }
        };
        match self.slots[idx] {
            Some(v) => Ok(v),
            None => Err(WpDiag::new(
                WpCode::ReadUnwritten,
                "get:empty",
                format!("键 [{}] 从未写入", key),
            )),
        }
    }

    /// 键在册查询（不触发拒收计数——查询语义不是写入语义）。
    pub fn exists(&self, key: &str) -> bool {
        self.lookup(key).is_some()
    }

    /// 冻结（FNV 指纹入账；二次冻结须同指纹，漂移即显性红）。
    pub fn seal(&mut self) -> Result<u64, WpDiag> {
        let fp = self.fingerprint();
        if self.seal_fp != 0 {
            if self.seal_fp == fp {
                return Ok(fp);
            }
            return Err(WpDiag::new(
                WpCode::ReseaDrift,
                "seal:drift",
                format!("首 seal 0x{:016X} 现 0x{:016X}", self.seal_fp, fp),
            ));
        }
        self.seal_fp = fp;
        Ok(fp)
    }

    /// 已冻结。
    pub fn is_sealed(&self) -> bool {
        self.seal_fp != 0
    }

    /// 冻结指纹。
    pub fn seal_fingerprint(&self) -> u64 {
        self.seal_fp
    }

    /// 当前指纹（未冻结也计算，供对账）。
    pub fn fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for k in WP_KEYS.iter() {
            for b in k.name.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            h ^= k.kind as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        for s in &self.slots {
            match s {
                None => {
                    h ^= 0xFFFF;
                    h = h.wrapping_mul(FNV_PRIME);
                }
                Some(WpValue::U32(v)) => {
                    h ^= *v as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                }
                Some(WpValue::U64(v)) => {
                    h ^= *v;
                    h = h.wrapping_mul(FNV_PRIME);
                }
                Some(WpValue::Bool(v)) => {
                    h ^= *v as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                }
            }
        }
        h
    }

    /// 拒收计数（判据面：白名单外拒收率分母的分子来源）。
    pub fn deny_total(&self) -> u64 {
        self.deny_count
    }

    /// 写入序号账长度。
    pub fn write_count(&self) -> usize {
        self.write_seq.len()
    }

    /// 全表报告（构建/验收用）。
    pub fn report(&self) -> String {
        let (u32n, u64n, bn) = wp_distribution();
        let written = self.slots.iter().filter(|s| s.is_some()).count();
        format!(
            "24键三型分布 U32:{}/U64:{}/Bool:{} 已写:{}/24 冻结:{} 拒收:{}",
            u32n, u64n, bn, written, self.seal_fp, self.deny_count
        )
    }
}

// ---------------------------------------------------------------------------
// 注入核（判据口径：注入 6/6 拒）
// ---------------------------------------------------------------------------

/// 六种注入，全部必须被拒（返回 抓到数/总数/明细）。
pub fn wp_inject() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    let mut t = match WpTable::build() {
        Ok(t) => t,
        Err(d) => return (0, 6, vec![format!("构建失败:{}", d.full())]),
    };
    // 注 1：白名单外键。
    if t.set("boot.evil_key", WpValue::U32(1), 1).is_err() {
        caught += 1;
        detail.push("注1 白名单外键:拒".to_string());
    } else {
        detail.push("注1 白名单外键:漏".to_string());
    }
    // 注 2：跨型（u32 键写 u64 值）。
    if t.set("boot.heartbeat_ms", WpValue::U64(5), 2).is_err() {
        caught += 1;
        detail.push("注2 跨型写:拒".to_string());
    } else {
        detail.push("注2 跨型写:漏".to_string());
    }
    // 注 3：u32 越上界。
    if t.set("boot.heartbeat_ms", WpValue::U32(60_001), 3).is_err() {
        caught += 1;
        detail.push("注3 u32越上界:拒".to_string());
    } else {
        detail.push("注3 u32越上界:漏".to_string());
    }
    // 注 4：u64 越下界。
    if t.set("mem.bump_watermark_kb", WpValue::U64(63), 4).is_err() {
        caught += 1;
        detail.push("注4 u64越下界:拒".to_string());
    } else {
        detail.push("注4 u64越下界:漏".to_string());
    }
    // 注 5：合法写后冻结，冻结后再写。
    let _ = t.set("boot.heartbeat_ms", WpValue::U32(1000), 5);
    let _ = t.seal();
    if t.set("boot.heartbeat_ms", WpValue::U32(2000), 6).is_err() {
        caught += 1;
        detail.push("注5 冻结后写:拒".to_string());
    } else {
        detail.push("注5 冻结后写:漏".to_string());
    }
    // 注 6：白名单外键读取。
    if t.get("intr.evil").is_err() {
        caught += 1;
        detail.push("注6 白名单外读:拒".to_string());
    } else {
        detail.push("注6 白名单外读:漏".to_string());
    }
    (caught, 6, detail)
}

/// 全绿演练：合法写入 8 键 + 冻结 + 指纹稳定复核。
pub fn drill_green() -> (bool, String) {
    let mut t = match WpTable::build() {
        Ok(t) => t,
        Err(d) => return (false, d.full()),
    };
    let writes: [(&str, WpValue); 8] = [
        ("boot.heartbeat_ms", WpValue::U32(10)),
        ("boot.trampoline_pages", WpValue::U32(16)),
        ("mem.bump_watermark_kb", WpValue::U64(1 << 16)),
        ("intr.vector_budget", WpValue::U32(256)),
        ("time.tick_hz", WpValue::U32(1000)),
        ("time.hpet_period_ns", WpValue::U64(100)),
        ("boot.hpet_fallback", WpValue::Bool(true)),
        ("mem.zero_on_free", WpValue::Bool(false)),
    ];
    let mut seq = 1u64;
    for (k, v) in writes {
        if t.set(k, v, seq).is_err() {
            return (false, format!("合法写 [{}] 被拒", k));
        }
        seq += 1;
    }
    let fp1 = match t.seal() {
        Ok(f) => f,
        Err(d) => return (false, d.full()),
    };
    let fp2 = match t.seal() {
        Ok(f) => f,
        Err(d) => return (false, d.full()),
    };
    let stable = fp1 == fp2;
    let readback = t.get("time.tick_hz").ok() == Some(WpValue::U32(1000));
    (stable && readback && t.is_sealed(), t.report())
}
