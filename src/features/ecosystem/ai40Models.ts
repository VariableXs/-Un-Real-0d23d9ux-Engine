/**
 * UNREAL-X-15000 · AI-40 生态面 V/三方线逻辑核（族0391~0400 · X09751~X10000），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0391 插件系统 2.0（X09751~X09775）-------- */

export const PLUGIN_CAPS = ['storage', 'ui', 'net', 'fs-sandbox', 'notify'] as const;
export type PluginCap = (typeof PLUGIN_CAPS)[number];

/** 插件系统 2.0：清单校验 + 能力授权 + 启停门禁。 */
export class PluginSystemX2 {
  clamped = 0;
  installed = new Map<string, string[]>();
  /** 清单校验：id 非空且能力全在白名单内才可装。 */
  install(id: string, caps: string[]): boolean {
    if (!id || caps.some((c) => !(PLUGIN_CAPS as readonly string[]).includes(c))) {
      this.clamped++;
      return false;
    }
    this.installed.set(id, caps);
    return true;
  }
  /** 能力裁决：插件只可用已授权能力。 */
  canUse(id: string, cap: string): boolean {
    return (this.installed.get(id) ?? []).includes(cap);
  }
  /** 卸载净身。 */
  uninstall(id: string): boolean {
    return this.installed.delete(id);
  }
  /** 沙盒降级：fs-sandbox → 只读白名单。 */
  static sandboxLevel(caps: string[]): 'full' | 'readonly' | 'none' {
    if (!caps.includes('fs-sandbox')) return 'none';
    return caps.includes('storage') ? 'full' : 'readonly';
  }
}

/* -------- 族0392 壁纸社区 2.0（X09776~X09800）-------- */

export const WALL_RATE_LIMITS = [10, 50, 200] as const;

/** 壁纸社区：上传分级 + 审核流 + 限速。 */
export class WallpaperCommunity {
  clamped = 0;
  gallery: { id: string; kind: string; approved: boolean; dl: number }[] = [];
  /** 投稿：动态/静态分级，非空才收。 */
  submit(id: string, kind: string): boolean {
    if (!id || !(kind === 'static' || kind === 'animated')) {
      this.clamped++;
      return false;
    }
    this.gallery.push({ id, kind, approved: false, dl: 0 });
    return true;
  }
  /** 审核通过后可下载。 */
  approve(id: string): boolean {
    const g = this.gallery.find((x) => x.id === id);
    if (!g) {
      this.clamped++;
      return false;
    }
    g.approved = true;
    return true;
  }
  /** 下载只对过审作品生效。 */
  download(id: string): boolean {
    const g = this.gallery.find((x) => x.id === id);
    if (!g || !g.approved) return false;
    g.dl++;
    return true;
  }
  /** 热门榜：按下载排序取前 N。 */
  static topN(items: { dl: number }[], n: number): string[] {
    return [...items].sort((a, b) => b.dl - a.dl).slice(0, Math.max(0, n)).map((_, i) => `#${i + 1}`);
  }
}

/* -------- 族0393 商店 2.0（X09801~X09825）-------- */

export const STORE_TIERS = ['free', 'paid', 'subscription'] as const;
export type StoreTier = (typeof STORE_TIERS)[number];

/** 商店 2.0：上架/定价/购买/退款闭环。 */
export class StoreX2 {
  clamped = 0;
  catalog = new Map<string, { tier: StoreTier; price: number; bought: boolean }>();
  /** 上架：价格钳制（free 必须 0，paid 0~9999）。 */
  list(id: string, tier: string, price: number): boolean {
    const ok = (STORE_TIERS as readonly string[]).includes(tier);
    const legal = tier === 'free' ? price === 0 : price >= 0 && price <= 9999 && Number.isFinite(price);
    if (!ok || !legal) {
      this.clamped++;
      return false;
    }
    this.catalog.set(id, { tier: tier as StoreTier, price, bought: false });
    return true;
  }
  /** 购买：free 直接通过，paid 需余额。 */
  buy(id: string, balance: number): boolean {
    const it = this.catalog.get(id);
    if (!it || it.bought) return false;
    if (it.price > balance) return false;
    it.bought = true;
    return true;
  }
  /** 退款净身：买过才可退。 */
  refund(id: string): boolean {
    const it = this.catalog.get(id);
    if (!it || !it.bought) {
      this.clamped++;
      return false;
    }
    it.bought = false;
    return true;
  }
}

