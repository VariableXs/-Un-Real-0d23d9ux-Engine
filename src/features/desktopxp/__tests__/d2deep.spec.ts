/**
 * C 桌面体验域·后段 AI-D2（F093-F110）前端逻辑单测（深化面）。
 * 覆盖：F093 缩略图缓存/队列 / F094 媒体解析与悬停 / F095 终端核心 /
 * F096 命令面板。判据数字逐条钉死（与内核模型面 K 口径同源）。
 */

import { describe, expect, it } from "vitest";
import {
  ThumbCache,
  ThumbQueue,
  thumbKey,
  progressivePlan,
  QUEUE_CAP,
  CACHE_CAP_BYTES,
} from "../thumbeng";
import {
  MediaInfoStore,
  HoverScheduler,
  sniffAndParse,
  parseMkv,
  parseFlac,
  parseMp3,
  durationLabel,
  resolutionLabel,
  fnv1a64,
  HOVER_DELAY_MS,
  CACHE_HIT_LINE_MS,
  HEAD_BYTES,
} from "../mediainfo";
import {
  Scrollback,
  charWidth,
  lineWidth,
  frameAccount,
  layoutTree,
  splitPane,
  closePane,
  leafIds,
  leafCount,
  dividerHit,
  dragDivider,
  newPane,
  resetPaneIds,
  SCROLLBACK_LINES,
  SCROLLBACK_BYTES,
  COST_PER_CELL_US,
  FRAME_BUDGET_US,
  TRUNCATION_NOTICE,
  FONT_SIZE_STEPS,
} from "../termcore";
import {
  Palette,
  Favorites,
  fuzzyScore,
  highlightIndices,
  BUILTINS,
  BUILTIN_COUNT,
  FAVORITE_CAP,
  OPEN_BUDGET_MS,
  templateVars,
  fillTemplate,
} from "../termpalette";

/* ------------------------------ F093 缩略图 ------------------------------ */

describe("F093 缩略图缓存面", () => {
  it("键：同文件同尺寸稳定，尺寸/时刻变化即换键", () => {
    expect(thumbKey("a.png", 100, 1000)).toBe(thumbKey("a.png", 100, 1000));
    expect(thumbKey("a.png", 100, 1000)).not.toBe(thumbKey("a.png", 200, 1000));
    expect(thumbKey("a.png", 100, 1000)).not.toBe(thumbKey("b.png", 100, 1000));
  });

  it("2GB LRU 上限生效（判据线）+ 逐出计数", () => {
    const cache = new ThumbCache(1000); // 测试用小上限，机制同 2GB
    expect(CACHE_CAP_BYTES).toBe(2 * 1024 * 1024 * 1024);
    for (let i = 0; i < 20; i++) cache.put(`k${i}`, 100);
    for (let i = 20; i < 40; i++) cache.put(`k${i}`, 100);
    expect(cache.bytes).toBeLessThanOrEqual(1000);
    expect(cache.evictions).toBeGreaterThan(0);
  });

  it("热库增量命中率（K5 口径）：窗口核算冷启动 miss 不混算，二批 >95%", () => {
    const cache = new ThumbCache();
    for (let i = 0; i < 500; i++) {
      if (!cache.touch(`t${i}`)) cache.put(`t${i}`, 24 * 1024);
    }
    cache.startWindow(); // 热窗口从二次浏览起算
    for (let i = 0; i < 500; i++) cache.touch(`t${i}`);
    expect(cache.hitRateWindow()).toBeGreaterThan(0.95); // 二次浏览判据
    expect(cache.hitRateWindow()).toBe(1); // 二批全命中
  });

  it("损坏即弃自愈：弃后重生成", () => {
    const cache = new ThumbCache();
    cache.put("bad", 500);
    cache.discard("bad");
    expect(cache.has("bad")).toBe(false);
    expect(cache.discards).toBe(1);
  });

  it("三级优先队列：可视先出、满载诚实拒绝、幂等入队", () => {
    const q = new ThumbQueue();
    q.enqueue("bg", 2);
    q.enqueue("near", 1);
    q.enqueue("vis", 0);
    expect(q.dequeue()).toBe("vis");
    expect(q.dequeue()).toBe("near");
    expect(q.dequeue()).toBe("bg");
    const full = new ThumbQueue();
    for (let i = 0; i < QUEUE_CAP; i++) full.enqueue(`k${i}`, 0);
    expect(full.enqueue("overflow", 0)).toBe(false); // 诚实拒绝
    expect(full.rejected).toBe(1);
    expect(full.enqueue("k0", 0)).toBe(true); // 幂等
  });

  it("渐进占位策略：可视直出全清，背景低清先行", () => {
    expect(progressivePlan(0, true)).toEqual({ placeholder: false, target: "full" });
    expect(progressivePlan(2, true)).toEqual({ placeholder: true, target: "low" });
    expect(progressivePlan(2, false)).toEqual({ placeholder: false, target: "full" });
  });
});

