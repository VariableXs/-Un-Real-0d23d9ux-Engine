//! unx0550_memfence —— UNX-F0550 核间内存序账。
//! 锚点：SMP 卷 · acquire/release 序（发布 / 订阅 / 数据可见性时序）。
//! 判据原文：release 前写全可见于 acquire 后读（发布-订阅链）；
//!           弱序读不得越过 release 前值；序账守恒。

/// 内存单元（单写多读模拟）。
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub val: u64,
    /// 最近一次写入的序号。
    pub seq: u64,
    /// 是否已经 release 过（发布标志）。
    pub released: bool,
}

/// 通信信箱（发布者写 + release，订阅者 acquire 读）。
#[derive(Debug, Default)]
pub struct Mailbox {
    pub cells: alloc::vec::Vec<Cell>,
    /// 发布账 / 订阅账 / 弱读越序拒账。
    pub n_pub: u64,
    pub n_sub: u64,
    pub n_weak: u64,
}

impl Mailbox {
    pub fn with_cells(n: usize) -> Self {
        let cells = (0..n).map(|_| Cell { val: 0, seq: 0, released: false }).collect();
        Mailbox { cells, n_pub: 0, n_sub: 0, n_weak: 0 }
    }

    /// 发布：写值 + release（标记可见）。seq 必须单调。
    pub fn publish(&mut self, idx: usize, val: u64, seq: u64) -> Result<(), ()> {
        let c = self.cells.get_mut(idx).ok_or(())?;
        if seq <= c.seq {
            return Err(()); // 序回退拒
        }
        c.val = val;
        c.seq = seq;
        c.released = true;
        self.n_pub += 1;
        Ok(())
    }

    /// 弱读：未 release 的单元读不到（读到即违序）。
    pub fn read_weak(&mut self, idx: usize) -> Result<u64, ()> {
        let c = self.cells.get(idx).ok_or(())?;
        if !c.released {
            self.n_weak += 1;
            return Err(()); // 不可见
        }
        Ok(c.val)
    }

    /// acquire 读：可见且返回配对的 seq。
    pub fn read_acquire(&mut self, idx: usize) -> Result<(u64, u64), ()> {
        let c = self.cells.get(idx).ok_or(())?;
        if !c.released {
            return Err(());
        }
        self.n_sub += 1;
        Ok((c.val, c.seq))
    }

    /// 未发布计数。
    pub fn unreleased(&self) -> usize {
        self.cells.iter().filter(|c| !c.released).count()
    }
}

/// 判据入口：内存序全性质核。
pub fn run_unx0550_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("UNX-F0550");

    // 族一：未发布不可见。
    let mut m = Mailbox::with_cells(4);
    cs.add("弱读拒", m.read_weak(0).is_err(), "未发布不可见");
    cs.add("acquire 拒", m.read_acquire(0).is_err(), "同步读亦拒");
    cs.add("未发布账", m.unreleased() == 4, "四格全遮");

    // 族二：发布后可见。
    m.publish(0, 0xCAFE, 1).ok();
    cs.add("弱读过", m.read_weak(0) == Ok(0xCAFE), "发布即见");
    let (v, s) = m.read_acquire(0).ok().unwrap_or((0, 0));
    cs.add("acquire 配对", v == 0xCAFE && s == 1, "值 + 序对齐");

    // 族三：序回退拒。
    cs.add("同序拒", m.publish(0, 0xDEAD, 1).is_err(), "seq 须严格增");
    cs.add("回退拒", m.publish(0, 0xDEAD, 0).is_err(), "回退更拒");
    cs.add("增序过", m.publish(0, 0xBEEF, 2).is_ok(), "seq 2 收");
    cs.add("新值可见", m.read_weak(0) == Ok(0xBEEF), "覆盖旧值");

    // 族四：发布-订阅链（多格部分发布）。
    let mut m2 = Mailbox::with_cells(3);
    m2.publish(1, 100, 1).ok();
    m2.publish(2, 200, 1).ok();
    cs.add("格 0 仍遮", m2.read_weak(0).is_err(), "部分发布");
    cs.add("格 1 可见", m2.read_weak(1) == Ok(100), "各自独立");
    cs.add("未发布一", m2.unreleased() == 1, "仅格 0 遮");

    // 族五：越序读账（弱读拒计入）。
    let w0 = m2.n_weak;
    let _ = m2.read_weak(0);
    let _ = m2.read_weak(0);
    cs.add("弱读账单调", m2.n_weak == w0 + 2, "两次遮读两账");

    // 族六：多轮发布序链单调（seq 严格增贯穿）。
    let mut m3 = Mailbox::with_cells(1);
    let mut chain_ok = true;
    for i in 1..=10u64 {
        if m3.publish(0, i * 111, i).is_err() {
            chain_ok = false;
        }
        if m3.read_weak(0) != Ok(i * 111) {
            chain_ok = false;
        }
    }
    cs.add("十轮序链", chain_ok, "seq 1~10 单调");
    cs.add("终值", m3.read_weak(0) == Ok(1110), "最后一写胜出");
    cs.add("账齐", m3.n_pub == 10 && m3.n_sub == 0, "全发布零订阅");

    // 族七：发布-订阅成对核（每发布一次 acquire 一次）。
    let mut m4 = Mailbox::with_cells(2);
    let mut pair_ok = true;
    for i in 1..=6u64 {
        let idx = (i % 2) as usize;
        let val = 1000 + i;
        if m4.publish(idx, val, i).is_err() {
            pair_ok = false;
        }
        match m4.read_acquire(idx) {
            Ok((v, s)) if v == val && s == i => {}
            _ => pair_ok = false,
        }
    }
    cs.add("六轮发布-订阅对", pair_ok, "值-序成对可见");
    cs.add("对账齐", m4.n_pub == 6 && m4.n_sub == 6, "发布订阅对称");
    cs.add("双格全显", m4.unreleased() == 0, "全格已发布");

    // 族八：seq 独立性（两格各自单调互不牵制）。
    let mut m5 = Mailbox::with_cells(2);
    m5.publish(0, 1, 100).ok();
    cs.add("格 1 低序可发", m5.publish(1, 2, 1).is_ok(), "各格独立 seq 空间");
    cs.add("格 0 低序仍拒", m5.publish(0, 9, 99).is_err(), "100 封顶");
    cs.add("格 1 增序通", m5.publish(1, 3, 2).is_ok(), "1→2 单调");

    // 族九：发布覆盖可见性（新值发布后旧值即不可读）。
    let mut m6 = Mailbox::with_cells(1);
    m6.publish(0, 0xAAAA, 1).ok();
    cs.add("首值可见", m6.read_weak(0) == Ok(0xAAAA), "A 态");
    m6.publish(0, 0xBBBB, 2).ok();
    cs.add("覆写后新值", m6.read_weak(0) == Ok(0xBBBB), "B 胜出");
    let (_, s2) = m6.read_acquire(0).ok().unwrap_or((0, 0));
    cs.add("acquire 序随新", s2 == 2, "配对最新序");
    cs.add("旧值无残留接口", m6.cells[0].val == 0xBBBB, "账面只存最新");

    cs
}
