//! F036 深化批次四 · 星卡存储格式面（compatstar2/deep3 · G-A-36）。
//!
//! 批次一~三覆盖草稿生成/脱敏审计/遥测聚合/采样账；本批补齐【功能定义】
//! 「星卡本地留档、用户可导出」全语义对齐的序列化/账本/容错面：记录
//! schema 版本化（v1/v2 字段表——v2 新增 gpu_tier/mem_mb 两字段，v1 记录
//! 读入 → 按默认值补齐迁移，逐字段对拍）、记录级校验和（每条记录 FNV-1a
//! 尾标，损坏检出定位到条目——主册【状态与异常】「导出文件损坏如实报
//! 错」）、字段尺寸预算账（各节字节数与总预算 4KB 对比，超支列明细）、
//! 导出打包（定长帧：头 + 记录区 + 尾标，pack/parse round-trip——MS
//! 文件格式版本化惯例语义对拍）。
//!
//! 判据对账：主册 G-A-36【功能定义】星卡本地留档 +【设计细节】脱敏审计
//! 三类全查无（导出面为字节级取证底座）；FNV-1a 为公开参考参数（非 MS
//! 面——如实注明）。零堆纪律：定长槽表 + 定长帧缓冲，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）

/// schema 版本 v1（初版字段表：id/category/ts）。
pub const SCHEMA_V1: u8 = 1;
/// schema 版本 v2（新增 gpu_tier/mem_mb——主册【设计细节】补 GPU 档位
/// 与内存峰值采样口径）。
pub const SCHEMA_V2: u8 = 2;
/// v2 相对 v1 新增字段数。
pub const V2_NEW_FIELDS: usize = 2;
/// v1 缺省迁移默认值：gpu_tier 0 = 未采样。
pub const DEFAULT_GPU_TIER: u8 = 0;
/// v1 缺省迁移默认值：mem_mb 0 = 未采样。
pub const DEFAULT_MEM_MB: u16 = 0;
/// 记录表容量（域内模型口径）。
pub const MAX_RECORDS: usize = 16;
/// 导出总预算 4KB（诊断中心单卡留档上限——主册【设计细节】）。
pub const TOTAL_BUDGET: usize = 4096;
/// v1 记录序列化字节数：id(2)+category(1)+ts(4)。
pub const V1_REC_BYTES: usize = 7;
/// v2 记录序列化字节数：v1 三字段 + gpu_tier(1)+mem_mb(2)。
pub const V2_REC_BYTES: usize = 10;
/// 记录槽字节数：v2 记录(10) + 逐条 FNV-1a 尾标(4)，对齐 16。
pub const REC_SLOT_BYTES: usize = 16;
/// 帧头字节数：magic(2)+ver(1)+count(1)+reserved(4)。
pub const FRAME_HEADER_BYTES: usize = 8;
/// 帧尾标字节数（FNV-1a 32 位）。
pub const FRAME_TAIL_BYTES: usize = 4;
/// 导出帧总长：头(8)+记录区(16 槽×16B)+尾标(4)。
pub const FRAME_LEN: usize = FRAME_HEADER_BYTES + MAX_RECORDS * REC_SLOT_BYTES + FRAME_TAIL_BYTES;
/// 帧魔数（"SC"——StarCard）。
pub const FRAME_MAGIC: [u8; 2] = [b'S', b'C'];
/// FNV-1a 32 位偏移基（公开参考参数）。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数（公开参考参数）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位（逐字节异或后乘素数——记录级与帧级校验和的公共底座）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ---------------------------------------------------------------------------
// 记录 schema 版本化
/// v1 星卡记录（id/category/ts 三字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordV1 { pub id: u16, pub category: u8, pub ts: u32 }

/// v2 星卡记录（v1 全字段 + gpu_tier/mem_mb 两新增采样字段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordV2 { pub id: u16, pub category: u8, pub ts: u32, pub gpu_tier: u8, pub mem_mb: u16 }

