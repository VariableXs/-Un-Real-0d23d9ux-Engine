//! 驱动框架与内核模块通道收口（WP-404 · B-3701~3703 · 篇 37）。
//!
//! 生命周期铁律三条：probe 失败留痕不阻断总线（一个设备坏不拖死一类设备）、
//! remove 必须可重入（热拔插的反复横跳是常态不是异常）、引用计数管设备对象
//! 生命周期（使用中的设备对象零悬空——**B-3701 达标线：拔插横跳压测零悬空**）；
//! 错误类枚举全系统唯一全覆盖（超时/CRC/协议错/资源枯竭四类——**B-3702
//! 达标线**）；模块通道空载预留符号白名单就位（**B-3703 达标线**）。

// ---------------------------------------------------------------------------
// B-3701 驱动生命周期：probe 不阻断 / remove 可重入 / 引用零悬空
// ---------------------------------------------------------------------------

/// 设备对象：唯一实例标识 + 绑定态 + 引用计数（生命周期即计数）。
#[derive(Clone, Copy, Debug)]
pub struct DeviceObj {
    pub id: u8,
    /// 使用中引用数（U 盘拔出瞬间的凶险路径靠它兜住）。
    pub refs: u32,
    /// 驱动绑定态。
    pub bound: bool,
    /// 曾成功绑定（僵尸判据的依据：probe 失败的未绑定设备是合法暂态，
    /// 曾绑定却解绑零引用仍滞留在册才是账面僵尸）。
    pub ever_bound: bool,
}

impl DeviceObj {
    pub fn new(id: u8) -> Self {
        DeviceObj { id, refs: 0, bound: false, ever_bound: false }
    }
}

/// 总线：设备集合 + probe 失败留痕（失败留痕不阻断——一个设备坏不拖死一类）。
#[derive(Clone, Copy)]
pub struct Bus {
    pub devices: [Option<DeviceObj>; 4],
    /// probe 失败留痕计数（账面在册——不是静默吞掉）。
    pub probe_fails: u32,
}

impl Bus {
    pub fn new() -> Self {
        Bus { devices: [None; 4], probe_fails: 0 }
    }

    /// 注册设备（枚举通道：PCI/USB/平台设备统一走此入口）。
    pub fn register(&mut self, slot: usize, dev: DeviceObj) -> bool {
        if slot >= 4 || self.devices[slot].is_some() {
            return false;
        }
        self.devices[slot] = Some(dev);
        true
    }

    /// probe：ok=false 留痕返回假——**不阻断**（其余槽位照常可用）。
    /// 成功绑定留 ever_bound 痕（僵尸审计的判据依据）。
    pub fn probe(&mut self, slot: usize, ok: bool) -> bool {
        match self.devices.get_mut(slot) {
            Some(Some(d)) => {
                d.bound = ok;
                if ok {
                    d.ever_bound = true;
                    true
                } else {
                    self.probe_fails += 1;
                    false
                }
            }
            _ => {
                self.probe_fails += 1;
                false
            }
        }
    }

    /// 引用获取（使用中计数 +1）。
    pub fn acquire(&mut self, slot: usize) -> bool {
        match self.devices.get_mut(slot) {
            Some(Some(d)) => {
                d.refs += 1;
                true
            }
            _ => false,
        }
    }

