
// ---------------------------------------------------------------------------
// F009 · 深化批次八：蜂巢压实记账面（删除后的空洞回收——B 树原地删除是
// 既有面；本段补「压实把记录前移、回收空洞字节」的模拟核：压实后容量
// 占用连续、每条记录字节内容逐字节保真）。
// ---------------------------------------------------------------------------

/// 蜂巢页压实模型（页内记录槽，删后留空洞；压实 = 前移保序）。
pub struct HivePage {
    slots: [Option<alloc::vec::Vec<u8>>; 8],
    pub compactions: u32,
}

impl HivePage {
    pub fn new() -> HivePage {
        HivePage { slots: Default::default(), compactions: 0 }
    }

    pub fn put(&mut self, idx: usize, rec: &[u8]) -> bool {
        if idx >= 8 || self.slots[idx].is_some() {
            return false;
        }
        self.slots[idx] = Some(rec.to_vec());
        true
    }

    pub fn remove(&mut self, idx: usize) -> bool {
        if idx >= 8 {
            return false;
        }
        self.slots[idx].take().is_some()
    }

    /// 空洞数（None 槽位于有记录槽之后才算尾洞，中间 None = 碎片）。
    pub fn fragments(&self) -> usize {
        let last = self.slots.iter().rposition(|s| s.is_some()).map_or(0, |i| i + 1);
        (0..last).filter(|&i| self.slots[i].is_none()).count()
    }

    /// 压实：所有记录保序前移。返回移动条数（无碎片时 0 且不计数）。
    pub fn compact(&mut self) -> usize {
        let mut write = 0usize;
        let mut moved = 0usize;
        for read in 0..8 {
            if let Some(rec) = self.slots[read].take() {
                if write != read {
                    moved += 1;
                }
                self.slots[write] = Some(rec);
                write += 1;
            }
        }
        if moved > 0 {
            self.compactions += 1;
        }
        moved
    }
}

/// F009 深化批次八自检。
fn run_reghive_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep7");
    let mut page = HivePage::new();
    for (i, name) in [b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec(), b"delta".to_vec()]
        .into_iter()
        .enumerate()
    {
        let _ = page.put(i * 2, &name); // 0/2/4/6 有记录，1/3/5 为碎片
    }
    // 1) 中间空洞 = 碎片 3；删除首条后碎片仍 3（0/2/4 三个洞）。
    let frag0 = page.fragments();
    let _ = page.remove(0);
    let frag1 = page.fragments();
    cs.add(
        "fragments_counted_honestly",
        frag0 == 3 && frag1 == 3,
        "",
    );
    // 2) 压实：保序前移，内容逐字节保真，空洞只剩尾洞。
    let moved = page.compact();
    let ok = page.slots.iter().filter(|s| s.is_some()).count() == 3
        && page.slots[0].as_deref() == Some(b"beta".as_slice())
        && page.slots[1].as_deref() == Some(b"gamma".as_slice())
        && page.slots[2].as_deref() == Some(b"delta".as_slice())
        && page.fragments() == 0;
    cs.add(
        "compact_preserves_order_and_bytes",
        moved == 3 && ok && page.compactions == 1,
        "",
    );
    // 3) 无碎片压实 = 不动不计数（零噪声纪律）。
    let moved2 = page.compact();
    cs.add(
        "compact_noop_when_dense",
        moved2 == 0 && page.compactions == 1,
        "",
    );
    cs
}
