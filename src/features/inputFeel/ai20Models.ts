/**
 * UNREAL-X-15000 · AI-20 输入工程与中文 V 线逻辑核（族0191/0196/0197/0198 · X04751~X04775 / X04876~X04950），勿删。
 * V 线落点：src/features/inputFeel/。零 AI：全部确定性算法。
 */

/* -------- 族0191 中文排版 2.0（X04751~X04775）-------- */

/** 中文排版档位（5 档，默认 balanced）。 */
export const CJK_TYPOGRAPHY_PROFILES = ['off', 'light', 'balanced', 'strict', 'print'] as const;
export type CjkProfileId = (typeof CJK_TYPOGRAPHY_PROFILES)[number];
export const DEFAULT_CJK_PROFILE: CjkProfileId = 'balanced';

/** 排档参数：字距/行高/标点悬挂/中西文间距。 */
export interface CjkProfile {
  tracking: number;
  lineHeight: number;
  hangingPunctuation: boolean;
  hanSpace: boolean;
}

export const CJK_PROFILE_MATRIX: Record<CjkProfileId, CjkProfile> = {
  off: { tracking: 0, lineHeight: 1.4, hangingPunctuation: false, hanSpace: false },
  light: { tracking: 0, lineHeight: 1.5, hangingPunctuation: false, hanSpace: true },
  balanced: { tracking: 0, lineHeight: 1.6, hangingPunctuation: false, hanSpace: true },
  strict: { tracking: 0, lineHeight: 1.6, hangingPunctuation: true, hanSpace: true },
  print: { tracking: 0.01, lineHeight: 1.7, hangingPunctuation: true, hanSpace: true },
};

/** 中文排版器：档位 + 标点挤压 + 中西文间距 + 段首缩进。 */
export class CjkTypography {
  profileId: CjkProfileId;
  clamped = 0;
  constructor(profileId: string = DEFAULT_CJK_PROFILE) {
    this.profileId = (CJK_TYPOGRAPHY_PROFILES as readonly string[]).includes(profileId)
      ? (profileId as CjkProfileId)
      : DEFAULT_CJK_PROFILE;
    if (this.profileId !== profileId) this.clamped = 1;
  }
  get profile(): CjkProfile {
    return CJK_PROFILE_MATRIX[this.profileId];
  }
  /** 标点挤压：行首标点上移省略/行尾标点悬挂，确定映射。 */
  squeeze(line: string): string {
    let out = line;
    if (this.profile.hangingPunctuation) return out;
    // 行首起始标点挤压（，。等前空去除）
    out = out.replace(/^([，。、；：？！])/, '');
    return out;
  }
  /** 中西文间距：汉字与拉丁字母间插入 U+2009。 */
  hanSpace(text: string): string {
    if (!this.profile.hanSpace) return text;
    return text.replace(/([\u4e00-\u9fff])([A-Za-z0-9])/g, '$1\u2009$2')
      .replace(/([A-Za-z0-9])([\u4e00-\u9fff])/g, '$1\u2009$2');
  }
  /** 段首缩进：两枚全角空格（print/strict 档）。 */
  indent(paragraph: string): string {
    return this.profileId === 'print' || this.profileId === 'strict'
      ? '\u3000\u3000' + paragraph
      : paragraph;
  }
  serialize(): string {
    return JSON.stringify({ profileId: this.profileId });
  }
  static deserialize(data: string): CjkTypography {
    try {
      const p = JSON.parse(data) as { profileId?: string };
      return new CjkTypography(p.profileId ?? DEFAULT_CJK_PROFILE);
    } catch {
      return new CjkTypography();
    }
  }
}

/* -------- 族0196 通用输入细节（X04876~X04900）-------- */

/** 输入细节包：智能引号 / 破折号 / 省略号 / 连续空格收敛。 */
export class InputDetail {
  smartQuotes: boolean;
  emDash: boolean;
  ellipsis: boolean;
  collapseSpace: boolean;
  history: string[] = [];
  constructor(opts: Partial<Pick<InputDetail, 'smartQuotes' | 'emDash' | 'ellipsis' | 'collapseSpace'>> = {}) {
    this.smartQuotes = opts.smartQuotes ?? true;
    this.emDash = opts.emDash ?? true;
    this.ellipsis = opts.ellipsis ?? true;
    this.collapseSpace = opts.collapseSpace ?? true;
  }
  apply(text: string): string {
    let out = text;
    if (this.smartQuotes) {
      out = out.replace(/"([^"]*)"/g, '\u201c$1\u201d').replace(/'([^']*)'/g, '\u2018$1\u2019');
    }
    if (this.emDash) out = out.replace(/--/g, '\u2014');
    if (this.ellipsis) out = out.replace(/\.{3,}/g, '\u2026');
    if (this.collapseSpace) out = out.replace(/[ \t]{2,}/g, ' ');
    this.history.push(text);
    if (this.history.length > 8) this.history.shift();
    return out;
  }
  reset(): void {
    this.history = [];
  }
}

/* -------- 族0197 输入彩蛋 2.0（X04901~X04925）-------- */

/** 输入彩蛋：连续输入暗号触发（konami 风格序列匹配）。 */
export const INPUT_EGG_SEQUENCES: Record<string, string> = {
  'variable': 'egg-palette',
  'aurora': 'egg-confetti',
};

export class InputEgg {
  buffer: string[] = [];
  fired: string[] = [];
  enabled: boolean;
  constructor(enabled = true) {
    this.enabled = enabled;
  }
  feed(ch: string): string | null {
    if (!this.enabled) return null;
    this.buffer.push(ch.toLowerCase());
    if (this.buffer.length > 16) this.buffer.shift();
    const joined = this.buffer.join('');
    for (const [seq, egg] of Object.entries(INPUT_EGG_SEQUENCES)) {
      if (joined.endsWith(seq) && !this.fired.includes(egg)) {
        this.fired.push(egg);
        return egg;
      }
    }
    return null;
  }
  get firedCount(): number {
    return this.fired.length;
  }
}

/* -------- 族0198 输入迁移（X04926~X04950）-------- */

/** 输入偏好迁移包：导出/导入/版本携带/坏包钳制。 */
export interface InputPrefs {
  profile: CjkProfileId;
  smartQuotes: boolean;
  eggsEnabled: boolean;
}

export const INPUT_PREFS_VERSION = 1;

export function exportInputPrefs(p: InputPrefs): string {
  return JSON.stringify({ v: INPUT_PREFS_VERSION, ...p });
}

export function importInputPrefs(data: string): InputPrefs {
  const fallback: InputPrefs = { profile: DEFAULT_CJK_PROFILE, smartQuotes: true, eggsEnabled: true };
  try {
    const o = JSON.parse(data) as Partial<InputPrefs & { v: number }>;
    if (o.profile !== undefined && !(CJK_TYPOGRAPHY_PROFILES as readonly string[]).includes(o.profile)) {
      return fallback;
    }
    return {
      profile: (o.profile as InputPrefs['profile']) ?? fallback.profile,
      smartQuotes: o.smartQuotes ?? fallback.smartQuotes,
      eggsEnabled: o.eggsEnabled ?? fallback.eggsEnabled,
    };
  } catch {
    return fallback;
  }
}
