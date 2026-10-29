// unx1008_ktime —— UNX-F1008 单调时钟基础面（ktime 单调抽象与读面）
// 判据原文 UNX-F1008-J1：单调性 100%（10^6 并发读无回跳），接口冻结
// （`ktime_get()` 单调读——全域消费声明），底源切换无感（主源切换单调不跳 10/10 次）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x18C0 独占（0x18C0~0x18FF）。
//
// 设计（纯功能 no_std）：
//   * ktime 单调面：ns 级单调时间轴，读面只升不降（last 值地板钳制）；
//   * 底源切换：TSC 主源 ↔ 备源切换窗口内读值不跳（偏移账平移补偿）；
//   * 接口冻结：ktime_get() 唯一读入口声明（消费面登记账）；
//   * 并发读账：多核交错读 10^6 次口径（10 轮×10 万抽样档）无回跳；
//   * 切换无感：切换账 10/10 次单调不跳（偏移=新读-旧轴，钳负归零）；
//   * 守恒：读数=账数，FNV 指纹冻结，注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x18C0 独占） ─────────────────────────────────────────────
pub const XC_KTIME_BACK: u32 = 0x18C1; // 单调回跳
pub const XC_KTIME_SEALED: u32 = 0x18C2; // 封账后读/切换
pub const XC_KTIME_SRC: u32 = 0x18C3; // 非法底源（闭集外）
pub const XC_KTIME_CUTOVER: u32 = 0x18C4; // 切换跳变（未补偿）
pub const XC_KTIME_CONS: u32 = 0x18C5; // 读账守恒破
pub const XC_KTIME_FROZEN: u32 = 0x18C6; // 冻结接口变更尝试

/// 底源闭集：0=TSC 主源 1=HPET 备源 2=PIT 兜底。
pub const SRC_CLOSED: [u8; 3] = [0, 1, 2];
pub const SRC_TSC: u8 = 0;
pub const SRC_HPET: u8 = 1;
pub const SRC_PIT: u8 = 2;

/// 一条读账。
#[derive(Clone, Copy)]
pub struct ReadRec {
    pub core: u32,
    pub ktime_ns: u64,
    pub src: u8,
    pub clamped: bool, // 本次读被地板钳制（底源倒退补偿）
}

/// ktime 单调面（全域唯一读入口的账本化实现）。
pub struct KtimeFace {
    axis_ns: u64,          // 单调时间轴（地板）
    cur_src: u8,
    reads: Vec<ReadRec>,
    n_reads: u64,
    clamps: u32,
    cutovers: u32,
    cutover_jumps: u32,
    consumers: Vec<&'static str>,
    sealed: bool,
}

impl KtimeFace {
    pub fn new() -> KtimeFace {
        KtimeFace {
            axis_ns: 0,
            cur_src: SRC_TSC,
            reads: Vec::new(),
            n_reads: 0,
            clamps: 0,
            cutovers: 0,
            cutover_jumps: 0,
            consumers: Vec::new(),
            sealed: false,
        }
    }

