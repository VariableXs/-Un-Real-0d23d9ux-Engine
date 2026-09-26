
// ---------------------------------------------------------------------------
// F009 · 深化批次三：恢复点快照（F121 联动——注册表可回滚）+ 并发写 WAL 串行化
//
// 主册依据（G-A-09【设计细节】）：「恢复点（F121）自动纳入蜂巢快照——注册表
// 可回滚」；【数据与存储】「并发写冲突 WAL 串行化」。Hive/Rec/WAL 既有面
// （一处一事实）：快照用既有 record_count/record_at 枚举，回滚用既有 set 回写，
// 本段只做快照/回滚编排与单飞写闸。
// ---------------------------------------------------------------------------

/// 快照键缓冲（KEY_MAX 同源——一处一事实引用既有上限）。
const RESTORE_KEY_CAP: usize = KEY_MAX;
/// 快照值缓冲（超长值如实截断并计数——不静默丢）。
const RESTORE_VAL_CAP: usize = 64;

#[derive(Clone, Copy)]
struct RestoreRec {
    key: [u8; RESTORE_KEY_CAP],
    key_n: usize,
    val: [u8; RESTORE_VAL_CAP],
    val_n: usize,
}

impl RestoreRec {
    fn capture(key: &str, val: &[u8]) -> (RestoreRec, bool) {
        let mut r = RestoreRec { key: [0; RESTORE_KEY_CAP], key_n: 0, val: [0; RESTORE_VAL_CAP], val_n: 0 };
        let kb = key.as_bytes();
        r.key_n = kb.len().min(RESTORE_KEY_CAP);
        r.key[..r.key_n].copy_from_slice(&kb[..r.key_n]);
        r.val_n = val.len().min(RESTORE_VAL_CAP);
        r.val[..r.val_n].copy_from_slice(&val[..r.val_n]);
        (r, val.len() > RESTORE_VAL_CAP)
    }

    fn key_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.key[..self.key_n]).ok()
    }
}

/// 蜂巢恢复点（F121 自动纳入——快照即回滚材料）。
pub struct RestorePoint {
    recs: [Option<RestoreRec>; 48],
    n: usize,
    /// 采集时因槽位/长度限制未能完整快照的记录数（如实登记，不静默）。
    pub truncated_recs: u32,
    pub taken_at_ms: u64,
}

impl RestorePoint {
    pub const fn new(taken_at_ms: u64) -> RestorePoint {
        RestorePoint { recs: [None; 48], n: 0, truncated_recs: 0, taken_at_ms }
    }

    fn push(&mut self, key: &str, val: &[u8]) {
        if self.n >= self.recs.len() {
            self.truncated_recs += 1;
            return;
        }
        let (r, val_trunc) = RestoreRec::capture(key, val);
        if val_trunc {
            self.truncated_recs += 1;
        }
        self.recs[self.n] = Some(r);
        self.n += 1;
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

impl Hive {
    /// 采集恢复点（全量自有记录——回滚 = 逐条 set 回写既有语义）。
    pub fn take_restore_point(&self, at_ms: u64) -> RestorePoint {
        let mut rp = RestorePoint::new(at_ms);
        let total = self.record_count();
        for i in 0..total {
            if let Some(rec) = self.record_at(i) {
                rp.push(rec.key_str(), rec.val_bytes());
            }
        }
        rp
    }
}

/// 回滚：把恢复点逐条回写进蜂巢（成功条数返回；键被截断的记录如实跳过——
/// 回写不了的不装作成功）。
pub fn rollback_from(hive: &mut Hive, point: &RestorePoint) -> usize {
    let mut restored = 0usize;
    for i in 0..point.n {
        if let Some(r) = point.recs[i] {
            if let Some(key) = r.key_str() {
                if hive.set(key, &r.val[..r.val_n]) {
                    restored += 1;
                }
            }
        }
    }
    restored
}

/// 并发写单飞闸（WAL 串行化的进程面模型：同一时刻仅一个写者在途，其余
/// 排队计数——release 时按序放行）。
pub struct WriteSerializer {
    busy: bool,
    pub queued: u32,
    pub serialized: u32,
}

impl WriteSerializer {
    pub const fn new() -> WriteSerializer {
        WriteSerializer { busy: false, queued: 0, serialized: 0 }
    }

    /// 提交写请求：闸空闲 → 占闸（true，写者放行）；占用 → 排队计数（false）。
    pub fn submit(&mut self) -> bool {
        if self.busy {
            self.queued += 1;
            return false;
        }
        self.busy = true;
        true
    }

    /// 在途写者完成：闸释放，排队者按序获得放行权（返回排队放行数）。
    pub fn release(&mut self) -> u32 {
        self.busy = false;
        let pass = self.queued;
        self.queued = 0;
        self.serialized += pass;
        pass
    }
}

/// F009 深化批次三自检。
pub fn run_reghive_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F009-reghive-deep2");
    // 1) 快照-回滚闭环：三笔写入 → 快照 → 再改再删 → 回滚 → 三笔逐条还原
    //    （值一致 = F121「注册表可回滚」判据的模型面）。
    let mut hive = Hive::new(SAMPLE_TEMPLATE);
    hive.set("RPTest\\A", b"alpha");
    hive.set("RPTest\\B", b"beta");
    hive.set("RPTest\\C", b"gamma");
    let point = hive.take_restore_point(1_000);
    hive.set("RPTest\\A", b"CHANGED");
    hive.delete("RPTest\\B");
    hive.set("RPTest\\Extra", b"x");
    let restored = rollback_from(&mut hive, &point);
    let a_ok = matches!(hive.get("RPTest\\A"), Some(r) if r.val_bytes() == b"alpha");
    let b_ok = matches!(hive.get("RPTest\\B"), Some(r) if r.val_bytes() == b"beta");
    let c_ok = matches!(hive.get("RPTest\\C"), Some(r) if r.val_bytes() == b"gamma");
    cs.add(
        "restore_point_rollback_roundtrip",
        point.len() == 3
            && point.truncated_recs == 0
            && restored == 3
            && a_ok
            && b_ok
            && c_ok,
        "",
    );
    // 2) 超长值如实截断计数（不静默丢）：值 100 字节 > 64 缓冲 → truncated_recs +1，
    //    回滚还原前 64 字节。
    let mut hive2 = Hive::new(SAMPLE_TEMPLATE);
    let long = [7u8; 100];
    hive2.set("RPTest\\Long", &long);
    let p2 = hive2.take_restore_point(2_000);
    hive2.set("RPTest\\Long", b"mutated");
    let restored2 = rollback_from(&mut hive2, &p2);
    let back2 = matches!(hive2.get("RPTest\\Long"), Some(r) if r.val_bytes() == &long[..RESTORE_VAL_CAP]);
    cs.add(
        "restore_point_long_value_truncation_honest",
        p2.truncated_recs == 1 && restored2 == 1 && back2,
        "",
    );
    // 3) 单飞写闸：占用期提交 = 排队；release 放行排队者；串行化计数可审计。
    let mut ser = WriteSerializer::new();
    let first = ser.submit();
    let second = ser.submit();
    let third = ser.submit();
    let passed = ser.release();
    cs.add(
        "write_serializer_single_flight",
        first && !second && !third && passed == 2 && ser.serialized == 2 && !ser.busy,
        "",
    );
    cs
}
