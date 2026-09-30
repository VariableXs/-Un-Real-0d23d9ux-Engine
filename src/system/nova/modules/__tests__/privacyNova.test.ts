import { beforeEach, describe, expect, it } from "vitest";
import {
  ACL_UNSUPPORTED_NOTE,
  BURN_CHARGE_MS,
  BURN_MARK_TEXT,
  BURN_STEP_TEXT,
  BURN_TARGET_ALL,
  CAMERA_BADGE_LIVE,
  CAMERA_BADGE_MASKED,
  CHECKUP_FIXES,
  CHECKUP_INTERVAL_DAYS,
  DNS_GLOSSARY,
  DNS_LEDGER_MAX,
  DNS_RISKY_TLDS,
  DNS_TRACKER_WORDS,
  FINGERPRINT_MIGRATE_MATCH,
  HEARTBEAT_BUSY_PCT,
  HEARTBEAT_LEVEL_TEXT,
  HEARTBEAT_QUIET_PCT,
  PASSWORD_MIN_LEN,
  PASSWORD_TRACK_WEIGHTS,
  PRIVACY_NOVA_FEATURES,
  REDLINE_LOG_MAX,
  REDLINE_Z,
  SENSITIVE_BUBBLE_MS,
  SENSITIVE_PATTERNS,
  STAGE_DESTROY_NOTICE,
  STAGE_REJECT_REAL,
  TRUST_DECAY_PER_DAY,
  TRUST_INCIDENT_PENALTY,
  TRUST_REREVIEW_DAYS,
  TRUST_REREVIEW_SCORE,
  aclChainText,
  aclUnsupportedText,
  burnMarkEnv,
  burnPlan,
  burnPurgeEnv,
  burnReadEnv,
  burnResidue,
  cameraBadgeText,
  cameraMaskToggle,
  checkupDue,
  checkupGradeText,
  checkupRun,
  dnsAnnotate,
  dnsLedgerAppend,
  dnsRowText,
  fingerprintAllowed,
  fingerprintMigration,
  fingerprintOf,
  fingerprintSimilarity,
  flagOn,
  fpHash,
  heartbeatLevel,
  heartbeatRowText,
  heartbeatWall,
  isPrivacyNovaActive,
  isSensitivePath,
  passwordAdvice,
  passwordTracks,
  privacyNovaDomain,
  redlineCardText,
  redlineLogAppend,
  redlineSanitize,
  sensitiveBubbleText,
  sensitiveHitReason,
  stageDestroy,
  stageIngest,
  stageResidue,
  stageScript,
  trustCurve,
  trustReReview,
  trustReviewText,
  trustScore,
} from "../privacyNova";
import type { AclChain, BurnEnv, CheckupInputs, DnsReq, RedlineCard, RedlineLogEntry } from "../privacyNova";
import { NOVA_FEATURES, resetNovaAll, setNovaOn } from "../../registry";

const DAY = 24 * 3_600_000;

/** 固定基准：2026-09-10 12:00 local。 */
const T0 = new Date(2026, 8, 10, 12, 0, 0).getTime();

const CLEAN_INPUTS: CheckupInputs = {
  burnLeftover: 0,
  sensitiveHits: 0,
  cameraLive: false,
  dnsSuspicious: 0,
  clipboardPlain: 0,
  trustMin: 100,
};

beforeEach(() => {
  resetNovaAll();
  localStorage.clear();
});

// ---------------------------------------------------------------------------
// manifest：13 项齐、编号连续、域标识、overlay/默认态对齐注册表
// ---------------------------------------------------------------------------