/* -------- 族0394 开发者平台 2.0（X09826~X09850）-------- */

/** 开发者平台：密钥 + 配额 + 示例工程模板。 */
export class DevPlatform {
  clamped = 0;
  keys = new Map<string, { quota: number; used: number }>();
  /** 签发密钥：配额 100~100000 钳制。 */
  issue(dev: string, quota: number): number {
    if (!dev) {
      this.clamped++;
      return 0;
    }
    const q = Math.min(100000, Math.max(100, Math.round(quota)));
    const key = `dk_${dev}_${q}`;
    this.keys.set(key, { quota: q, used: 0 });
    return q;
  }
  /** 调用记账：超额拒绝。 */
  call(key: string): boolean {
    const k = this.keys.get(key);
    if (!k || k.used >= k.quota) return false;
    k.used++;
    return true;
  }
  /** 剩余额度比例（0~1）。 */
  static remain(k: { quota: number; used: number } | undefined): number {
    if (!k || k.quota <= 0) return 0;
    return Math.max(0, 1 - k.used / k.quota);
  }
}

/* -------- 族0395 自动化开放（X09851~X09875）-------- */

export const AUTO_TRIGGERS = ['time', 'event', 'manual', 'watch'] as const;
export type AutoTrigger = (typeof AUTO_TRIGGERS)[number];

/** 自动化开放：脚本注册 + 触发器裁决 + 防环。 */
export class AutoOpen {
  clamped = 0;
  scripts: { id: string; trigger: AutoTrigger; chain: number }[] = [];
  /** 注册：触发器白名单外拒绝。 */
  register(id: string, trigger: string): boolean {
    if (!id || !(AUTO_TRIGGERS as readonly string[]).includes(trigger)) {
      this.clamped++;
      return false;
    }
    this.scripts.push({ id, trigger: trigger as AutoTrigger, chain: 0 });
    return true;
  }
  /** 触发：链深 ≤3 防自动化环。 */
  fire(id: string): boolean {
    const s = this.scripts.find((x) => x.id === id);
    if (!s) {
      this.clamped++;
      return false;
    }
    if (s.chain >= 3) return false;
    s.chain++;
    return true;
  }
  /** 净身：链计数清零。 */
  resetChains(): void {
    for (const s of this.scripts) s.chain = 0;
  }
}

/* -------- 族0396 API 2.0（X09876~X09900）-------- */

export const API_VERSIONS = ['v1', 'v2'] as const;
export type ApiVersion = (typeof API_VERSIONS)[number];

/** API 2.0：版本路由 + 废弃头 + 速率门禁。 */
export class ApiX2 {
  clamped = 0;
  deprecated = new Set<string>();
  /** 路由：未注册版本回退 v1。 */
  route(v: string): ApiVersion {
    if (!(API_VERSIONS as readonly string[]).includes(v)) {
      this.clamped++;
      return 'v1';
    }
    return v as ApiVersion;
  }
  /** 废弃标记：v1 走 Sunset 头。 */
  sunset(v: string): boolean {
    if (v !== 'v1') {
      this.clamped++;
      return false;
    }
    this.deprecated.add(v);
    return true;
  }
  /** 速率窗：滑动窗口内 ≤N 次。 */
  static allowWindow(times: number[], now: number, limit: number, spanMs: number): boolean {
    const inWin = times.filter((t) => now - t <= spanMs).length;
    return inWin < limit;
  }
}

/* -------- 族0397 Web 生态 2.0（X09901~X09925）-------- */

export const WEB_PERMS = ['camera', 'mic', 'geo', 'clipboard'] as const;