/* ------------------------------ F094 媒体信息 ------------------------------ */

/** 构造含视频轨的 MP4（tkhd 几何 + mdhd timescale + stbl/stts 帧序）→ fps 联算样本。 */
function buildSampleMp4VideoFps(): Uint8Array {
  const boxOf = (type: string, body: Uint8Array): Uint8Array => {
    const out = new Uint8Array(8 + body.length);
    const dv = new DataView(out.buffer);
    dv.setUint32(0, out.length);
    for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
    out.set(body, 8);
    return out;
  };
  const ftyp = boxOf("ftyp", new Uint8Array([0x69, 0x73, 0x6f, 0x6d, 0, 0, 2, 0, 0x69, 0x73, 0x6f, 0x6d]));
  const mvhd = boxOf("mvhd", (() => { const b = new Uint8Array(20); new DataView(b.buffer).setUint32(12, 12800); new DataView(b.buffer).setUint32(16, 20_000); return b; })());
  const tkhdBody = new Uint8Array(84);
  new DataView(tkhdBody.buffer).setUint32(76, 1920 << 16);
  new DataView(tkhdBody.buffer).setUint32(80, 1080 << 16);
  const tkhd = boxOf("tkhd", tkhdBody);
  // mdhd v0：timescale 在 body+12。
  const mdhdBody = new Uint8Array(24);
  new DataView(mdhdBody.buffer).setUint32(12, 12800);
  const mdhd = boxOf("mdhd", mdhdBody);
  // stts：1 条目 250 帧 × 512 单位 → 128_000 单位 = 10s @ 12800。
  const sttsBody = new Uint8Array(8 + 8);
  new DataView(sttsBody.buffer).setUint32(4, 1); // entry_count
  new DataView(sttsBody.buffer).setUint32(8, 250); // sample_count
  new DataView(sttsBody.buffer).setUint32(12, 512); // sample_delta
  const stts = boxOf("stts", sttsBody);
  const stbl = boxOf("stbl", stts);
  const minf = boxOf("minf", stbl);
  const mdia = boxOf("mdia", concatBytes([mdhd, minf]));
  const trak = boxOf("trak", concatBytes([tkhd, mdia]));
  const moov = boxOf("moov", concatBytes([mvhd, trak]));
  return concatBytes([ftyp, moov]);
}

function concatBytes(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((a, p) => a + p.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const p of parts) { out.set(p, o); o += p.length; }
  return out;
}

/** 构造最小合法 MP4 头（ftyp + moov/mvhd v0——与内核面 D8/D9 修法同源）。 */
function buildSampleMp4(durationSec = 12, timescale = 1000): Uint8Array {
  const box = (type: string, body: Uint8Array): Uint8Array => {
    const out = new Uint8Array(8 + body.length);
    const dv = new DataView(out.buffer);
    dv.setUint32(0, out.length);
    for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
    out.set(body, 8);
    return out;
  };
  const ftyp = box("ftyp", new Uint8Array([0x69, 0x73, 0x6f, 0x6d, 0, 0, 2, 0, 0x69, 0x73, 0x6f, 0x6d]));
  // mvhd v0：ver/flags(4) + ctime(4) + mtime(4) + timescale(4) + duration(4)。
  const mvhdBody = new Uint8Array(4 + 4 + 4 + 4 + 4);
  const dv = new DataView(mvhdBody.buffer);
  dv.setUint32(12, timescale);
  dv.setUint32(16, durationSec * timescale);
  const mvhd = box("mvhd", mvhdBody);
  const moov = box("moov", mvhd);
  const out = new Uint8Array(ftyp.length + moov.length);
  out.set(ftyp, 0);
  out.set(moov, ftyp.length);
  return out;
}