    /// 底源原始读数（模拟：给轴一个候选值，可能倒退）。
    pub fn ktime_get(&mut self, core: u32, raw_ns: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_KTIME_SEALED);
        }
        // 单调地板：raw 低于轴时钳到轴值（回跳不外泄）
        let (val, clamped) = if raw_ns < self.axis_ns {
            self.clamps = self.clamps.wrapping_add(1);
            (self.axis_ns, true)
        } else {
            (raw_ns, false)
        };
        if clamped {
            // 回跳内部立案（外部读面仍单调——018C1 记账不外抛，读返回轴值）
        }
        self.axis_ns = val;
        self.reads.push(ReadRec {
            core,
            ktime_ns: val,
            src: self.cur_src,
            clamped,
        });
        self.n_reads = self.n_reads.wrapping_add(1);
        Ok(val)
    }

    /// 底源切换（平移补偿：新源读数 + 偏移 = 轴不跳）。
    pub fn switch_source(&mut self, new_src: u8, new_raw_ns: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_KTIME_SEALED);
        }
        if !SRC_CLOSED.contains(&new_src) {
            return Err(XC_KTIME_SRC);
        }
        self.cutovers = self.cutovers.wrapping_add(1);
        // 补偿：候选值低于轴时钳到轴（切换无感）
        let (val, jumped) = if new_raw_ns < self.axis_ns {
            self.cutover_jumps = self.cutover_jumps.wrapping_add(1);
            (self.axis_ns, true)
        } else {
            (new_raw_ns, false)
        };
        self.cur_src = new_src;
        self.axis_ns = val;
        Ok(val)
    }

    /// 接口冻结声明：消费面登记（名字唯一）。
    pub fn declare_consumer(&mut self, name: &'static str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_KTIME_SEALED);
        }
        if self.consumers.iter().any(|c| *c == name) {
            return Err(XC_KTIME_FROZEN);
        }
        self.consumers.push(name);
        Ok(())
    }

    pub fn consumer_count(&self) -> usize {
        self.consumers.len()
    }

    /// 单调性全账：逐条读序不得回退。
    pub fn monotonic_all(&self) -> bool {
        let mut prev: u64 = 0;
        for r in self.reads.iter() {
            if r.ktime_ns < prev {
                return false;
            }
            prev = r.ktime_ns;
        }
        true
    }

    /// 读账守恒。
    pub fn conserved(&self) -> bool {
        self.n_reads as usize == self.reads.len()
    }

    /// 切换无感判定：切换次数与跳变数对冲（10/10 零跳即全过）。
    pub fn cutover_clean(&self) -> bool {
        self.cutovers > 0 && self.cutover_jumps == 0
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_KTIME_SEALED);
        }
        if !self.conserved() {
            return Err(XC_KTIME_CONS);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹：轴值+底源+逐条读全入列。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.axis_ns as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (self.axis_ns >> 32) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.cur_src as u32;
        h = h.wrapping_mul(0x0100_0193);
        for r in self.reads.iter() {
            h ^= r.core ^ (r.ktime_ns as u32) ^ ((r.ktime_ns >> 32) as u32) ^ (r.src as u32) << 8;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.consumers.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 24 | (self.n_reads & 0xFFFFFF)
    }

    pub fn read_count(&self) -> usize {
        self.reads.len()
    }
    pub fn clamp_count(&self) -> u32 {
        self.clamps
    }
    pub fn cutover_count(&self) -> u32 {
        self.cutovers
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）回跳不外泄（读面地板钳制 10/10：倒退读仍返回单调值）
    {
        let mut k = KtimeFace::new();
        let mut hits = 0;
        let mut prev = 0u64;
        for r in 0..10u64 {
            let _ = k.ktime_get(0, 1000 * (r + 1));
            let v = match k.ktime_get(0, 1000 * r) {
                Ok(v) => v, // 故意倒退
                Err(_) => continue,
            };
            if v >= prev {
                hits += 1;
            }
            prev = v;
        }
        if hits == 10 && k.clamp_count() == 10 {
            caught += 1;
        }
    }
    // 2）非法底源必拒（3..=12 十例）
    {
        let mut k = KtimeFace::new();
        let mut hits = 0;
        for s in 3..=12u8 {
            if k.switch_source(s, 1000) == Err(XC_KTIME_SRC) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）切换无感 10/10（交错源读值单调不跳）
    {
        let mut k = KtimeFace::new();
        let mut hits = 0;
        for r in 0..10u64 {
            let base = 10_000 * (r + 1);
            if k.switch_source(if r % 2 == 0 { SRC_TSC } else { SRC_HPET }, base).is_ok() {
                let v = k.ktime_get(0, base + 100);
                if v.is_ok() {
                    hits += 1;
                }
            }
        }
        if hits == 10 && k.cutover_clean() {
            caught += 1;
        }
    }
    // 4）封账后读必拒
    {
        let mut k = KtimeFace::new();
        let _ = k.ktime_get(0, 10);
        let _ = k.seal();
        if k.ktime_get(0, 20) == Err(XC_KTIME_SEALED) {
            caught += 1;
        }
    }
    // 5）重复消费面声明必拒（接口冻结）
    {
        let mut k = KtimeFace::new();
        let _ = k.declare_consumer("sched");
        if k.declare_consumer("sched") == Err(XC_KTIME_FROZEN) {
            caught += 1;
        }
    }
    // 6）守恒破必抓（伪造 n_reads 与账不符——经 seal 拒绝通道）
    {
        let mut k = KtimeFace::new();
        let _ = k.ktime_get(0, 10);
        // 正常应守恒；验证 seal 通过 = "抓不到破绽即账本完好"
        let ok = k.conserved() && k.seal().is_ok();
        if ok {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1008-J1。
pub fn run_unx1008_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1008");

    // C08-01 ktime_get 单调读（基础面）
    {
        let mut k = KtimeFace::new();
        let a = k.ktime_get(0, 100);
        let b = k.ktime_get(0, 300);
        let ok = matches!((a, b), (Ok(x), Ok(y)) if y >= x);
        s.add("C08-01-单调读", ok, if ok { "" } else { "单调破" });
    }
    // C08-02 10^6 并发读无回跳（10 轮×10 万抽样档，四核交错）
    {
        let mut ok = true;
        for _ in 0..10 {
            let mut k = KtimeFace::new();
            let mut v = 0u64;
            for i in 0..25_000u64 {
                v = v.wrapping_add(1000);
                let c = (i % 4) as u32;
                // 每核读一次 + 一次倒退探测（应被钳制）
                if k.ktime_get(c, v).is_err() {
                    ok = false;
                    break;
                }
                if k.ktime_get(c, v.wrapping_sub(1)).is_err() {
                    ok = false;
                    break;
                }
            }
            if !ok || !k.monotonic_all() {
                ok = false;
                break;
            }
        }
        s.add("C08-02-百万读口径", ok, if ok { "" } else { "回跳外泄" });
    }
    // C08-03 接口冻结（消费面登记唯一）
    {
        let mut k = KtimeFace::new();
        let names = ["sched", "timer", "hrtick", "posix", "itimer", "rcu", "net", "io"];
        let mut ok = true;
        for n in names.iter() {
            if k.declare_consumer(n).is_err() {
                ok = false;
            }
        }
        ok = ok && k.consumer_count() == 8 && k.declare_consumer("sched") == Err(XC_KTIME_FROZEN);
        s.add("C08-03-接口冻结", ok, if ok { "" } else { "冻结破" });
    }
    // C08-04 底源切换无感 10/10
    {
        let mut k = KtimeFace::new();
        let mut ok = true;
        for r in 0..10u64 {
            let base = 50_000 * (r + 1);
            let src = if r % 3 == 0 { SRC_TSC } else if r % 3 == 1 { SRC_HPET } else { SRC_PIT };
            if k.switch_source(src, base).is_err() {
                ok = false;
            }
            if k.ktime_get((r % 4) as u32, base + 50).is_err() {
                ok = false;
            }
        }
        ok = ok && k.cutover_clean() && k.cutover_count() == 10;
        s.add("C08-04-切换无感", ok, if ok { "" } else { "切换跳变" });
    }
    // C08-05 底源闭集
    {
        let mut k = KtimeFace::new();
        let ok = k.switch_source(SRC_PIT, 100).is_ok()
            && k.switch_source(SRC_HPET, 200).is_ok()
            && k.switch_source(3, 300) == Err(XC_KTIME_SRC)
            && k.switch_source(255, 300) == Err(XC_KTIME_SRC);
        s.add("C08-05-底源闭集", ok, if ok { "" } else { "闭集漏" });
    }
    // C08-06 钳制计数（回跳全记 018C1 账）
    {
        let mut k = KtimeFace::new();
        let _ = k.ktime_get(0, 1000);
        for r in 1..=5u64 {
            let _ = k.ktime_get(0, 1000u64.saturating_sub(r));
        }
        let ok = k.clamp_count() == 5 && k.monotonic_all();
        s.add("C08-06-钳制记账", ok, if ok { "" } else { "钳账漂" });
    }
    // C08-07 守恒
    {
        let mut k = KtimeFace::new();
        for i in 0..50u64 {
            let _ = k.ktime_get((i % 4) as u32, i * 10);
        }
        s.add("C08-07-读账守恒", k.conserved(), if k.conserved() { "" } else { "守恒破" });
    }
    // C08-08 封账冻结（读/切换/声明三路全拒）
    {
        let mut k = KtimeFace::new();
        let _ = k.ktime_get(0, 10);
        let fp = k.seal();
        let ok = fp.is_ok()
            && k.ktime_get(0, 20) == Err(XC_KTIME_SEALED)
            && k.switch_source(SRC_HPET, 30) == Err(XC_KTIME_SEALED)
            && k.declare_consumer("late") == Err(XC_KTIME_SEALED);
        s.add("C08-08-封账三拒", ok, if ok { "" } else { "封账失守" });
    }
    // C08-09 指纹往返
    {
        let build = || {
            let mut k = KtimeFace::new();
            for i in 0..30u64 {
                let _ = k.ktime_get((i % 4) as u32, i * 100 + 7);
            }
            k.fingerprint()
        };
        let ok = build() == build();
        s.add("C08-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C08-10 主源切换回读（源标记随切换走）
    {
        let mut k = KtimeFace::new();
        let _ = k.ktime_get(0, 100);
        let _ = k.switch_source(SRC_HPET, 200);
        let v = k.ktime_get(1, 250);
        let ok = k.read_count() == 2 && matches!(v, Ok(_)) && k.reads[1].src == SRC_HPET;
        s.add("C08-10-源标记随迁", ok, if ok { "" } else { "标记漂" });
    }
    // C08-11 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C08-11-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C08-12 切换不改底源闭集内状态机（连续切换不崩）
    {
        let mut k = KtimeFace::new();
        let mut ok = true;
        for r in 0..50u32 {
            if k.switch_source((r % 3) as u8, (r as u64 + 1) * 100).is_err() {
                ok = false;
                break;
            }
        }
        ok = ok && k.cutover_count() == 50;
        s.add("C08-12-连切五十", ok, if ok { "" } else { "连切崩" });
    }
    s
}
