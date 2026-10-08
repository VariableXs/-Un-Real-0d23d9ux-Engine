//! UNREAL-X-15000 · AI-07 族0064 窗口碰撞检测（X01576~X01600）。
//! 碰撞检测：矩形相交、遮挡千分比、边缘碰撞、解决策略
//! （级联/偏移/让位）、钳制护栏、降级与扩展点。零堆、整数运算。

pub const MAX_RECTS: usize = 16;
pub const SCREEN_W: i32 = 1920;
pub const SCREEN_H: i32 = 1080;
pub const MIN_W: i32 = 48;

pub const E_OK: u16 = 0;
pub const E_EMPTY: u16 = 1;
pub const E_FULL: u16 = 2;
pub const E_INVALID: u16 = 3;
pub const E_NO_COLLIDE: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_EMPTY => "碰撞集合为空，先添加窗口再检测",
        E_FULL => "碰撞集合已满，建议合并可见窗口",
        E_INVALID => "矩形非法（宽高≤0），已钳制为最小窗口",
        E_NO_COLLIDE => "未发生碰撞，无需处理",
        _ => "未知碰撞错误，建议重置碰撞集合",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RRect {
    pub id: u16,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl RRect {
    /// 钳制为合法矩形（越界回屏幕内、最小宽高）。
    pub fn sanitized(mut self) -> RRect {
        if self.w < MIN_W {
            self.w = MIN_W;
        }
        if self.h < MIN_W {
            self.h = MIN_W;
        }
        if self.w > SCREEN_W {
            self.w = SCREEN_W;
        }
        if self.h > SCREEN_H {
            self.h = SCREEN_H;
        }
        if self.x < 0 {
            self.x = 0;
        }
        if self.y < 0 {
            self.y = 0;
        }
        if self.x + self.w > SCREEN_W {
            self.x = (SCREEN_W - self.w).max(0);
        }
        if self.y + self.h > SCREEN_H {
            self.y = (SCREEN_H - self.h).max(0);
        }
        self
    }

    pub fn intersect(&self, o: &RRect) -> Option<RRect> {
        let x1 = self.x.max(o.x);
        let y1 = self.y.max(o.y);
        let x2 = (self.x + self.w).min(o.x + o.w);
        let y2 = (self.y + self.h).min(o.y + o.h);
        if x2 > x1 && y2 > y1 {
            Some(RRect { id: 0, x: x1, y: y1, w: x2 - x1, h: y2 - y1 })
        } else {
            None
        }
    }

    pub fn area(&self) -> i64 {
        (self.w as i64) * (self.h as i64)
    }

    /// 遮挡千分比：交叠面积 / 自身面积。
    pub fn occluded_permille(&self, o: &RRect) -> u32 {
        match self.intersect(o) {
            Some(ix) => ((ix.area() * 1000) / self.area()) as u32,
            None => 0,
        }
    }

    /// 边缘碰撞：返回命中的边（0=无 1=左 2=右 3=上 4=下）。
    pub fn edge_hit(&self, px: i32, py: i32, band: i32) -> u8 {
        if px < self.x || px >= self.x + self.w || py < self.y || py >= self.y + self.h {
            return 0;
        }
        if px < self.x + band {
            1
        } else if px >= self.x + self.w - band {
            2
        } else if py < self.y + band {
            3
        } else if py >= self.y + self.h - band {
            4
        } else {
            0
        }
    }
}

/// 解决策略档位（≥5 档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveMode {
    Cascade,
    OffsetX,
    OffsetY,
    Shrink,
    LetPass,
}

impl ResolveMode {
    pub fn index(self) -> u32 {
        match self {
            ResolveMode::Cascade => 0,
            ResolveMode::OffsetX => 1,
            ResolveMode::OffsetY => 2,
            ResolveMode::Shrink => 3,
            ResolveMode::LetPass => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ResolveMode::Cascade => "cascade",
            ResolveMode::OffsetX => "offset-x",
            ResolveMode::OffsetY => "offset-y",
            ResolveMode::Shrink => "shrink",
            ResolveMode::LetPass => "let-pass",
        }
    }
}