/** 构造最小 FLAC（fLaC + STREAMINFO）——位布局与解析器同规：
 *  字节 10-12 = 采样率 17 位 + 声道 3 位 + 位深高 2 位（byte12 低 4 位中
 *  3 位是声道、4 位里 1 位属采样率最低位——按位段精确放置）。 */
function buildSampleFlac(sampleRate = 44100, totalSamples = 44100 * 30, channels = 2): Uint8Array {
  const out = new Uint8Array(42);
  out.set([0x66, 0x4c, 0x61, 0x43], 0); // fLaC
  out[4] = 0; // STREAMINFO 块头（最后一块）
  const bpsMinus1 = 15; // 16 位
  out[8 + 10] = (sampleRate >> 9) & 0xff;
  out[8 + 11] = (sampleRate >> 1) & 0xff;
  out[8 + 12] = ((sampleRate & 1) << 7) | ((channels - 1) << 4) | ((bpsMinus1 >> 1) & 0xf);
  out[8 + 13] = ((bpsMinus1 & 1) << 7) | (Math.floor(totalSamples / 2 ** 32) & 0x7f);
  const dv = new DataView(out.buffer);
  dv.setUint32(8 + 14, totalSamples >>> 0);
  return out;
}

function parseAnd(info: ReturnType<typeof sniffAndParse>): NonNullable<typeof info> {
  expect(info).not.toBeNull();
  return info!;
}