/// v1 → v2 迁移：旧三字段逐一保真，新增两字段按默认值补齐
/// （主册版本化迁移路径——迁移前后逐字段可对拍）。
pub fn migrate_v1_to_v2(r: &RecordV1) -> RecordV2 {
    RecordV2 { id: r.id, category: r.category, ts: r.ts, gpu_tier: DEFAULT_GPU_TIER, mem_mb: DEFAULT_MEM_MB }
}

/// v2 记录序列化（固定字段序：id LE / category / ts LE / gpu_tier / mem_mb LE）。
pub fn encode_v2(r: &RecordV2, out: &mut [u8; V2_REC_BYTES]) {
    out[0] = r.id as u8;
    out[1] = (r.id >> 8) as u8;
    out[2] = r.category;
    out[3] = r.ts as u8;
    out[4] = (r.ts >> 8) as u8;
    out[5] = (r.ts >> 16) as u8;
    out[6] = (r.ts >> 24) as u8;
    out[7] = r.gpu_tier;
    out[8] = r.mem_mb as u8;
    out[9] = (r.mem_mb >> 8) as u8;
}

/// v2 记录反序列化（与 encode_v2 互逆——round-trip 对拍前提）。
pub fn decode_v2(b: &[u8; V2_REC_BYTES]) -> RecordV2 {
    RecordV2 {
        id: b[0] as u16 | ((b[1] as u16) << 8),
        category: b[2],
        ts: b[3] as u32 | ((b[4] as u32) << 8) | ((b[5] as u32) << 16) | ((b[6] as u32) << 24),
        gpu_tier: b[7],
        mem_mb: b[8] as u16 | ((b[9] as u16) << 8),
    }
}

// ---------------------------------------------------------------------------
// 星卡存储模型（逐条校验和 + 损坏定位）

/// 星卡存储模型：定长槽表 + 记录级校验和 + 损坏定位账。
pub struct StarStore {
    slots: [[u8; REC_SLOT_BYTES]; MAX_RECORDS],
    pub count: usize,
    /// 损坏检出定位（首个损坏条目号；无损坏 None——零静默）。
    pub corrupt_at: Option<usize>,
    /// 损坏条目累计数。
    pub corrupt_total: u32,
}

impl StarStore {
    pub const fn new() -> Self {
        StarStore { slots: [[0; REC_SLOT_BYTES]; MAX_RECORDS], count: 0, corrupt_at: None, corrupt_total: 0 }
    }

