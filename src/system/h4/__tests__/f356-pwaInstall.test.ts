import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { auditListParity, fromManifest, installPwa, listPwaApps, pwaIdOf, uninstallPwa } from "../f356-pwaInstall";

const MANIFEST = {
  name: "Variable Docs",
  shortName: "Docs",
  startUrl: "https://docs.example.com/home",
  scope: "https://docs.example.com/",
  display: "standalone" as const,
  icons: [
    { src: "/i-32.png", sizes: "32x32" },
    { src: "/i-512.png", sizes: "512x512" },
  ],
  themeColor: "#101418",
};

describe("F356 PWA 应用化", () => {
  it("清单字段取用：short_name 优先、取最大图标、独立窗口三属性全 true", () => {
    const { app, problems } = fromManifest(MANIFEST, 1);
    expect(problems).toEqual([]);
    expect(app!.displayName).toBe("Docs");
    expect(app!.iconSrc).toBe("/i-512.png");
    expect(app!.windowProps).toEqual({ altTab: true, snappable: true, snapshotable: true });
    expect(app!.id).toBe(pwaIdOf(MANIFEST.startUrl));
  });

  it("清单缺 name / start_url / display=browser → 拒装并列出问题（零静默）", () => {
    expect(fromManifest({ ...MANIFEST, name: "" }, 1).problems).toContain("清单缺 name");
    expect(fromManifest({ ...MANIFEST, startUrl: "" }, 1).problems).toContain("清单缺 start_url");
    expect(fromManifest({ ...MANIFEST, display: "browser" }, 1).problems[0]).toContain("browser");
  });

  it("安装-卸载全链：幂等（重复安装拒绝）、卸载后册净", () => {
    __clearMem();
    const s = memStore();
    const r1 = installPwa(MANIFEST, 1, s);
    expect(r1.ok).toBe(true);
    const r2 = installPwa(MANIFEST, 2, s);
    expect(r2.ok).toBe(false);
    expect(r2.reason).toBe("exists");
    const un = uninstallPwa(r1.app!.id, [], [], s);
    expect(un.removed).toBe(true);
    expect(un.residue).toEqual([]);
    expect(listPwaApps(s)).toHaveLength(0);
  });

  it("卸载干净度扫描：快照/固定引用残留逐条报出", () => {
    __clearMem();
    const s = memStore();
    const { app } = installPwa(MANIFEST, 1, s)!;
    const id = app!.id;
    const un = uninstallPwa(id, [`snap:${id}:win1`], [`pin:${id}`], s);
    expect(un.residue).toHaveLength(2);
    expect(un.residue.map((r) => r.where)).toEqual(["snapshots", "pinned"]);
  });

  it("列表平权审计：装了处处有、名字图标齐；没装如实报不含", () => {
    const { app } = fromManifest(MANIFEST, 1);
    const rows = auditListParity(app);
    expect(rows).toHaveLength(4);
    expect(rows.every((r) => r.includes && r.fieldsParity)).toBe(true);
    expect(auditListParity(null).every((r) => !r.includes)).toBe(true);
  });
});