describe("F094 媒体信息解析", () => {
  it("MP4：ftyp 嗅探 + mvhd 时长实算", () => {
    const data = buildSampleMp4(12, 1000);
    const info = parseAnd(sniffAndParse(data, 1024 * 1024));
    expect(info.container).toBe("mp4");
    expect(info.durationMs).toBe(12_000);
    expect(info.fps).toBeNull(); // 无视频轨 → fps 诚实留白
  });

  it("MP4 fps 联算（stts × mdhd timescale —— 25fps 帧序）", () => {
    const info = parseAnd(sniffAndParse(buildSampleMp4VideoFps(), 8_000_000));
    expect(info.width).toBe(1920);
    expect(info.height).toBe(1080);
    expect(info.fps).toBe(25); // 250 帧 × 12800 ÷ (250×512) = 25
  });

  it("FLAC：STREAMINFO 位拆（采样率/声道/时长）", () => {
    const info = parseAnd(parseFlac(buildSampleFlac(44100, 44100 * 30, 2)));
    expect(info.container).toBe("flac");
    expect(info.sampleRate).toBe(44100);
    expect(info.channels).toBe(2);
    expect(info.durationMs).toBe(30_000);
  });

  it("MP3：ID3 跳过 + 帧头 + CBR 时长兜底", () => {
    // 构造 32kbps/44100 MPEG1 L3 帧头（bitrateIdx=1, srIdx=0, stereo）。
    const data = new Uint8Array(512);
    data.set([0x49, 0x44, 0x33], 0); // "ID3"
    data[6] = 0;
    data[7] = 0;
    data[8] = 0;
    data[9] = 0; // ID3 大小 0
    data[10] = 0xff;
    data[11] = 0xfb; // MPEG1 L3 no CRC
    data[12] = 0x10; // bitrateIdx=1(32kbps) srIdx=0(44100) padding=0
    const info = parseAnd(parseMp3(data, 128_000));
    expect(info.container).toBe("mp3");
    expect(info.sampleRate).toBe(44100);
    expect(info.durationMs).toBe(Math.round((128_000 * 8) / (32 * 1000)));
  });

  it("MKV/WebM：EBML 嗅探 + DocType 分型（非 webm → mkv 诚实标注）", () => {
    const head = new Uint8Array(64);
    head.set([0x1a, 0x45, 0xdf, 0xa3], 0);
    head.set([0x77, 0x65, 0x62, 0x6d], 8); // "webm" 字面量
    const info = parseAnd(parseMkv(head, true));
    expect(info.container).toBe("webm");
    // 无 Duration → 诚实留白（不编数）。
    expect(info.durationMs).toBeNull();
  });

  it("非媒体字节诚实返回 null（负缓存账面）", () => {
    const junk = new Uint8Array(64).fill(0x5a);
    expect(sniffAndParse(junk, 64)).toBeNull();
  });

  it("LRU + 负缓存：命中即触碰，最冷逐出", () => {
    const store = new MediaInfoStore(2);
    store.put("a", sniffAndParse(buildSampleMp4(), 1));
    store.put("b", null); // 负缓存（解析失败也记账——不重复烧头）
    expect(store.lookup("b")).toBeNull();
    store.put("c", null); // 3 > 2 → 逐出最冷（a 未被触碰）
    expect(store.size).toBe(2);
    expect(store.lookup("a")).toBeUndefined(); // a 已逐出
    expect(store.lookup("b")).toBeNull(); // b 在册（负缓存条目）
    expect(store.lookup("c")).toBeNull();
  });

  it("悬停调度：冷读 800ms / 缓存命中即时（<100ms 判线）", () => {
    expect(HOVER_DELAY_MS).toBe(800);
    expect(CACHE_HIT_LINE_MS).toBe(100);
    expect(HEAD_BYTES).toBe(64 * 1024);
    const s = new HoverScheduler();
    s.hover(0, false);
    expect(s.tick(799)).toBe(false);
    expect(s.tick(800)).toBe(true);
    expect(s.state.kind).toBe("shown");
    s.leave();
    expect(s.state.kind).toBe("idle");
    s.hover(1000, true);
    expect(s.state.kind).toBe("shown"); // 缓存命中即时
    expect(s.instantShows).toBe(1);
  });

  it("标签格式化 + FNV 键稳定", () => {
    expect(durationLabel(65_500)).toBe("1:05");
    expect(durationLabel(3_723_000)).toBe("1:02:03");
    expect(durationLabel(null)).toBe("时长未知");
    expect(resolutionLabel({ container: "mp4", durationMs: null, width: 1920, height: 1080, videoTracks: 1, audioTracks: 1, videoBps: null, sampleRate: null, channels: null, codec: null, fps: null })).toBe("1920×1080");
    expect(fnv1a64("a.mp4", 100)).toBe(fnv1a64("a.mp4", 100));
    expect(fnv1a64("a.mp4", 100)).not.toBe(fnv1a64("a.mp4", 200));
  });
});

/* ------------------------------ F095 终端核心 ------------------------------ */