    /// 写入一条 v2 记录：序列化 + 逐条 FNV-1a 尾标（记录级校验和）。
    pub fn put(&mut self, r: &RecordV2) -> Result<usize, &'static str> {
        if self.count >= MAX_RECORDS {
            return Err("store-full");
        }
        let i = self.count;
        let mut rec = [0u8; V2_REC_BYTES];
        encode_v2(r, &mut rec);
        self.slots[i] = [0; REC_SLOT_BYTES];
        self.slots[i][..V2_REC_BYTES].copy_from_slice(&rec);
        self.slots[i][V2_REC_BYTES..V2_REC_BYTES + 4].copy_from_slice(&fnv1a(&rec).to_le_bytes());
        self.count += 1;
        Ok(i)
    }

    /// 读第 i 条记录的 v2 字段字节（校验通过前提由 verify 保证）。
    pub fn read(&self, i: usize) -> [u8; V2_REC_BYTES] {
        let mut rec = [0u8; V2_REC_BYTES];
        if i < self.count {
            rec.copy_from_slice(&self.slots[i][..V2_REC_BYTES]);
        }
        rec
    }

    /// 全表逐条校验：每条 FNV-1a 尾标重算对拍，损坏定位到条目（返回损坏条数）。
    pub fn verify(&mut self) -> u32 {
        self.corrupt_total = 0;
        self.corrupt_at = None;
        for i in 0..self.count {
            let e = V2_REC_BYTES;
            let h = fnv1a(&self.slots[i][..e]);
            let got = u32::from_le_bytes([self.slots[i][e], self.slots[i][e + 1], self.slots[i][e + 2], self.slots[i][e + 3]]);
            if h != got {
                self.corrupt_total += 1;
                if self.corrupt_at.is_none() {
                    self.corrupt_at = Some(i);
                }
            }
        }
        self.corrupt_total
    }

    /// 导出打包：头 + 记录区 + 尾标（定长帧；同数据两次打包逐字节一致）。
    pub fn pack(&self, frame: &mut [u8; FRAME_LEN]) -> Result<(), &'static str> {
        if frame.len() != FRAME_LEN { return Err("frame-size"); }
        frame[0] = FRAME_MAGIC[0];
        frame[1] = FRAME_MAGIC[1];
        frame[2] = SCHEMA_V2;
        frame[3] = self.count as u8;
        for k in 4..FRAME_HEADER_BYTES {
            frame[k] = 0; // reserved
        }
        for i in 0..MAX_RECORDS {
            let off = FRAME_HEADER_BYTES + i * REC_SLOT_BYTES;
            frame[off..off + REC_SLOT_BYTES].copy_from_slice(&self.slots[i]);
        }
        let tail_off = FRAME_LEN - FRAME_TAIL_BYTES;
        let tail = fnv1a(&frame[..tail_off]).to_le_bytes();
        frame[tail_off..].copy_from_slice(&tail);
        Ok(())
    }

    /// 解析入帧：魔数/版本/计数/尾标全验——损坏显性报错（不静默）。
    pub fn parse(frame: &[u8; FRAME_LEN]) -> Result<StarStore, &'static str> {
        if frame[0] != FRAME_MAGIC[0] || frame[1] != FRAME_MAGIC[1] { return Err("bad-magic"); }
        if frame[2] != SCHEMA_V2 { return Err("bad-version"); }
        let tail_off = FRAME_LEN - FRAME_TAIL_BYTES;
        let expect = fnv1a(&frame[..tail_off]);
        let got = u32::from_le_bytes([frame[tail_off], frame[tail_off + 1], frame[tail_off + 2], frame[tail_off + 3]]);
        if expect != got {
            return Err("tail-mismatch");
        }
        let count = frame[3] as usize;
        if count > MAX_RECORDS {
            return Err("bad-count");
        }
        let mut st = StarStore::new();
        st.count = count;
        for i in 0..MAX_RECORDS {
            let off = FRAME_HEADER_BYTES + i * REC_SLOT_BYTES;
            st.slots[i].copy_from_slice(&frame[off..off + REC_SLOT_BYTES]);
        }
        st.verify();
        if st.corrupt_total > 0 {
            return Err("record-corrupt");
        }
        Ok(st)
    }
}

// ---------------------------------------------------------------------------
// 字段尺寸预算账