describe("manifest", () => {
  it("恰好 13 项功能卡", () => {
    expect(PRIVACY_NOVA_FEATURES).toHaveLength(13);
  });

  it("编号 W-114…W-126 连续无缺", () => {
    const ids = PRIVACY_NOVA_FEATURES.map((f) => f.id);
    expect(ids).toEqual(Array.from({ length: 13 }, (_, i) => `W-${114 + i}`));
  });

  it("域标识 S10 / AI-10", () => {
    expect(privacyNovaDomain.id).toBe("S10");
    expect(privacyNovaDomain.route).toBe("AI-10");
  });

  it("overlay 与 S0 注册表一致", () => {
    const reg = new Map(NOVA_FEATURES.map((f) => [f.id, f.overlay]));
    for (const card of PRIVACY_NOVA_FEATURES) {
      expect(card.overlay).toBe(reg.get(card.id));
    }
  });

  it("defaultOn 与注册表默认态一致（W-121/W-123 默认关）", () => {
    const reg = new Map(NOVA_FEATURES.map((f) => [f.id, f.on]));
    for (const card of PRIVACY_NOVA_FEATURES) {
      expect(card.defaultOn).toBe(reg.get(card.id));
    }
    const w121 = PRIVACY_NOVA_FEATURES.find((f) => f.id === "W-121");
    const w123 = PRIVACY_NOVA_FEATURES.find((f) => f.id === "W-123");
    expect(w121?.defaultOn).toBe(false);
    expect(w123?.defaultOn).toBe(false);
  });

  it("每张卡都有降级说明与中文描述", () => {
    for (const card of PRIVACY_NOVA_FEATURES) {
      expect(card.degrade.length).toBeGreaterThan(0);
      expect(card.descZh.length).toBeGreaterThan(0);
      expect(card.titleZh.length).toBeGreaterThan(0);
      expect(card.titleEn.length).toBeGreaterThan(0);
    }
  });

  it("flagOn 只读消费注册表", () => {
    expect(flagOn("W-121")).toBe(false);
    setNovaOn("W-121", true);
    expect(flagOn("W-121")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-114 隐私体检报告
// ---------------------------------------------------------------------------

describe("W-114 checkup", () => {
  it("90 天季度节奏：从未体检即到期", () => {
    expect(checkupDue(null, T0)).toBe(true);
  });

  it("89 天不到期，90 天整到期", () => {
    expect(checkupDue(T0 - 89 * DAY, T0)).toBe(false);
    expect(checkupDue(T0 - CHECKUP_INTERVAL_DAYS * DAY, T0)).toBe(true);
  });

  it("全净输入 → 三轴满分 A 级零整改", () => {
    const r = checkupRun(CLEAN_INPUTS, T0);
    expect(r.axes.map((a) => a.score)).toEqual([100, 100, 100]);
    expect(r.grade).toBe("A");
    expect(r.axes.every((a) => a.fixes.length === 0)).toBe(true);
  });

  it("痕迹轴：焚毁残留 ×10 + 可疑 DNS ×4", () => {
    const r = checkupRun({ ...CLEAN_INPUTS, burnLeftover: 2, dnsSuspicious: 1 }, T0);
    const traces = r.axes.find((a) => a.axis === "traces");
    expect(traces?.score).toBe(100 - 20 - 4);
    expect(traces?.fixes).toContain(CHECKUP_FIXES.traces);
  });

  it("权限轴：摄像头未蒙眼 −25；低信任插件逐级加扣", () => {
    const r1 = checkupRun({ ...CLEAN_INPUTS, cameraLive: true }, T0);
    expect(r1.axes.find((a) => a.axis === "access")?.score).toBe(75);
    const r2 = checkupRun({ ...CLEAN_INPUTS, trustMin: 50 }, T0);
    expect(r2.axes.find((a) => a.axis === "access")?.score).toBe(75);
    const r3 = checkupRun({ ...CLEAN_INPUTS, trustMin: 20 }, T0);
    expect(r3.axes.find((a) => a.axis === "access")?.score).toBe(60); // −25(低信任) −15(极低信任)
  });

  it("出口轴：敏感访问 ×3 + 明文剪贴板 ×2", () => {
    const r = checkupRun({ ...CLEAN_INPUTS, sensitiveHits: 5, clipboardPlain: 2 }, T0);
    expect(r.axes.find((a) => a.axis === "egress")?.score).toBe(100 - 15 - 4);
  });

  it("分数下限 0 不为负", () => {
    const r = checkupRun({ ...CLEAN_INPUTS, sensitiveHits: 100 }, T0);
    expect(r.axes.find((a) => a.axis === "egress")?.score).toBe(0);
  });

  it("等级阈值 A/B/C/D", () => {
    // A：均分 92
    const a = checkupRun({ ...CLEAN_INPUTS, cameraLive: true }, T0);
    expect(a.grade).toBe("A");
    // B：traces 90 / access 75 / egress 91 → 85
    const b = checkupRun({ ...CLEAN_INPUTS, burnLeftover: 1, cameraLive: true, sensitiveHits: 3 }, T0);
    expect(b.grade).toBe("B");
    // C：traces 60 / access 100 / egress 100 → 87？改为三轴压到 60–75 区间
    const c = checkupRun({ ...CLEAN_INPUTS, burnLeftover: 3, dnsSuspicious: 10, sensitiveHits: 14 }, T0);
    expect(c.grade).toBe("C");
    // D：全轴塌方
    const d = checkupRun({ ...CLEAN_INPUTS, burnLeftover: 10, cameraLive: true, trustMin: 20, sensitiveHits: 50 }, T0);
    expect(d.grade).toBe("D");
  });

  it("报告文本含等级与三轴分", () => {
    const text = checkupGradeText(checkupRun(CLEAN_INPUTS, T0));
    expect(text).toContain("A");
    expect(text).toContain("TRACES");
    expect(text).toContain("ACCESS");
    expect(text).toContain("EGRESS");
  });
});

// ---------------------------------------------------------------------------
// W-115 密码力度合奏（内容零记录）
// ---------------------------------------------------------------------------

describe("W-115 password ensemble", () => {
  it("返回值只有分数，永不携带密码内容", () => {
    const t = passwordTracks("S3cr3t!Key#2026");
    expect(Object.keys(t).sort()).toEqual(["entropy", "length", "total", "variety"]);
    expect(JSON.stringify(t)).not.toContain("S3cr3t");
  });

  it("长度轨：8 位起评、32 位满分、20 位过半", () => {
    expect(passwordTracks("a".repeat(8)).length).toBe(0);
    expect(passwordTracks("a".repeat(32)).length).toBe(100);
    expect(passwordTracks("a".repeat(20)).length).toBe(50);
    expect(passwordTracks("a".repeat(4)).length).toBe(0);
  });

  it("字符种轨：单类 25 分、四类满分", () => {
    expect(passwordTracks("abcdefgh").variety).toBe(25);
    expect(passwordTracks("abcdefghij12").variety).toBe(50);
    expect(passwordTracks("Abcd1234!@").variety).toBe(100);
  });

  it("熵轨：连续顺子扣分（abcdefgh 八连 −50）", () => {
    expect(passwordTracks("abcdefgh").entropy).toBe(50);
  });

  it("熵轨：重复占比扣分（16×a → 43）", () => {
    // repeatRatio 15/16=0.9375 → 100−56.25=43.75 → round 44
    const t = passwordTracks("a".repeat(16));
    expect(t.entropy).toBe(44);
  });

  it("总分按 40/30/30 权重合成", () => {
    expect(PASSWORD_TRACK_WEIGHTS).toEqual({ length: 0.4, variety: 0.3, entropy: 0.3 });
    const t = passwordTracks("abcdefgh");
    const expected = Math.round(t.length * 0.4 + t.variety * 0.3 + t.entropy * 0.3);
    expect(t.total).toBe(expected);
  });

  it("最短长度常量 8", () => {
    expect(PASSWORD_MIN_LEN).toBe(8);
  });

  it("建议逐条可执行且永不复述密码", () => {
    const pw = "Xk9$mQ2vLp#8";
    const advice = passwordAdvice(pw);
    for (const a of advice) expect(a).not.toContain(pw);
    expect(advice.join(" ")).not.toContain("Xk9");
    // 12 位 4 类无顺子：仅长度未满分一条
    expect(advice).toHaveLength(1);
    expect(advice[0]).toContain("再长");
  });

  it("不足 8 位首先给补长指导", () => {
    const advice = passwordAdvice("ab1");
    expect(advice[0]).toContain("至少 8 位");
  });

  it("强密码给全绿结语", () => {
    // 32 位 4 类无顺子无重复 → 三轨全满
    const advice = passwordAdvice("Xk9$mQ2vLp#8Zr4&Wt7!Bn3*Fd6^Jh1+");
    expect(advice).toEqual(["三轨全绿 —— 这把钥匙很结实"]);
  });

  it("连续序列与重复各出对应指导", () => {
    const advice = passwordAdvice("abcd1234");
    expect(advice.join(" ")).toContain("连续序列");
    const advice2 = passwordAdvice("aaaaaaaaaaaaaaaa");
    expect(advice2.join(" ")).toContain("重复");
  });
});

// ---------------------------------------------------------------------------
// W-116 摄像头眼罩
// ---------------------------------------------------------------------------

describe("W-116 camera blindfold", () => {
  it("双击翻转蒙眼态", () => {
    expect(cameraMaskToggle(false)).toBe(true);
    expect(cameraMaskToggle(cameraMaskToggle(true))).toBe(true);
  });

  it("徽标文案：蒙眼/使用中", () => {
    expect(cameraBadgeText(true)).toBe(CAMERA_BADGE_MASKED);
    expect(cameraBadgeText(false)).toBe(CAMERA_BADGE_LIVE);
    expect(CAMERA_BADGE_MASKED).toBe("已蒙眼");
  });
});

// ---------------------------------------------------------------------------
// W-117 敏感文件气泡
// ---------------------------------------------------------------------------

describe("W-117 sensitive bubble", () => {
  it("特征表命中凭据/密钥/账本类", () => {
    expect(isSensitivePath("C:/Users/a/id_rsa")).toBe(true);
    expect(isSensitivePath("D:/proj/.env.local")).toBe(true);
    expect(isSensitivePath("D:/proj/server.key")).toBe(true);
    expect(isSensitivePath("D:/家庭账本.xlsx")).toBe(true);
    expect(isSensitivePath("C:/keys/token.dat")).toBe(true);
    expect(isSensitivePath("D:/wallpaper.png")).toBe(false);
    expect(isSensitivePath("D:/notes.txt")).toBe(false);
  });

  it("命中理由可解释（返回命中的特征片段）", () => {
    expect(sensitiveHitReason("C:/Users/a/id_rsa")).toBe("id_rsa");
    expect(sensitiveHitReason("D:/wallpaper.png")).toBeNull();
  });

  it("气泡文案：文件名 + 读取方", () => {
    expect(sensitiveBubbleText("C:/Users/a/id_rsa", "同步器")).toBe("「id_rsa」正被 同步器 读取");
    expect(sensitiveBubbleText("D:/dir/.env", "编辑器")).toBe("「.env」正被 编辑器 读取");
  });

  it("气泡 2s 即焚", () => {
    expect(SENSITIVE_BUBBLE_MS).toBe(2000);
  });

  it("特征表为正则清单（可解释）", () => {
    expect(SENSITIVE_PATTERNS.length).toBeGreaterThan(3);
    expect(SENSITIVE_PATTERNS.every((re) => re instanceof RegExp)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-118 阅后即焚剪贴板
// ---------------------------------------------------------------------------

describe("W-118 burn-after-read", () => {
  it("显式标记入封套：marked 恒 true", () => {
    const env = burnMarkEnv("secret-token", T0);
    expect(env.marked).toBe(true);
    expect(env.consumed).toBe(false);
    expect(env.at).toBe(T0);
  });

  it("首读交付并烧毁（引用同步清空）", () => {
    const env = burnMarkEnv("secret-token", T0);
    const r = burnReadEnv(env);
    expect(r?.text).toBe("secret-token");
    expect(env.consumed).toBe(true);
    expect(env.text).toBe("");
  });

  it("再读为空（一次即焚）", () => {
    const env = burnMarkEnv("secret-token", T0);
    burnReadEnv(env);
    expect(burnReadEnv(env)).toBeNull();
    expect(burnReadEnv(env)).toBeNull();
  });

  it("彻底清除：封套归零", () => {
    const env: BurnEnv = burnMarkEnv("data", T0);
    expect(burnPurgeEnv(env)).toBe(true);
    expect(env.text).toBe("");
    expect(env.consumed).toBe(true);
    expect(burnPurgeEnv(null)).toBe(true);
  });

  it("标记文案常量", () => {
    expect(BURN_MARK_TEXT).toBe("阅后即焚");
  });
});

// ---------------------------------------------------------------------------
// W-119 后台心跳墙（三电平）
// ---------------------------------------------------------------------------

describe("W-119 heartbeat wall", () => {
  it("三电平阈值：2% 静息界、15% 活跃界", () => {
    expect(HEARTBEAT_QUIET_PCT).toBe(2);
    expect(HEARTBEAT_BUSY_PCT).toBe(15);
    expect(heartbeatLevel(0)).toBe(0);
    expect(heartbeatLevel(1.9)).toBe(0);
    expect(heartbeatLevel(2)).toBe(1);
    expect(heartbeatLevel(14.9)).toBe(1);
    expect(heartbeatLevel(15)).toBe(2);
    expect(heartbeatLevel(80)).toBe(2);
  });

  it("墙面按占用降序，同分按名字稳定序", () => {
    const rows = heartbeatWall([
      { name: "beta", pct: 10 },
      { name: "alpha", pct: 10 },
      { name: "gamma", pct: 50 },
    ]);
    expect(rows.map((r) => r.name)).toEqual(["gamma", "alpha", "beta"]);
    expect(rows[0]?.level).toBe(2);
    expect(rows[1]?.level).toBe(1);
    expect(rows[2]?.level).toBe(1);
  });

  it("行文案含电平中文与占比", () => {
    const text = heartbeatRowText({ name: "sync", pct: 30, level: 2 });
    expect(text).toContain("sync");
    expect(text).toContain(HEARTBEAT_LEVEL_TEXT[2]);
    expect(text).toContain("30.0%");
  });
});

// ---------------------------------------------------------------------------
// W-120 信任衰减曲线（透明公式）
// ---------------------------------------------------------------------------

describe("W-120 trust decay", () => {
  it("公式透明：base − 天数×0.5 − 事故×12", () => {
    expect(TRUST_DECAY_PER_DAY).toBe(0.5);
    expect(TRUST_INCIDENT_PENALTY).toBe(12);
    expect(trustScore(100, 0, 0)).toBe(100);
    expect(trustScore(100, 10, 0)).toBe(95);
    expect(trustScore(100, 0, 2)).toBe(76);
    expect(trustScore(80, 5, 1)).toBe(66); // round(65.5) 四舍五入
  });

  it("分数夹在 0–100", () => {
    expect(trustScore(50, 200, 10)).toBe(0);
    expect(trustScore(100, -50, -10)).toBe(100);
  });

  it("复审建议：低于 60 分或 60 天未复审", () => {
    expect(TRUST_REREVIEW_SCORE).toBe(60);
    expect(TRUST_REREVIEW_DAYS).toBe(60);
    expect(trustReReview(59, 0)).toBe(true);
    expect(trustReReview(60, 0)).toBe(false);
    expect(trustReReview(100, 59)).toBe(false);
    expect(trustReReview(100, 60)).toBe(true);
    expect(trustReReview(30, 100)).toBe(true);
  });

  it("曲线逐日采样且单调不升", () => {
    const curve = trustCurve(100, 0, 30);
    expect(curve).toHaveLength(31);
    expect(curve[0]).toBe(100);
    for (let i = 1; i < curve.length; i++) {
      expect(curve[i]!).toBeLessThanOrEqual(curve[i - 1]!);
    }
  });

  it("复审文案含插件名与天数", () => {
    const text = trustReviewText("clip-helper", 45, 61);
    expect(text).toContain("clip-helper");
    expect(text).toContain("45");
    expect(text).toContain("61");
  });
});

// ---------------------------------------------------------------------------
// W-121 隐私剧场排演（零真实数据）
// ---------------------------------------------------------------------------

describe("W-121 leak theater", () => {
  it("拒收未标记剧料（sandbox 红线）", () => {
    expect(stageIngest({ scene: "s", text: "真实数据" })).toBeNull();
    expect(stageIngest({ scene: "s", text: "t", sandbox: false })).toBeNull();
    expect(stageIngest({ text: "t", sandbox: true })).toBeNull();
    expect(STAGE_REJECT_REAL).toContain("sandbox");
  });

  it("固定剧本五幕且全部带 sandbox 标记", () => {
    const script = stageScript();
    expect(script).toHaveLength(5);
    expect(script.every((s) => s.sandbox === true)).toBe(true);
    expect(script.every((s) => s.scene.length > 0 && s.text.length > 0)).toBe(true);
  });

  it("闭幕销毁：残留清点必须为 0", () => {
    const script = stageScript();
    expect(stageResidue(script)).toBe(5);
    expect(stageDestroy(script)).toBe(0);
    expect(stageResidue(script)).toBe(0);
    expect(script.every((s) => s.text === "" && s.scene === "")).toBe(true);
    expect(STAGE_DESTROY_NOTICE).toContain("0");
  });

  it("剧料再摄入闭环：销毁后重建不受污染", () => {
    const s1 = stageScript();
    stageDestroy(s1);
    const s2 = stageScript();
    expect(stageResidue(s2)).toBe(5);
  });
});

// ---------------------------------------------------------------------------
// W-122 DNS 白话簿
// ---------------------------------------------------------------------------

describe("W-122 dns ledger", () => {
  it("内置词典白话标注", () => {
    expect(DNS_GLOSSARY["fonts.googleapis.com"]).toBe("字体服务");
    const a = dnsAnnotate("fonts.googleapis.com");
    expect(a.plain).toBe("字体服务");
    expect(a.suspicious).toBe(false);
  });

  it("未收录域名如实「未收录」", () => {
    const a = dnsAnnotate("some-unknown-site.example");
    expect(a.plain).toBe("未收录");
    expect(a.suspicious).toBe(false);
  });

  it("红标：punycode 编码域", () => {
    const a = dnsAnnotate("xn--80ak6aa92e.com");
    expect(a.suspicious).toBe(true);
    expect(a.reasons.join()).toContain("punycode");
  });

  it("红标：裸 IP 直连", () => {
    const a = dnsAnnotate("192.168.1.1");
    expect(a.suspicious).toBe(true);
    expect(a.reasons.join()).toContain("裸 IP");
  });

  it("红标：跟踪词根", () => {
    expect(DNS_TRACKER_WORDS).toContain("track");
    const a = dnsAnnotate("cdn.tracker-ads.example.com");
    expect(a.suspicious).toBe(true);
    expect(a.reasons.length).toBeGreaterThan(0);
  });

  it("红标：高风险后缀", () => {
    expect(DNS_RISKY_TLDS).toContain("zip");
    const a = dnsAnnotate("invoice.zip");
    expect(a.suspicious).toBe(true);
    expect(a.reasons.join()).toContain(".zip");
  });

  it("红标：多级连字符子域", () => {
    const a = dnsAnnotate("a-b.c-d.e.com");
    expect(a.suspicious).toBe(true);
    expect(a.reasons.join()).toContain("连字符");
  });

  it("正常域名零误报（github.com）", () => {
    const a = dnsAnnotate("github.com");
    expect(a.plain).toBe("代码托管");
    expect(a.suspicious).toBe(false);
    expect(a.reasons).toHaveLength(0);
  });

  it("账本容量滚动：只留最新 200 条", () => {
    let ledger: DnsReq[] = [];
    for (let i = 0; i < DNS_LEDGER_MAX + 10; i++) {
      ledger = dnsLedgerAppend(ledger, { domain: `d${i}.test`, at: T0 + i });
    }
    expect(ledger).toHaveLength(DNS_LEDGER_MAX);
    expect(ledger[0]?.domain).toBe("d10.test");
    expect(ledger[DNS_LEDGER_MAX - 1]?.domain).toBe(`d${DNS_LEDGER_MAX + 9}.test`);
  });

  it("行文本：白话 + 可疑加 ⚠ 与理由", () => {
    const clean = dnsRowText({ domain: "github.com", at: T0 });
    expect(clean).toContain("代码托管");
    expect(clean).not.toContain("⚠");
    const bad = dnsRowText({ domain: "invoice.zip", at: T0 });
    expect(bad).toContain("⚠");
    expect(bad).toContain(".zip");
  });
});

// ---------------------------------------------------------------------------
// W-123 指纹黑匣（单向；opt-in）
// ---------------------------------------------------------------------------

describe("W-123 fingerprint vault", () => {
  it("FNV-1a 单向哈希：确定 + 8 位十六进制", () => {
    const h1 = fpHash("vector-a\u0000vector-b");
    const h2 = fpHash("vector-a\u0000vector-b");
    expect(h1).toBe(h2);
    expect(h1).toMatch(/^[0-9a-f]{8}$/);
    expect(fpHash("different")).not.toBe(h1);
  });

  it("指纹依赖特征向量顺序（单向，不可逆推）", () => {
    const fp1 = fingerprintOf(["cpu:abc", "os:win"]);
    const fp2 = fingerprintOf(["os:win", "cpu:abc"]);
    expect(fp1).not.toBe(fp2);
    expect(JSON.stringify(fp1)).not.toContain("cpu");
  });

  it("十六进制相似度：逐位一致比例", () => {
    expect(fingerprintSimilarity("aaaaaaaa", "aaaaaaaa")).toBe(1);
    expect(fingerprintSimilarity("aaaaaaaa", "aaaaaaab")).toBe(7 / 8);
    expect(fingerprintSimilarity("aaaaaaaa", "bbbbbbbb")).toBe(0);
    expect(fingerprintSimilarity("abc", "abcd")).toBe(0);
    expect(fingerprintSimilarity("", "")).toBe(0);
  });

  it("迁移比对：相似度 ≥ 0.75 判同一环境", () => {
    expect(FINGERPRINT_MIGRATE_MATCH).toBe(0.75);
    expect(fingerprintMigration("aaaaaaaa", "aaaaaaaa").same).toBe(true);
    expect(fingerprintMigration("aaaaaaaa", "aaaaaaab").same).toBe(true);
    expect(fingerprintMigration("aaaaaaaa", "bbbbbbbb").same).toBe(false);
    const m = fingerprintMigration("aaaaaaaa", "aaaaaaab");
    expect(m.similarity).toBeCloseTo(0.875);
  });

  it("opt-in 守卫：默认关闭不采集", () => {
    expect(fingerprintAllowed()).toBe(false);
    setNovaOn("W-123", true);
    expect(fingerprintAllowed()).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-124 安全红线通知
// ---------------------------------------------------------------------------

describe("W-124 redline notice", () => {
  const card: RedlineCard = { title: "剪贴板读取者", body: "后台应用读取了即焚内容", at: T0 };

  it("金边红字置顶：z 序高于一切 nova 层", () => {
    expect(REDLINE_Z).toBeGreaterThan(2147483000);
  });

  it("日志净化：只留标题与时间，正文即焚", () => {
    const e = redlineSanitize(card);
    expect(e.title).toBe("剪贴板读取者");
    expect(e.at).toBe(T0);
    expect(JSON.stringify(e)).not.toContain("后台应用读取了即焚内容");
  });

  it("日志容量滚动 50 条", () => {
    let log: RedlineLogEntry[] = [];
    for (let i = 0; i < REDLINE_LOG_MAX + 5; i++) {
      log = redlineLogAppend(log, { title: `t${i}`, at: T0 + i });
    }
    expect(log).toHaveLength(REDLINE_LOG_MAX);
    expect(log[0]?.title).toBe("t5");
  });

  it("卡片文本含标题与正文", () => {
    expect(redlineCardText(card)).toContain("剪贴板读取者");
    expect(redlineCardText(card)).toContain("后台应用读取了即焚内容");
  });
});

// ---------------------------------------------------------------------------
// W-125 权限族谱（ACL）
// ---------------------------------------------------------------------------

describe("W-125 permission tree", () => {
  const chain: AclChain = {
    path: "D:/vault/ledger.xlsx",
    chain: [
      { from: null, name: "D:/ (根)", rights: ["读"] },
      { from: "D:/ (根)", name: "vault", rights: ["读", "写"] },
      { from: "vault", name: "ledger.xlsx", rights: ["读", "写", "删"] },
    ],
  };

  it("族谱文本：根→叶逐级缩进含继承来源", () => {
    const text = aclChainText(chain);
    expect(text).toContain("D:/vault/ledger.xlsx");
    expect(text).toContain("↳ D:/ (根) [读]");
    expect(text).toContain("← vault");
    expect(text.split("\n")).toHaveLength(4);
  });

  it("ACL 不支持：如实标注 NOT AVAILABLE，绝不编造族谱", () => {
    const text = aclUnsupportedText("D:/any.file");
    expect(text).toContain("NOT AVAILABLE");
    expect(text).toContain(ACL_UNSUPPORTED_NOTE);
    expect(text).toContain("D:/any.file");
    expect(text).not.toContain("↳ D:/");
  });
});

// ---------------------------------------------------------------------------
// W-126 焚毁仪式
// ---------------------------------------------------------------------------

describe("W-126 burn ritual", () => {
  it("蓄力 1.2s", () => {
    expect(BURN_CHARGE_MS).toBe(1200);
  });

  it("焚毁计划：按固定顺序裁剪目标", () => {
    expect(BURN_TARGET_ALL).toEqual(["clipboard", "history", "ledger"]);
    expect(burnPlan(["clipboard", "ledger"])).toEqual(["clipboard", "ledger"]);
    expect(burnPlan(["ledger", "clipboard"])).toEqual(["clipboard", "ledger"]);
    expect(burnPlan(["history" as never])).toEqual(["history"]);
    expect(burnPlan(["nonsense" as never])).toEqual([]);
    expect(burnPlan(BURN_TARGET_ALL)).toEqual(BURN_TARGET_ALL);
  });

  it("步骤文案齐备", () => {
    expect(BURN_STEP_TEXT.clipboard).toContain("剪贴板");
    expect(BURN_STEP_TEXT.history).toContain("彻底清除");
    expect(BURN_STEP_TEXT.ledger).toContain("DNS");
  });

  it("残留清点：只认本域 nova.privacy.* 键", () => {
    localStorage.setItem("nova.privacy.dns-ledger", "[]");
    localStorage.setItem("nova.privacy.redline-log", "[]");
    localStorage.setItem("other.app.key", "x");
    expect(burnResidue()).toBe(2);
    localStorage.removeItem("nova.privacy.dns-ledger");
    expect(burnResidue()).toBe(1);
    localStorage.removeItem("nova.privacy.redline-log");
    expect(burnResidue()).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// 激活守卫（node 环境无 DOM：幂等 + 不抛错）
// ---------------------------------------------------------------------------

describe("lifecycle guards", () => {
  it("未激活即如实报告", () => {
    expect(isPrivacyNovaActive()).toBe(false);
  });
});