describe("F095 终端核心", () => {
  it("CJK 宽度表：全宽 2 / 组合零宽 / ASCII 1（混排列不错位本体）", () => {
    expect(charWidth("中")).toBe(2);
    expect(charWidth("ア")).toBe(2); // 片假名（全宽呈现）
    expect(charWidth("ｱ")).toBe(1); // 半角片假名（半角形式区 FF61-FF9F → 窄）
    expect(charWidth("\u0301")).toBe(0); // 组合音符
    expect(charWidth("\u200b")).toBe(0); // 零宽空格
    expect(charWidth("a")).toBe(1);
    expect(lineWidth("中文abc")).toBe(7); // 2+2+1+1+1
    expect(lineWidth("中文\u0301abc")).toBe(7); // 组合符不占列
  });

  it("回看双上限：行数上限逐出 + 头部提示行钉头（D12 修法）", () => {
    const sb = new Scrollback();
    for (let i = 0; i < SCROLLBACK_LINES + 50; i++) sb.push(`line ${i}`, i);
    expect(sb.length).toBeLessThanOrEqual(SCROLLBACK_LINES);
    expect(sb.slice(0, 1)[0]!.text).toBe(TRUNCATION_NOTICE); // 提示永不丢失
    expect(sb.truncations).toBe(1);
  });

  it("字节上限逐出（80MB 口径的机制面）", () => {
    const sb = new Scrollback();
    const big = "x".repeat(1000); // 每行字节账 ≈ 1000 + 2000 + 16
    for (let i = 0; i < 30_000; i++) sb.push(big, i); // ~90MB > 80MB
    expect(sb.bytesUsed).toBeLessThanOrEqual(SCROLLBACK_BYTES);
    expect(sb.truncations).toBeGreaterThanOrEqual(1);
    expect(sb.length).toBeLessThanOrEqual(SCROLLBACK_LINES);
    expect(sb.slice(0, 1)[0]!.text).toBe(TRUNCATION_NOTICE); // 提示行钉头
  }, 15_000);

  it("虚拟滚动切片：只取可视区间、越界钳制", () => {
    const sb = new Scrollback();
    for (let i = 0; i < 100; i++) sb.push(`l${i}`, i);
    expect(sb.slice(10, 5)).toHaveLength(5);
    expect(sb.slice(10, 5)[0]!.text).toBe("l10");
    expect(sb.slice(-5, 3)).toHaveLength(3);
    expect(sb.slice(99, 100)).toHaveLength(1);
    expect(sb.bottomRow()).toBe(99);
  });

  it("会话导出：时间戳 + 退出码元数据", () => {
    const sb = new Scrollback();
    sb.push("hello", 42);
    const text = sb.export(3);
    expect(text).toContain("# VARIX session export");
    expect(text).toContain("[000000000042] hello");
    expect(text).toContain("# exit=3");
  });

  it("帧账：2000 可视格恰在 12.5ms 预算（K1 口径）", () => {
    expect(COST_PER_CELL_US).toBe(6);
    const acc = frameAccount(80, 25);
    expect(acc.cells).toBe(2000);
    expect(acc.costUs).toBe(12_000);
    expect(acc.withinBudget).toBe(true);
    expect(frameAccount(200, 60).withinBudget).toBe(false);
    expect(FRAME_BUDGET_US).toBe(12_500);
  });

  it("分屏二叉树：分裂/关闭/面积守恒/拖拽实时重排", () => {
    resetPaneIds();
    const root = newPane();
    const two = splitPane(root, 1, "h");
    expect(leafCount(two)).toBe(2);
    const three = splitPane(two, 2, "v");
    expect(leafCount(three)).toBe(3); // K2 口径：四分需三次
    const totalArea = (rects: Array<{ w: number; h: number }>): number => rects.reduce((a, x) => a + x.w * x.h, 0);
    const rects = layoutTree(three, 0, 0, 800, 600);
    expect(rects.every((r) => r.w > 0 && r.h > 0)).toBe(true);
    expect(totalArea(rects)).toBe(800 * 600); // 面积守恒（重排不丢面积）
    expect(leafIds(three)).toEqual([1, 2, 3]);
    // 关闭叶。
    const afterClose = closePane(three, 2);
    expect(afterClose).not.toBeNull();
    expect(leafCount(afterClose!)).toBe(2);
    expect(closePane(newPane(), 1)).toBeNull(); // 根叶不可关
    // 拖拽重排：ratio 钳制 [0.1, 0.9]。
    const dragged = dragDivider(three, "h", 10_000, 800);
    expect(JSON.stringify(dragged)).toContain("0.9");
    // 分隔条命中检测（6px 热区）。
    const hit = dividerHit(two, 398, 300, 0, 0, 800, 600);
    expect(hit).toMatchObject({ dir: "h" });
    expect(dividerHit(two, 200, 300, 0, 0, 800, 600)).toBeNull();
  });

  it("字号 8 档", () => {
    expect(FONT_SIZE_STEPS).toEqual([10, 12, 14, 16, 18, 20, 24, 28]);
  });
});