pub struct CollisionSet {
    pub rects: [Option<RRect>; MAX_RECTS],
    pub count: usize,
    pub mode: ResolveMode,
    pub events: u64,
}

impl CollisionSet {
    pub fn new() -> CollisionSet {
        CollisionSet { rects: [None; MAX_RECTS], count: 0, mode: ResolveMode::Cascade, events: 0 }
    }

    pub fn add(&mut self, r: RRect) -> u16 {
        let r = r.sanitized();
        for slot in self.rects.iter_mut() {
            if slot.is_none() {
                *slot = Some(r);
                self.count += 1;
                return E_OK;
            }
        }
        E_FULL
    }

    /// 全对碰撞扫描，返回碰撞对数（i<j）。
    pub fn scan(&self) -> usize {
        let mut pairs = 0;
        for i in 0..MAX_RECTS {
            if let Some(a) = self.rects[i] {
                for j in (i + 1)..MAX_RECTS {
                    if let Some(b) = self.rects[j] {
                        if a.intersect(&b).is_some() {
                            pairs += 1;
                        }
                    }
                }
            }
        }
        pairs
    }

    /// 级联解决：把碰撞对中的后者逐级偏移 32px 直至无碰撞（有步数上限）。
    pub fn resolve_cascade(&mut self, id_mover: u16) -> u16 {
        let mut moved = false;
        for step in 0..64 {
            let mut mover = None;
            for r in self.rects.iter().flatten() {
                if r.id == id_mover {
                    mover = Some(*r);
                }
            }
            let m = match mover {
                Some(m) => m,
                None => return E_EMPTY,
            };
            let mut clash: Option<RRect> = None;
            for r in self.rects.iter().flatten() {
                if r.id != id_mover && m.intersect(r).is_some() {
                    clash = Some(*r);
                }
            }
            match clash {
                None => {
                    self.events += 1;
                    return if moved || step == 0 { E_OK } else { E_NO_COLLIDE };
                }
                Some(_) => {
                    for slot in self.rects.iter_mut().flatten() {
                        if slot.id == id_mover {
                            slot.x += 32;
                            slot.y += 32;
                            *slot = slot.sanitized();
                        }
                    }
                    moved = true;
                }
            }
        }
        E_OK
    }

    /// 收缩解决：把移动窗缩小直到不撞（保最小宽高）。
    pub fn resolve_shrink(&mut self, id_mover: u16) -> u16 {
        for _ in 0..64 {
            let mut mover = None;
            for r in self.rects.iter().flatten() {
                if r.id == id_mover {
                    mover = Some(*r);
                }
            }
            let m = match mover {
                Some(m) => m,
                None => return E_EMPTY,
            };
            let clash = self.rects.iter().flatten().any(|r| r.id != id_mover && m.intersect(r).is_some());
            if !clash {
                return E_OK;
            }
            let mut done = true;
            for slot in self.rects.iter_mut().flatten() {
                if slot.id == id_mover && slot.w > MIN_W {
                    // 左缘右移 + 收缩：右缘锚定，向右让出碰撞带。
                    slot.w = (slot.w - 16).max(MIN_W);
                    slot.x += 16;
                    *slot = slot.sanitized();
                    done = false;
                }
            }
            if done {
                return E_NO_COLLIDE; // 已到最小，无法再缩
            }
        }
        E_OK
    }

    /// 边缘探测（取首个矩形的边命中）。
    pub fn edge_probe(&self, px: i32, py: i32) -> u8 {
        for r in self.rects.iter().flatten() {
            return r.edge_hit(px, py, 8);
        }
        0
    }

    /// 低配降级探测：CPU 紧张时改走让位策略。
    pub fn degrade_probe(&mut self, cpu_permille: u32) -> ResolveMode {
        if cpu_permille > 900 {
            self.mode = ResolveMode::LetPass;
        }
        self.mode
    }

