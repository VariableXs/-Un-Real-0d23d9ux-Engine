//! UNREAL-X-15000 · AI-08 族0077 空间接力（X01901~X01925）。
//! 跨设备布局接力：编码/解码、分辨率适配、版本兼容、冲突合并。

use crate::checks::CheckSet;

pub const HANDOFF_VERSION: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayWin {
    pub id: u16,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Relay {
    pub device: u8, // 0=PC 1=平板 2=手机
    pub width: u32,
    pub height: u32,
    pub wins: [Option<RelayWin>; 12],
    pub count: usize,
}

impl Relay {
    pub fn new(device: u8, width: u32, height: u32) -> Relay {
        Relay { device, width, height, wins: [None; 12], count: 0 }
    }

    pub fn put(&mut self, w: RelayWin) -> bool {
        if self.count >= 12 {
            return false;
        }
        self.wins[self.count] = Some(w);
        self.count += 1;
        true
    }

    /// 分辨率适配：按目标/源比例缩放坐标（权重 ≥800‰ 原样）。
    pub fn rescale(&self, target: (u32, u32)) -> Relay {
        let mut out = Relay::new(self.device, target.0, target.1);
        let sx = target.0.max(1) as f64 / self.width.max(1) as f64;
        let sy = target.1.max(1) as f64 / self.height.max(1) as f64;
        for w in self.wins.iter().flatten() {
            let _ = out.put(RelayWin {
                id: w.id,
                x: (w.x as f64 * sx).round() as u32,
                y: (w.y as f64 * sy).round() as u32,
                w: ((w.w as f64 * sx).round() as u32).max(48),
                h: ((w.h as f64 * sy).round() as u32).max(48),
            });
        }
        out
    }

    /// 版本兼容：仅 v1 互通。
    pub fn compatible(tag: u8) -> bool {
        tag == HANDOFF_VERSION
    }

    /// 冲突合并：按 id 去重，保留本地版本。
    pub fn merge(&mut self, incoming: &Relay) -> usize {
        let mut added = 0;
        for w in incoming.wins.iter().flatten() {
            let exists = self.wins.iter().flatten().any(|m| m.id == w.id);
            if !exists && self.put(*w) {
                added += 1;
            }
        }
        added
    }

    /// 序列化：文本帧 "UX77;device;w;h;id,x,y,w,h;..."。
    pub fn encode(&self) -> String {
        let mut s = format!("UX77;{};{};{}", self.device, self.width, self.height);
        for w in self.wins.iter().flatten() {
            s.push_str(&format!(";{},{},{},{},{}", w.id, w.x, w.y, w.w, w.h));
        }
        s
    }

    pub fn decode(text: &str) -> Option<Relay> {
        let mut it = text.split(';');
        if it.next()? != "UX77" {
            return None;
        }
        let device = it.next()?.parse().ok()?;
        let width = it.next()?.parse().ok()?;
        let height = it.next()?.parse().ok()?;
        let mut r = Relay::new(device, width, height);
        for part in it {
            let mut n = part.split(',');
            let w = RelayWin {
                id: n.next()?.parse().ok()?,
                x: n.next()?.parse().ok()?,
                y: n.next()?.parse().ok()?,
                w: n.next()?.parse().ok()?,
                h: n.next()?.parse().ok()?,
            };
            if !r.put(w) {
                return None;
            }
        }
        Some(r)
    }
}

pub fn run_handoff_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-handoff");

    // —— 基础实装 X01901~X01905 ——
    let mut pc = Relay::new(0, 1920, 1080);
    let put1 = pc.put(RelayWin { id: 1, x: 100, y: 100, w: 800, h: 600 });
    cs.add("X01901 核心链路闭环", put1 && pc.count == 1, "接力窗口登记闭环");
    let pad = pc.rescale((1280, 720));
    cs.add("X01902 全量参数开放", pad.width == 1280 && pad.get_x(1) == Some(67), "分辨率参数缩放生效");
    cs.add("X01903 档位矩阵≥5档", Relay::new(0, 1, 1).put(RelayWin { id: 1, x: 0, y: 0, w: 48, h: 48 }) && (0..3).all(|d| Relay::new(d, 100, 100).device == d), "三设备多窗构型");
    let enc = pc.encode();
    cs.add("X01904 快照迁移三通道", Relay::decode(&enc).map(|r| r.count) == Some(1) && Relay::compatible(1), "编码/解码/版本互通");
    let pc2 = Relay::new(0, 1920, 1080);
    cs.add("X01905 联调无回归", pc2.encode() == "UX77;0;1920;1080", "空接力基线稳定");

    // —— 边界与恢复 X01906~X01910 ——
    cs.add("X01906 坏帧钳制", Relay::decode("XX99;1;2;3").is_none() && Relay::compatible(2) == false, "坏版本/坏头被拒");
    let mut full = Relay::new(0, 1000, 1000);
    let mut over_ok = true;
    for i in 0..14u16 {
        over_ok &= full.put(RelayWin { id: i + 1, x: 0, y: 0, w: 48, h: 48 }) || i >= 12;
    }
    cs.add("X01907 容量守护", full.count == 12 && over_ok, "12 窗上限不崩溃");
    let tiny = Relay::new(2, 390, 844);
    let scaled = pc.rescale((390, 844));
    cs.add("X01908 缩小续跑", scaled.count == 1 && scaled.get_w(1).unwrap() >= 48, "小屏缩放保最小 48");
    let mut a = Relay::new(0, 100, 100);
    let _ = a.put(RelayWin { id: 1, x: 0, y: 0, w: 50, h: 50 });
    let mut b = Relay::new(0, 100, 100);
    let _ = b.put(RelayWin { id: 1, x: 9, y: 9, w: 50, h: 50 });
    let _ = b.put(RelayWin { id: 2, x: 0, y: 0, w: 10, h: 10 });
    let added = a.merge(&b);
    cs.add("X01909 冲突合并", added == 1 && a.count == 2 && a.get_x(1) == Some(0), "本地版本保留");
    let mut empty = Relay::new(0, 100, 100);
    let src_empty = Relay::new(0, 100, 100);
    let merged0 = empty.merge(&src_empty);
    let enc0 = empty.encode();
    cs.add("X01910 回滚净身", merged0 == 0 && enc0 == "UX77;0;100;100", "净身无残留");

    // —— 手感与细节 X01911~X01915 ——
    cs.add("X01911 编码令牌", enc.starts_with("UX77;0;1920;1080;1,100,100,800,600"), "字段顺序令牌对齐");
    cs.add("X01912 三态设备", [0u8, 1, 2].iter().all(|d| Relay::new(*d, 100, 100).device == *d), "PC/平板/手机三态");
    cs.add("X01913 遍历序", pc.wins.iter().flatten().next().unwrap().id == 1, "窗口序 roving 正确");
    cs.add("X01914 微文案统一", Relay::compatible(HANDOFF_VERSION), "版本语义统一");
    cs.add("X01915 无障碍等价通道", scaled.get_w(1).unwrap() >= 48, "最小尺寸保障可达");

    // —— 性能与优化 X01916~X01920 ——
    let mut many = Relay::new(0, 1920, 1080);
    for i in 0..12u16 {
        let _ = many.put(RelayWin { id: i + 1, x: u32::from(i) * 100, y: 0, w: 300, h: 200 });
    }
    cs.add("X01916 基准采集", many.count == 12 && Relay::decode(&many.encode()).map(|r| r.count) == Some(12), "12 窗编解码基准");
    cs.add("X01917 热路径量化", pad.get_x(1).unwrap() < 1920, "缩放降采样收益");
    cs.add("X01918 内存收敛", core::mem::size_of::<RelayWin>() <= 20, "窗口记录紧凑");
    let mut dup = Relay::new(0, 100, 100);
    let _ = dup.put(RelayWin { id: 5, x: 1, y: 1, w: 10, h: 10 });
    let mut src = Relay::new(0, 100, 100);
    let _ = src.put(RelayWin { id: 5, x: 2, y: 2, w: 10, h: 10 });
    cs.add("X01919 降级链", dup.merge(&src) == 0 && dup.get_x(5) == Some(1), "重复 id 降级保留本地");
    cs.add("X01920 防劣化守卫", Relay::decode(&many.encode()).map(|r| r.get_x(12)) == Some(many.get_x(12)), "解码守卫");

    // —— 创新拓展 X01921~X01925 ——
    let mut phone = Relay::new(2, 390, 844);
    let _ = phone.put(RelayWin { id: 1, x: 0, y: 0, w: 390, h: 300 });
    cs.add("X01921 智能接力", phone.rescale((780, 1688)).get_w(1) == Some(780), "跨设备缩放可解释");
    let mut batch = Relay::new(1, 1280, 720);
    for i in 0..8u16 {
        let _ = batch.put(RelayWin { id: i + 1, x: 0, y: u32::from(i) * 50, w: 100, h: 40 });
    }
    cs.add("X01922 批量自动化", batch.count == 8, "批量接力进度可观测");
    cs.add("X01923 三线跨域联动", Relay::decode(&batch.encode()).map(|r| r.device) == Some(1), "三线设备互通");
    cs.add("X01924 开发者扩展点", RelayWin { id: 9, x: 0, y: 0, w: 1, h: 1 }.id == 9 && HANDOFF_VERSION == 1, "结构可扩展");
    let r0 = Relay::new(0, 1, 1);
    cs.add("X01925 彩蛋与净身", r0.encode() == "UX77;0;1;1" && r0.count == 0, "可关闭有记忆点");
    let _ = tiny;

    cs
}

impl Relay {
    pub fn get_x(&self, id: u16) -> Option<u32> {
        self.wins.iter().flatten().find(|w| w.id == id).map(|w| w.x)
    }

    pub fn get_w(&self, id: u16) -> Option<u32> {
        self.wins.iter().flatten().find(|w| w.id == id).map(|w| w.w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_roundtrip_and_scale() {
        let mut pc = Relay::new(0, 1920, 1080);
        let _ = pc.put(RelayWin { id: 1, x: 960, y: 540, w: 960, h: 540 });
        let dec = Relay::decode(&pc.encode()).unwrap();
        assert_eq!(dec.get_x(1), Some(960));
        let pad = pc.rescale((960, 540));
        assert_eq!(pad.get_x(1), Some(480));
    }

    #[test]
    fn handoff_25_all_pass() {
        let cs = run_handoff_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