    /// 引用释放（计数 -1，不越零）。
    pub fn release(&mut self, slot: usize) -> bool {
        match self.devices.get_mut(slot) {
            Some(Some(d)) => {
                if d.refs > 0 {
                    d.refs -= 1;
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// remove 可重入：使用中（refs>0）先解绑但对象在册——**零悬空**；
    /// 再 remove 幂等返回假（无可移之物）。
    pub fn remove(&mut self, slot: usize) -> bool {
        match self.devices.get_mut(slot) {
            Some(Some(d)) => {
                if d.refs > 0 {
                    // 使用中：只解绑不消亡——对象仍在册，引用不悬空。
                    d.bound = false;
                    false
                } else {
                    self.devices[slot] = None;
                    true
                }
            }
            _ => false, // 空槽再 remove——幂等假
        }
    }

    /// 悬空审计：引用计数与在册性对账。悬空 = 引用指向已消亡对象——本模型
    /// 消亡仅当 refs==0（remove 保证），故"使用中必然在册"是结构保证；可执行
    /// 的账面对账是**僵尸判据**：曾绑定、现已解绑、零引用仍滞留在册（合法暂态
    /// 只有 probe 失败的未绑定设备——它从未 ever_bound）。
    pub fn no_dangle(&self) -> bool {
        self.devices.iter().all(|s| match s {
            None => true,
            Some(d) => !(d.ever_bound && !d.bound && d.refs == 0),
        })
    }
}

// ---------------------------------------------------------------------------
// B-3702 健康账本：错误类枚举全覆盖
// ---------------------------------------------------------------------------

/// 错误类（全系统唯一枚举：与崩溃分类学同构——错误有归宿）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrClass {
    Timeout,
    Crc,
    Protocol,
    Exhausted,
}

pub const ERR_CLASSES: usize = 4;

/// 健康账本：四类分别计数 + 总数守恒（实时健康与体质画像的共用底账）。
#[derive(Clone, Copy)]
pub struct HealthLedger {
    counts: [u32; ERR_CLASSES],
    pub total: u32,
}

impl HealthLedger {
    pub fn new() -> Self {
        HealthLedger { counts: [0; ERR_CLASSES], total: 0 }
    }

    pub fn record(&mut self, c: ErrClass) {
        let idx = match c {
            ErrClass::Timeout => 0,
            ErrClass::Crc => 1,
            ErrClass::Protocol => 2,
            ErrClass::Exhausted => 3,
        };
        self.counts[idx] += 1;
        self.total += 1;
    }

    pub fn count_of(&self, c: ErrClass) -> u32 {
        match c {
            ErrClass::Timeout => self.counts[0],
            ErrClass::Crc => self.counts[1],
            ErrClass::Protocol => self.counts[2],
            ErrClass::Exhausted => self.counts[3],
        }
    }

    /// 守恒：四类之和 == 总数（多记少记都是账面破损）。
    pub fn conserved(&self) -> bool {
        self.counts.iter().sum::<u32>() == self.total
    }
}

// ---------------------------------------------------------------------------
// B-3703 模块通道：空载预留 + 符号白名单
// ---------------------------------------------------------------------------

/// 内核模块通道（为硬件矩阵扩张预留：注册面就位、当前空载）。
pub struct ModChannel {
    /// 符号白名单（模块可见的内核符号编号表——白名单外拒载）。
    pub whitelist: [u16; 8],
    pub whitelist_len: usize,
    /// 已载模块数（空载预留：当前恒 0，通道不是死的）。
    pub loaded: usize,
}

pub const CHANNEL_EMPTY: usize = 0;

impl ModChannel {
    pub fn new() -> Self {
        ModChannel {
            whitelist: [1, 2, 3, 5, 8, 13, 21, 34],
            whitelist_len: 8,
            loaded: 0,
        }
    }

    pub fn whitelisted(&self, sym: u16) -> bool {
        self.whitelist[..self.whitelist_len].contains(&sym)
    }

    /// 载入裁决：白名单外符号拒——通道开放不等于大门敞开。
    pub fn load(&mut self, sym: u16) -> bool {
        if self.whitelisted(sym) {
            self.loaded += 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-3701~3703 · 7 项）
// ---------------------------------------------------------------------------

/// 驱动框架判据（WP-404）。
pub fn run_drvframe_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("drvframe");
    // 1. probe 失败留痕不阻断（**B-3701 前提**）：一槽败其余槽照常绑定。
    let mut bus = Bus::new();
    let _ = bus.register(0, DeviceObj::new(1));
    let _ = bus.register(1, DeviceObj::new(2));
    let p0 = bus.probe(0, false);
    let p1 = bus.probe(1, true);
    cs.add(
        "B-3701 probe 不阻断",
        !p0 && p1 && bus.probe_fails == 1,
        "一个设备坏留痕在册——不拖死一类设备",
    );
    // 2. remove 可重入（反复横跳）：空槽 remove 幂等假、绑定解绑再 remove 正常。
    let mut bus2 = Bus::new();
    let _ = bus2.register(0, DeviceObj::new(1));
    let r1 = bus2.remove(0);
    let r2 = bus2.remove(0); // 再 remove——幂等假，不崩
    cs.add("B-3701 remove 可重入", r1 && !r2 && bus2.devices[0].is_none(), "热拔插反复横跳是常态——重入不炸");
    // 3. 使用中零悬空（**B-3701 达标线**）：refs>0 时 remove 只解绑不消亡。
    let mut bus3 = Bus::new();
    let _ = bus3.register(0, DeviceObj::new(1));
    let _ = bus3.probe(0, true);
    let _ = bus3.acquire(0);
    let _ = bus3.acquire(0);
    let refused = !bus3.remove(0);
    let alive = bus3.devices[0].map(|d| d.refs == 2 && !d.bound).unwrap_or(false);
    let _ = bus3.release(0);
    let _ = bus3.release(0);
    let gone = bus3.remove(0);
    cs.add(
        "B-3701 零悬空",
        refused && alive && gone,
        "使用中对象消亡不存在——拔出瞬间的凶险路径靠计数兜住",
    );
    // 4. 错误类四类全覆盖（**B-3702 达标线**）：逐类登记逐类可查。
    let mut hl = HealthLedger::new();
    hl.record(ErrClass::Timeout);
    hl.record(ErrClass::Crc);
    hl.record(ErrClass::Protocol);
    hl.record(ErrClass::Exhausted);
    cs.add(
        "B-3702 四类全覆盖",
        hl.count_of(ErrClass::Timeout) == 1
            && hl.count_of(ErrClass::Crc) == 1
            && hl.count_of(ErrClass::Protocol) == 1
            && hl.count_of(ErrClass::Exhausted) == 1,
        "超时/CRC/协议错/资源枯竭——错误类枚举全系统唯一",
    );
    // 5. 账本守恒：类和==总数（体质画像的账面底线）。
    hl.record(ErrClass::Crc);
    cs.add("B-3702 账本守恒", hl.conserved() && hl.total == 5, "多记少记都是账面破损——守恒是审计前提");
    // 6. 模块通道空载预留（**B-3703 达标线**）：通道在册零载入。
    let ch = ModChannel::new();
    cs.add(
        "B-3703 空载预留",
        ch.loaded == CHANNEL_EMPTY && ch.whitelist_len == 8,
        "通道为硬件矩阵扩张预留——注册面就位、当前空载",
    );
    // 7. 符号白名单：白名单内放行、白名单外拒（**B-3703 达标线**）。
    let mut ch2 = ModChannel::new();
    let in_ok = ch2.load(13);
    let out_deny = !ch2.load(999);
    cs.add("B-3703 符号白名单", in_ok && out_deny && ch2.loaded == 1, "通道开放不等于大门敞开——白名单外拒载");
    cs
}

// ---------------------------------------------------------------------------
// 单测（fe35 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe35_probe_remove_lifecycle() {
        // 横跳压测：bind→unbind→bind 循环+穿插 probe 失败——账面一致。
        let mut bus = Bus::new();
        let _ = bus.register(0, DeviceObj::new(7));
        for i in 0..10 {
            // probe 返回值语义 = 本次调用是否成功——失败返假是行为不是断言失败。
            assert_eq!(bus.probe(0, i % 3 != 0), i % 3 != 0);
            assert_eq!(bus.devices[0].map(|d| d.bound), Some(i % 3 != 0));
        }
        assert_eq!(bus.probe_fails, 4);
        // 双槽互不阻断。
        let _ = bus.register(1, DeviceObj::new(8));
        assert!(!bus.probe(0, false));
        assert!(bus.probe(1, true));
    }

    #[test]
    fn fe35_refcount_no_dangle() {
        // 横跳压测：acquire/remove 混合循环终局——零悬空恒真。
        let mut bus = Bus::new();
        let _ = bus.register(0, DeviceObj::new(3));
        let _ = bus.probe(0, true);
        for i in 0..20 {
            let _ = bus.acquire(0);
            if i % 2 == 0 {
                let _ = bus.release(0);
            }
        }
        // refs 未清零 → remove 拒消亡只解绑。
        assert!(!bus.remove(0));
        assert!(bus.no_dangle());
        // 清零后可消亡。
        while bus.release(0) {}
        assert!(bus.remove(0));
        assert!(bus.no_dangle());
    }

    #[test]
    fn fe35_health_ledger() {
        // 空账本守恒；偏斜分布守恒；逐类独立计数。
        let mut hl = HealthLedger::new();
        assert!(hl.conserved() && hl.total == 0);
        for _ in 0..7 {
            hl.record(ErrClass::Timeout);
        }
        for _ in 0..2 {
            hl.record(ErrClass::Exhausted);
        }
        assert_eq!(hl.count_of(ErrClass::Timeout), 7);
        assert_eq!(hl.count_of(ErrClass::Exhausted), 2);
        assert_eq!(hl.count_of(ErrClass::Crc), 0);
        assert!(hl.conserved() && hl.total == 9);
    }

    #[test]
    fn fe35_module_channel() {
        // 白名单全表对练：8 个白名单符号全过、表外全拒、计数对账。
        let mut ch = ModChannel::new();
        let wl = ch.whitelist[..ch.whitelist_len].to_vec();
        for &s in &wl {
            assert!(ch.load(s));
        }
        assert_eq!(ch.loaded, wl.len());
        for s in [0u16, 4, 6, 100, 65535] {
            assert!(!ch.whitelisted(s));
            assert!(!ch.load(s));
        }
        assert_eq!(ch.loaded, wl.len()); // 拒载不计数
    }
}