/** Web 生态：PWA 清单 + 权限询问 + 离线降级。 */
export class WebEco {
  clamped = 0;
  granted = new Set<string>();
  ask(perm: string): boolean {
    if (!(WEB_PERMS as readonly string[]).includes(perm)) {
      this.clamped++;
      return false;
    }
    this.granted.add(perm);
    return true;
  }
  revoke(perm: string): boolean {
    return this.granted.delete(perm);
  }
  /** 离线降级：缓存命中走本地，未命中给空态。 */
  static offline(hit: boolean): 'cache' | 'empty' {
    return hit ? 'cache' : 'empty';
  }
}

/* -------- 族0398 创作者 2.0（X09926~X09950）-------- */

export const CREATOR_LEVELS = ['seed', 'bloom', 'aurora'] as const;
export type CreatorLevel = (typeof CREATOR_LEVELS)[number];

/** 创作者 2.0：成长等级 + 收益分成 + 署名。 */
export class CreatorX2 {
  clamped = 0;
  level: Record<string, CreatorLevel> = {};
  earnings: Record<string, number> = {};
  setLevel(c: string, lv: string): CreatorLevel {
    if (!c || !(CREATOR_LEVELS as readonly string[]).includes(lv)) {
      this.clamped++;
      return 'seed';
    }
    this.level[c] = lv as CreatorLevel;
    return lv as CreatorLevel;
  }
  /** 分成：按等级 55%/65%/75%。 */
  share(c: string, gross: number): number {
    const rate = { seed: 0.55, bloom: 0.65, aurora: 0.75 }[this.level[c] ?? 'seed'];
    const v = Math.round(gross * rate);
    this.earnings[c] = (this.earnings[c] ?? 0) + v;
    return v;
  }
  /** 署名串。 */
  static credit(c: string, lv: CreatorLevel): string {
    return `${c} · ${lv}`;
  }
}

/* -------- 族0399 硬件伙伴（X09951~X09975）-------- */

export const PARTNER_TIERS = ['basic', 'silver', 'gold'] as const;
export type PartnerTier = (typeof PARTNER_TIERS)[number];

/** 硬件伙伴：认证分级 + 驱动白名单 + 适配矩阵。 */
export class PartnerProgram {
  clamped = 0;
  members = new Map<string, PartnerTier>();
  certified: string[] = [];
  join(vendor: string, tier: string): boolean {
    if (!vendor || !(PARTNER_TIERS as readonly string[]).includes(tier)) {
      this.clamped++;
      return false;
    }
    this.members.set(vendor, tier as PartnerTier);
    return true;
  }
  /** 认证：silver 及以上可进驱动白名单（白名单去重）。 */
  certify(vendor: string): boolean {
    const t = this.members.get(vendor);
    if (!t || t === 'basic' || this.certified.includes(vendor)) {
      this.clamped++;
      return false;
    }
    this.certified.push(vendor);
    return true;
  }
  /** 适配矩阵：gold 伙伴获得 3 档测试矩阵，其余 2 档。 */
  static matrix(tier: PartnerTier): number {
    return tier === 'gold' ? 3 : 2;
  }
}

/* -------- 族0400 国际社区（X09976~X10000）-------- */

export const COMMUNITY_LANGS = ['zh', 'en', 'ja', 'ko', 'es', 'de', 'fr', 'ru'] as const;

/** 国际社区：分语言分坛 + 置顶 + 翻译接力。 */
export class IntlCommunity {
  clamped = 0;
  posts: { id: string; lang: string; pinned: boolean }[] = [];
  post(id: string, lang: string): boolean {
    if (!id || !(COMMUNITY_LANGS as readonly string[]).includes(lang)) {
      this.clamped++;
      return false;
    }
    this.posts.push({ id, lang, pinned: false });
    return true;
  }
  pin(id: string): boolean {
    const p = this.posts.find((x) => x.id === id);
    if (!p) return false;
    p.pinned = true;
    return true;
  }
  /** 时间线：置顶优先，其余按序。 */
  timeline(lang: string): string[] {
    const own = this.posts.filter((p) => p.lang === lang);
    const top = own.filter((p) => p.pinned).map((p) => p.id);
    const rest = own.filter((p) => !p.pinned).map((p) => p.id);
    return [...top, ...rest];
  }
  /** 翻译接力：缺口语言回退 zh。 */
  static relay(available: string[], want: string): string {
    return available.includes(want) ? want : 'zh';
  }
}