/* ---------------------------- F096 命令面板 ---------------------------- */

describe("F096 命令面板", () => {
  it("内置 12 条全可达（枚举即唯一源）", () => {
    expect(BUILTINS.length).toBe(BUILTIN_COUNT);
    const p = new Palette();
    p.open(0);
    expect(p.allBuiltinsReachable()).toBe(true);
    expect(BUILTINS.map((b) => b.name)).toContain("清屏");
  });

  it("双路模糊打分：子序列/首字母/前缀/频次封顶", () => {
    expect(fuzzyScore("清屏", "qp", "qp", 0)).toBe(80); // 首字母命中
    expect(fuzzyScore("清屏", "qp", "清屏", 0)).toBe(80); // 子序列 60 + 连续前缀 20
    expect(fuzzyScore("清屏", "qp", "清", 0)).toBe(80); // 前缀子序列同分
    expect(fuzzyScore("清屏", "qp", "屏", 0)).toBe(60); // 非前缀子序列
    expect(fuzzyScore("清屏", "qp", "xyz", 0)).toBeNull();
    expect(fuzzyScore("清屏", "qp", "清屏", 100)).toBe(90); // 频次封顶 +10
    expect(fuzzyScore("清屏", "qp", "", 0)).toBe(0); // 空查询全量
  });

  it("高亮段 + 收藏 frecency 定容逐出", () => {
    expect(highlightIndices("清屏", "清")).toEqual([[0, 1]]);
    expect(highlightIndices("清屏", "")).toEqual([]);
    const fav = new Favorites();
    for (let i = 0; i < FAVORITE_CAP + 5; i++) fav.register(`cmd${i}`, `cmd${i}`, i * 1000);
    expect(fav.size).toBeLessThanOrEqual(FAVORITE_CAP);
    // bump 保活：高频者不被逐出。
    const kept = new Favorites();
    kept.register("keepme", "keepme", 0);
    kept.bump("keepme", 1); // 先保活（freq=2）再灌满——frecency 语义
    for (let i = 0; i < FAVORITE_CAP; i++) kept.register(`f${i}`, `f${i}`, i * 1000);
    for (let i = 0; i < FAVORITE_CAP; i++) kept.register(`g${i}`, `g${i}`, 100_000 + i * 1000);
    expect(kept.list().some((c) => c.cmdline === "keepme")).toBe(true);
  });

  it("面板全链：打开/搜索/执行 + 弹出预算账（<100ms 判线）", () => {
    expect(OPEN_BUDGET_MS).toBe(100);
    const p = new Palette();
    p.open(50);
    expect(p.overBudget).toBe(0);
    p.close();
    p.open(120);
    expect(p.overBudget).toBe(1); // 超预算诚实记账
    const hits = p.search("qp");
    expect(hits[0]!.command.name).toBe("清屏");
    const executed = p.execute(hits[0]!, 0);
    expect(executed).toEqual({ cmdline: "clearScreen", effect: "immediate" });
  });

  it("模板变量提取与回填（缺失变量诚实保留占位符）", () => {
    expect(templateVars("ping {host}")).toEqual(["host"]);
    expect(templateVars("cd {dir} && ls {file}")).toEqual(["dir", "file"]);
    expect(templateVars("no vars")).toEqual([]);
    expect(fillTemplate("ping {host}", [["host", "varix"]])).toBe("ping varix");
    expect(fillTemplate("ping {host} {user}", [["host", "varix"]])).toBe("ping varix {user}");
  });

  it("收藏导入导出 round-trip", () => {
    const fav = new Favorites();
    fav.register("构建", "vx build", 100);
    const data = fav.export();
    const fav2 = new Favorites();
    expect(fav2.import(data, 200)).toBe(1);
    expect(fav2.list()[0]!.freq).toBe(1); // 首次导入频次来自数据
    expect(fav2.import(data, 300)).toBe(0); // 同名同命令幂等（频次合并）
    expect(fav2.list()[0]!.freq).toBe(2);
  });
});