/// 预算账单行（节名 + 字节数——超支明细的载体）。
pub struct BudgetRow { pub name: &'static str, pub bytes: usize }

/// 各节字节账 + 合计 + 超支节数（判据：合计 > 4KB 预算即超支；
/// 超支明细随账单行如实列出——不静默吞字节数）。
pub fn budget_ledger(count: usize) -> ([BudgetRow; 3], usize, usize) {
    let rows = [
        BudgetRow { name: "header", bytes: FRAME_HEADER_BYTES },
        BudgetRow { name: "records", bytes: count.saturating_mul(REC_SLOT_BYTES) },
        BudgetRow { name: "tail", bytes: FRAME_TAIL_BYTES },
    ];
    let total = FRAME_HEADER_BYTES + rows[1].bytes + FRAME_TAIL_BYTES;
    let over = usize::from(total > TOTAL_BUDGET);
    (rows, total, over)
}

// ---------------------------------------------------------------------------
// 域自检（深化批次四）

/// 域自检（深化批次四）。
pub fn run_f036f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F036-starstore-d4");
    // 1) schema 版本化常量自洽（v2 = v1+1，新增两字段）。
    cs.add("schema_versioning", SCHEMA_V2 == SCHEMA_V1 + 1 && V2_NEW_FIELDS == 2, "");
    // 2) v1 → v2 迁移逐字段对拍：旧三字段保真 + 两新增按默认值补齐。
    let v1 = RecordV1 { id: 0x1234, category: 7, ts: 1_700_000_000 };
    let v2 = migrate_v1_to_v2(&v1);
    cs.add(
        "v1_v2_migrate",
        v2.id == 0x1234 && v2.category == 7 && v2.ts == 1_700_000_000
            && v2.gpu_tier == DEFAULT_GPU_TIER && v2.mem_mb == DEFAULT_MEM_MB,
        "",
    );
    // 3) v2 编码/解码互逆（round-trip 对拍）。
    let mut enc = [0u8; V2_REC_BYTES];
    encode_v2(&v2, &mut enc);
    cs.add("v2_codec_roundtrip", decode_v2(&enc) == v2, "");
    // 4) 逐条校验和：写入两条后全表通过。
    let mut st = StarStore::new();
    let r2 = RecordV2 { id: 2, category: 1, ts: 99, gpu_tier: 3, mem_mb: 4096 };
    let _ = st.put(&v2);
    let _ = st.put(&r2);
    cs.add("rec_checksum_clean", st.verify() == 0 && st.corrupt_at.is_none(), "");
    // 5) 损坏检出定位到条目（改第 1 条一个字节 → corrupt_at == Some(1)）。
    st.slots[1][3] ^= 0xFF;
    cs.add("rec_corrupt_locate", st.verify() == 1 && st.corrupt_at == Some(1), "");
    // 6) 字段尺寸预算账：16 条整表 268 字节（6.5% 预算），不超支。
    let (rows, total, over) = budget_ledger(MAX_RECORDS);
    cs.add("budget_under", total == 268 && over == 0 && rows[1].bytes == 256, "");
    // 7) 预算超支明细：256 条 → 合计 4108 > 4096，超支节数 1。
    let (_, total_over, over) = budget_ledger(256);
    cs.add("budget_over_detail", total_over == 4108 && over == 1, "");
    // 8) 导出打包确定性：两次 pack 逐字节一致。
    let mut st2 = StarStore::new();
    let _ = st2.put(&v2);
    let _ = st2.put(&r2);
    let mut frame = [0u8; FRAME_LEN];
    let mut frame2 = [0u8; FRAME_LEN];
    cs.add("pack_deterministic", st2.pack(&mut frame).is_ok()
        && st2.pack(&mut frame2).is_ok() && frame == frame2, "");
    // 9) pack/parse round-trip：解析回读逐字段一致。
    let back = StarStore::parse(&frame);
    cs.add(
        "pack_parse_roundtrip",
        matches!(&back, Ok(s) if s.count == 2
            && decode_v2(&s.read(0)) == v2 && decode_v2(&s.read(1)) == r2),
        "",
    );
    // 10) 损坏帧显性报错：记录区翻转 → 帧尾标失配；魔数错 → bad-magic。
    let mut bad = frame;
    bad[FRAME_HEADER_BYTES] ^= 0x01;
    let mut badmagic = frame;
    badmagic[0] = b'X';
    cs.add(
        "parse_explicit_err",
        matches!(StarStore::parse(&bad), Err("tail-mismatch"))
            && matches!(StarStore::parse(&badmagic), Err("bad-magic")),
        "",
    );
    // 11) 表满显性报错（17 条 → Err("store-full")，不静默覆盖）。
    let mut full = StarStore::new();
    let mut ok_all = true;
    for i in 0..MAX_RECORDS {
        ok_all &= full
            .put(&RecordV2 { id: i as u16, category: 0, ts: i as u32, gpu_tier: 0, mem_mb: 0 })
            .is_ok();
    }
    cs.add("store_full_explicit", ok_all && full.put(&r2) == Err("store-full"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_known_vectors() {
        // 公开参考向量：空串 = 偏移基；"a" / "foobar" 为 FNV-1a 32 标准值。
        assert_eq!(fnv1a(&[]), 0x811C_9DC5);
        assert_eq!(fnv1a(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a(b"foobar"), 0xBF9C_F968);
    }

    #[test]
    fn corrupt_record_locates_entry() {
        let mut st = StarStore::new();
        for i in 0..4usize {
            let _ = st.put(&RecordV2 {
                id: i as u16, category: 1, ts: 100 + i as u32, gpu_tier: 2, mem_mb: 512,
            });
        }
        st.slots[3][5] ^= 0x55;
        assert_eq!(st.verify(), 1, "单条损坏只计一处");
        assert_eq!(st.corrupt_at, Some(3), "损坏必须定位到第 3 条");
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f036f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
