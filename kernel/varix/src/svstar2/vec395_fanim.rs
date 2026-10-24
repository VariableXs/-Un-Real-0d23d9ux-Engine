//! VE-F0795 滤镜参数动画
//! 职责：把滤镜参数随时间变化建模为关键帧轨道 —— 线性插值、
//!       持钳（hold）模式、帧范围外钳制；产出逐帧参数表供链执行。
//! 契约：关键帧时间严格递增；单帧轨道恒值。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 关键帧
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    pub t: u32,
    pub v: f64,
}

/// 参数轨道
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub keys: Vec<Keyframe>,
    /// false = 线性插值，true = 持钳（取前一帧值）
    pub hold: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackErr {
    NotIncreasing,
    Empty,
}

impl Track {
    /// 构造：校验时间严格递增
    pub fn new(keys: Vec<Keyframe>, hold: bool) -> Result<Track, TrackErr> {
        if keys.is_empty() {
            return Err(TrackErr::Empty);
        }
        for w in keys.windows(2) {
            if w[1].t <= w[0].t {
                return Err(TrackErr::NotIncreasing);
            }
        }
        Ok(Track { keys, hold })
    }

    /// 单帧轨道
    pub fn constant(v: f64) -> Track {
        Track { keys: alloc::vec![Keyframe { t: 0, v }], hold: true }
    }

    /// 采样：范围外钳制到端点
    pub fn sample(&self, t: u32) -> f64 {
        let ks = &self.keys;
        if t <= ks[0].t {
            return ks[0].v;
        }
        if t >= ks.last().expect("k").t {
            return ks.last().expect("k").v;
        }
        for w in ks.windows(2) {
            if t >= w[0].t && t <= w[1].t {
                if self.hold {
                    return w[0].v;
                }
                let f = (t - w[0].t) as f64 / (w[1].t - w[0].t) as f64;
                return w[0].v + (w[1].v - w[0].v) * f;
            }
        }
        ks.last().expect("k").v
    }

    /// 展开为逐帧参数表（t0..=t1 闭区间）
    pub fn bake(&self, t0: u32, t1: u32) -> Vec<f64> {
        (t0..=t1).map(|t| self.sample(t)).collect()
    }
}

pub fn run_vec395_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec395");
    // 族一：构造校验
    let ok_tr = Track::new(alloc::vec![
        Keyframe { t: 0, v: 0.0 },
        Keyframe { t: 10, v: 10.0 },
    ], false);
    s.add("轨道-合法构造", ok_tr.is_ok(), "valid");
    s.add("轨道-空拒", Track::new(alloc::vec![], false) == Err(TrackErr::Empty), "empty reject");
    s.add(
        "轨道-倒退拒",
        Track::new(alloc::vec![Keyframe { t: 5, v: 1.0 }, Keyframe { t: 5, v: 2.0 }], false)
            == Err(TrackErr::NotIncreasing),
        "non-increasing reject",
    );
    // 族二：线性插值
    let tr = ok_tr.expect("tr");
    s.add("轨道-t0起点", tr.sample(0) == 0.0, "start");
    s.add("轨道-t5半程", tr.sample(5) == 5.0, "midpoint");
    s.add("轨道-t10终点", tr.sample(10) == 10.0, "end");
    s.add("轨道-t越前钳", tr.sample(100) == 10.0, "clamp hi");
    s.add("轨道-反向钳", tr.sample(0) == tr.keys[0].v, "clamp lo");
    // 族三：非线性段插值
    let tr2 = Track::new(alloc::vec![
        Keyframe { t: 0, v: 0.0 },
        Keyframe { t: 4, v: 8.0 },
        Keyframe { t: 8, v: 0.0 },
    ], false).expect("t2");
    s.add("轨道-峰中点", tr2.sample(2) == 4.0, "up seg");
    s.add("轨道-降段", tr2.sample(6) == 4.0, "down seg");
    // 族四：hold 模式
    let trh = Track::new(alloc::vec![
        Keyframe { t: 0, v: 1.0 },
        Keyframe { t: 10, v: 9.0 },
    ], true).expect("th");
    s.add("轨道-hold取前帧", trh.sample(5) == 1.0, "hold prev");
    s.add("轨道-hold端点", trh.sample(10) == 9.0, "hold end");
    // 族五：单帧恒值
    let tc = Track::constant(3.5);
    s.add("轨道-恒值", (0..=5).all(|t| tc.sample(t) == 3.5), "constant");
    // 族六：bake 展开
    let baked = tr.bake(0, 10);
    s.add("轨道-bake长度", baked.len() == 11, "bake len");
    s.add("轨道-bake首尾", baked[0] == 0.0 && baked[10] == 10.0, "bake ends");
    s.add("轨道-bake单调", baked.windows(2).all(|w| w[1] >= w[0]), "bake monotone");
    // 族七：bake 部分区间
    let part = tr.bake(3, 6);
    s.add("轨道-bake区间", part.len() == 4 && part[0] == 3.0, "bake partial");
    s
}