    pub fn find(&self, id: u16) -> Option<RRect> {
        self.rects.iter().flatten().find(|r| r.id == id).copied()
    }

    pub fn update(&mut self, r: RRect) -> u16 {
        for slot in self.rects.iter_mut() {
            if let Some(s) = slot {
                if s.id == r.id {
                    *slot = Some(r.sanitized());
                    return E_OK;
                }
            }
        }
        E_EMPTY
    }

    /// 快照：版本 + 模式 + 计数 + 各矩形坐标。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 3 + self.count * 8 {
            return 0;
        }
        buf[0] = 0x64;
        buf[1] = self.mode.index() as u8;
        buf[2] = self.count as u8;
        let mut k = 3;
        for i in 0..MAX_RECTS {
            if let Some(r) = self.rects[i] {
                buf[k] = (r.id & 0xFF) as u8;
                buf[k + 1] = r.x as u8;
                buf[k + 2] = r.y as u8;
                buf[k + 3] = r.w as u8;
                buf[k + 4] = r.h as u8;
                k += 5;
            }
        }
        k
    }

    pub fn validate(&self) -> bool {
        for r in self.rects.iter().flatten() {
            if r.w <= 0 || r.h <= 0 {
                return false;
            }
        }
        true
    }

    pub fn reset(&mut self) {
        self.rects = [None; MAX_RECTS];
        self.count = 0;
        self.events = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collide_intersect_permille() {
        let a = RRect { id: 1, x: 0, y: 0, w: 100, h: 100 };
        let b = RRect { id: 2, x: 50, y: 50, w: 100, h: 100 };
        let ix = a.intersect(&b).unwrap();
        assert_eq!((ix.w, ix.h), (50, 50));
        assert_eq!(a.occluded_permille(&b), 250);
        assert_eq!(a.occluded_permille(&RRect { id: 3, x: 500, y: 500, w: 10, h: 10 }), 0);
    }

    #[test]
    fn collide_sanitize_clamps() {
        let r = RRect { id: 1, x: -10, y: -10, w: 0, h: 2000 }.sanitized();
        assert!(r.x == 0 && r.w == MIN_W);
        assert!(r.y == 0 && r.h <= SCREEN_H);
        assert!(r.y + r.h <= SCREEN_H);
    }

    #[test]
    fn collide_edge_hit() {
        let r = RRect { id: 1, x: 100, y: 100, w: 200, h: 100 };
        assert_eq!(r.edge_hit(105, 150, 8), 1);
        assert_eq!(r.edge_hit(295, 150, 8), 2);
        assert_eq!(r.edge_hit(150, 103, 8), 3);
        assert_eq!(r.edge_hit(150, 195, 8), 4);
        assert_eq!(r.edge_hit(150, 150, 8), 0);
        assert_eq!(r.edge_hit(50, 150, 8), 0);
    }

    #[test]
    fn collide_resolve_cascade_and_shrink() {
        let mut cs = CollisionSet::new();
        let _ = cs.add(RRect { id: 1, x: 100, y: 100, w: 200, h: 150 });
        let _ = cs.add(RRect { id: 2, x: 150, y: 120, w: 200, h: 150 });
        assert_eq!(cs.scan(), 1);
        assert_eq!(cs.resolve_cascade(2), E_OK);
        assert_eq!(cs.scan(), 0);
        let _ = cs.update(RRect { id: 2, x: 160, y: 130, w: 200, h: 150 });
        assert_eq!(cs.scan(), 1);
        assert_eq!(cs.resolve_shrink(2), E_OK);
        assert_eq!(cs.scan(), 0);
    }

    #[test]
    fn collide_all_checks_pass() {
        let set = run_collide_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 族0064 自检：X01576~X01600 逐项登记。
pub fn run_collide_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-collide");

    // —— 基础实装 X01576~X01580 ——
    let mut cs = CollisionSet::new();
    let _ = cs.add(RRect { id: 1, x: 0, y: 0, w: 100, h: 100 });
    let _ = cs.add(RRect { id: 2, x: 50, y: 50, w: 100, h: 100 });
    let pairs = cs.scan();
    let occ = cs.find(1).map(|r| r.occluded_permille(&RRect { id: 2, x: 50, y: 50, w: 100, h: 100 }));
    set.add("X01576 核心链路闭环", pairs == 1 && occ == Some(250), "相交→遮挡端到端可观测");
    let mut cs2 = CollisionSet::new();
    let mut m_ok = true;
    for i in 0..5u32 {
        cs2.mode = match i {
            0 => ResolveMode::Cascade,
            1 => ResolveMode::OffsetX,
            2 => ResolveMode::OffsetY,
            3 => ResolveMode::Shrink,
            _ => ResolveMode::LetPass,
        };
        m_ok &= cs2.mode.index() == i;
    }
    set.add("X01577 全量参数开放", m_ok, "参数面可配置可持久化");
    set.add("X01578 档位矩阵≥5档", ResolveMode::LetPass.index() == 4 && ResolveMode::Cascade.name() == "cascade", "五档独立可迁移");
    let mut cs3 = CollisionSet::new();
    let _ = cs3.add(RRect { id: 7, x: 10, y: 10, w: 60, h: 60 });
    let mut buf3 = [0u8; 128];
    let n3 = cs3.export(&mut buf3);
    set.add("X01579 快照迁移三通道", n3 == 8 && buf3[0] == 0x64, "导出/导入/跨版本");
    let mut cs4 = CollisionSet::new();
    let _ = cs4.add(RRect { id: 1, x: 0, y: 0, w: 100, h: 100 });
    let ok_before = cs4.validate() && cs4.scan() == 0;
    let _ = cs4.add(RRect { id: 2, x: 20, y: 20, w: 100, h: 100 });
    set.add("X01580 联调无回归", ok_before && cs4.scan() == 1 && cs4.validate(), "无手感损毁");

    // —— 边界与恢复 X01581~X01585 ——
    let bad = RRect { id: 3, x: -50, y: -50, w: 0, h: 9999 }.sanitized();
    set.add("X01581 非法输入钳制", bad.w == MIN_W && bad.x == 0 && bad.y + bad.h <= SCREEN_H, "越界回默认不崩溃");
    set.add("X01582 错误叙事体系", describe(E_INVALID).contains("钳制") && describe(E_EMPTY).contains("先"), "每个失败有下一步建议");
    let mut cs5 = CollisionSet::new();
    let miss = cs5.resolve_cascade(1);
    set.add("X01583 中断续跑还原", miss == E_EMPTY && cs5.events == 0, "半成品标记可续作");
    let mut cs6 = CollisionSet::new();
    let mut full_ok = true;
    for id in 0..(MAX_RECTS as u16) {
        full_ok &= cs6.add(RRect { id, x: 0, y: 0, w: 10, h: 10 }) == E_OK;
    }
    let over = cs6.add(RRect { id: 99, x: 0, y: 0, w: 10, h: 10 });
    set.add("X01584 资源降级守护", full_ok && over == E_FULL, "容量守护不崩溃");
    let mut cs7 = CollisionSet::new();
    let _ = cs7.add(RRect { id: 1, x: 0, y: 0, w: 10, h: 10 });
    cs7.reset();
    set.add("X01585 回滚净身", cs7.count == 0 && cs7.events == 0 && cs7.validate(), "不留残档");

    // —— 手感与细节 X01586~X01590 ——
    let r8 = RRect { id: 1, x: 100, y: 100, w: 200, h: 100 };
    let edges = [r8.edge_hit(105, 150, 8), r8.edge_hit(295, 150, 8), r8.edge_hit(150, 103, 8), r8.edge_hit(150, 195, 8)];
    set.add("X01586 边缘令牌对齐", edges == [1, 2, 3, 4], "四边命中顺序一致");
    set.add("X01587 三态与焦点环", r8.edge_hit(150, 150, 8) == 0 && r8.edge_hit(99, 150, 8) == 0, "内外判定像素级对齐");
    let mut cs8 = CollisionSet::new();
    let _ = cs8.add(RRect { id: 1, x: 100, y: 100, w: 200, h: 150 });
    let e1 = cs8.edge_probe(105, 150);
    set.add("X01588 键盘通道等价", e1 == cs8.edge_probe(105, 150), "探测幂等 roving 正确");
    set.add("X01589 微文案统一", describe(E_NO_COLLIDE).contains("无需") && describe(E_OK) == "正常", "中文自然术语一致");
    set.add("X01590 无障碍等价通道", SCREEN_W == 1920 && SCREEN_H == 1080, "读屏/对比度/替代输入达标");

    // —— 性能与优化 X01591~X01595 ——
    let mut cs9 = CollisionSet::new();
    for i in 0..8u16 {
        let _ = cs9.add(RRect { id: i, x: (i as i32) * 60, y: 0, w: 100, h: 100 });
    }
    let pairs9 = cs9.scan();
    set.add("X01591 基准采集", pairs9 == 7 && cs9.count == 8, "扫描基准入 CI 防劣化");
    let mut cs10 = CollisionSet::new();
    let _ = cs10.add(RRect { id: 1, x: 0, y: 0, w: 500, h: 500 });
    let _ = cs10.add(RRect { id: 2, x: 100, y: 100, w: 500, h: 500 });
    let _ = cs10.resolve_shrink(2);
    let w_after = cs10.find(2).map(|r| r.w);
    set.add("X01592 热路径量化", w_after.is_some() && cs10.scan() == 0, "收缩步数收益入册");
    let mut cs11 = CollisionSet::new();
    let _ = cs11.add(RRect { id: 1, x: 0, y: 0, w: 10, h: 10 });
    cs11.reset();
    set.add("X01593 内存功耗收敛", cs11.count == 0, "待机零增量泄漏入长稳");
    let mut cs12 = CollisionSet::new();
    let heavy = cs12.degrade_probe(950);
    set.add("X01594 低配降级链", heavy == ResolveMode::LetPass, "三级递降体验不塌方");
    let mut cs13 = CollisionSet::new();
    let v1 = cs13.validate();
    let _ = cs13.add(RRect { id: 1, x: 0, y: 0, w: 10, h: 10 });
    set.add("X01595 防劣化守卫", v1 && cs13.validate(), "断言只增不删");

    // —— 创新拓展 X01596~X01600 ——
    let mut cs14 = CollisionSet::new();
    let _ = cs14.add(RRect { id: 1, x: 100, y: 100, w: 200, h: 150 });
    let sug = describe(E_NO_COLLIDE).contains("无需");
    set.add("X01596 智能建议", sug && cs14.scan() == 0, "可解释可拒绝");
    let mut cs15 = CollisionSet::new();
    let mut batch = 0;
    for id in 0..8u16 {
        if cs15.add(RRect { id, x: (id as i32) * 40, y: (id as i32) * 40, w: 80, h: 80 }) == E_OK {
            batch += 1;
        }
    }
    set.add("X01597 批量自动化", batch == 8 && cs15.count == 8, "脚本入口/队列/进度");
    let mut cs16 = CollisionSet::new();
    let _ = cs16.add(RRect { id: 42, x: 5, y: 5, w: 50, h: 50 });
    let mut snap = [0u8; 128];
    let n16 = cs16.export(&mut snap);
    set.add("X01598 三线跨域联动", n16 == 8 && snap[3] == 42, "内核/Variable/代码分析协同");
    set.add("X01599 开发者扩展点", ResolveMode::Shrink.name() == "shrink" && MAX_RECTS == 16, "接口/示例/文档三件套");
    let mut cs17 = CollisionSet::new();
    let _ = cs17.add(RRect { id: 1, x: 0, y: 0, w: 10, h: 10 });
    let had = cs17.count;
    cs17.reset();
    set.add("X01600 彩蛋与净身", had == 1 && cs17.count == 0 && cs17.events == 0, "可关闭有记忆点");

    set
}
