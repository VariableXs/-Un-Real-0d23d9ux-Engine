//! veb304_adrsub —— VE-F0304 Adreno 提交通路。
//! 锚点（CoRun Unxreal 主册）：VE-F0304 · Adreno 提交通路 · 目标 300 行
//! 判据原文：VE-F0304-J1 提交通路账：环形账（IB1 环，2 的幂
//!           尺寸）；写指针推进（只增不回绕错误：环内模）；读
//!           指针跟随（GPU 读到写处即闲）；水位（写 - 读 =
//!           在飞，超水位拒提交）；空环查询（读写平 → 空）；
//!           回绕核（写指针过界回绕，水位计算不破）； retire
//!           核（读指针推进 → 在飞减少）。
//! 组件：环形账 + 双指针 + 水位闸 + retire。
//! 职责定位：提交通路是「Adreno 的传送环」——环是圈：写指针
//!           奔跑、读指针追赶，差距就是在飞量；水位闸是环的
//!           载重线：在飞超载就别再塞；回绕不破水位是环的数
//!           学：圈上的距离永远按短弧算。
//! 性能逐项分解：提交 O(1)；水位 O(1)。
//! 诊断码段：0x31500（0x31501~0x31509 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const RS_VERSION: &str = "veb304_adrsub-v1";

/// 诊断段基址（0x31500 段 · 本模块独占）。
pub const RS_SEG_BASE: u32 = 0x31500;
pub const XC_MAGIC: u32 = 0x31501; // 账魔数破
pub const XC_SIZE: u32 = 0x31502; // 环尺寸非 2 的幂
pub const XC_FULL: u32 = 0x31503; // 超水位

/// 账魔数。
const MAGIC: u32 = 0x3150_1B0E;

/// 默认环尺寸（2 的幂）。
pub const RING_SZ: u32 = 64;

/// 水位（在飞上限）。
pub const HIGH_WATER: u32 = 48;

/// 提交环。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubRing {
    /// 环尺寸（2 的幂）。
    pub size: u32,
    /// 写指针（单调增，模size定位）。
    wptr: u64,
    /// 读指针。
    rptr: u64,
    magic: u32,
}

impl SubRing {
    /// 建。
    pub fn new(size: u32) -> Result<SubRing, u32> {
        if size == 0 || size & (size - 1) != 0 {
            return Err(XC_SIZE);
        }
        Ok(SubRing { size, wptr: 0, rptr: 0, magic: MAGIC })
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 提交 n 笔：在飞 + n；超水位拒（不部分提交）。
    pub fn submit(&mut self, n: u32) -> Result<u64, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let inflight = self.inflight() as u32;
        if inflight + n > HIGH_WATER {
            return Err(XC_FULL);
        }
        self.wptr += n as u64;
        Ok(self.wptr)
    }

    /// retire：读指针推进。
    pub fn retire(&mut self, n: u32) -> Result<u64, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if n as u64 > self.inflight() {
            return Err(XC_MAGIC);
        }
        self.rptr += n as u64;
        Ok(self.rptr)
    }

    /// 在飞（写 - 读）。
    pub fn inflight(&self) -> u64 {
        self.wptr - self.rptr
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.wptr == self.rptr
    }

    /// 写指针环内位置。
    pub fn wpos(&self) -> u32 {
        (self.wptr % self.size as u64) as u32
    }

    /// 读指针环内位置。
    pub fn rpos(&self) -> u32 {
        (self.rptr % self.size as u64) as u32
    }
}

pub fn run_veb304_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0304");
    // 族零：环尺寸。
    s.add("尺寸 0 拒", SubRing::new(0) == Err(XC_SIZE), "无零环");
    s.add("尺寸 63 拒", SubRing::new(63) == Err(XC_SIZE), "非 2 幂");
    s.add("尺寸 100 拒", SubRing::new(100) == Err(XC_SIZE), "非 2 幂");
    s.add("尺寸 64 过", SubRing::new(64).is_ok(), "2 幂");
    // 族一：提交与在飞。
    let mut r = match SubRing::new(RING_SZ) { Ok(x) => x, Err(_) => { s.add("建环败", false, "不应到"); return s; } };
    s.add("空环", r.is_empty(), "平指针");
    s.add("提交 10", r.submit(10) == Ok(10), "写进 10");
    s.add("在飞 10", r.inflight() == 10, "差 10");
    s.add("非空", r.is_empty() == false, "有在飞");
    s.add("再提交 5", r.submit(5) == Ok(15), "累计");
    s.add("在飞 15", r.inflight() == 15, "差 15");
    // 族二：水位闸。
    // 在飞 15 + 34 = 49 > 48 → 拒。
    s.add("超水位拒", r.submit(34) == Err(XC_FULL), "49>48");
    // 恰水位：15 + 33 = 48 → 过。
    s.add("恰水位过", r.submit(33) == Ok(48), "=48");
    s.add("零提交", r.submit(0) == Ok(48), "零不拒");
    // 族三：retire。
    s.add("retire 20", r.retire(20) == Ok(20), "读进 20");
    s.add("在飞 28", r.inflight() == 28, "48-20");
    // 超退拒。
    s.add("超退拒", r.retire(99) == Err(XC_MAGIC), "读不过写");
    // 全退 → 空。
    let _ = r.retire(28);
    s.add("退空", r.is_empty(), "平指针");
    // 族四：回绕核。
    // 推到 60，再退到 0，再推 70：写指针 70，环位 70%64=6。
    let mut w = match SubRing::new(RING_SZ) { Ok(x) => x, Err(_) => { s.add("建环败", false, "不应到"); return s; } };
    let _ = w.submit(60);
    let _ = w.retire(60);
    let _ = w.submit(70);
    s.add("写位回绕", w.wpos() == 6, "70%64");
    s.add("在飞 70", w.inflight() == 70, "单调差");
    // 退到 64：读位 0，在飞 6。
    let _ = w.retire(64);
    s.add("读位 0", w.rpos() == 0, "64%64");
    s.add("在飞 6", w.inflight() == 6, "70-64");
    // 回绕后水位照常。
    s.add("回绕后拒", w.submit(43) == Err(XC_FULL), "6+43=49");
    s.add("回绕后过", w.submit(42) == Ok(112), "6+42=48");
    // 族五：指针位置核。
    let mut p = match SubRing::new(16) { Ok(x) => x, Err(_) => { s.add("建环败", false, "不应到"); return s; } };
    let _ = p.submit(3);
    s.add("小环写位 3", p.wpos() == 3, "3%16");
    let _ = p.submit(15); // 18 总
    s.add("小环回绕 2", p.wpos() == 2, "18%16");
    s.add("在飞 18", p.inflight() == 18, "无回绕损");
    s.add("魔数核", RS_SEG_BASE == 0x31500, "段基址");
    s
}
