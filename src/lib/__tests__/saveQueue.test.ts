import { describe, expect, it, vi } from "vitest";
import { SaveCoordinator } from "../saveQueue";

describe("SaveCoordinator", () => {
  it("serializes tasks for the same key", async () => {
    const order: string[] = [];
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const c = new SaveCoordinator(async (p: string) => {
      order.push(`start:${p}`);
      if (p === "first") await gate;
      order.push(`end:${p}`);
    });
    const p1 = c.submit("doc1", "first");
    // wait until first starts
    await vi.waitFor(() => expect(order).toContain("start:first"));
    const p2 = c.submit("doc1", "second");
    release();
    await Promise.all([p1, p2]);
    expect(order.indexOf("end:first")).toBeLessThan(order.indexOf("start:second"));
    expect(order).toContain("end:second");
  });

  it("latest-wins: queued-but-not-started tasks are dropped", async () => {
    const executed: unknown[] = [];
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const c = new SaveCoordinator(async (p: { v: number }) => {
      executed.push(p);
      if (p.v === 1) await gate;
    });
    const p1 = c.submit("k", { v: 1 });
    await vi.waitFor(() => expect(executed.length).toBe(1));
    const p2 = c.submit("k", { v: 2 }); // queued while v1 runs
    const p3 = c.submit("k", { v: 3 }); // replaces v2 before it starts
    release();
    await Promise.all([p1, p2, p3]);
    expect(executed.map((e) => (e as { v: number }).v).sort()).toEqual([1, 3]);
  });

  it("different keys run independently", async () => {
    const done = new Set<string>();
    const c = new SaveCoordinator(async (p: string) => {
      done.add(p);
    });
    await Promise.all([c.submit("a", "a"), c.submit("b", "b")]);
    expect(done.has("a")).toBe(true);
    expect(done.has("b")).toBe(true);
  });

  it("propagates task errors after retries and keeps draining", async () => {
    let calls = 0;
    const c = new SaveCoordinator(async (p: number) => {
      calls++;
      if (p === 1) throw new Error("boom");
    }, { attempts: 3, backoffMs: 1 });
    await expect(c.submit("x", 1)).rejects.toThrow("boom");
    expect(calls).toBe(3); // exhausted all attempts before rejecting
    await expect(c.submit("x", 2)).resolves.toBeUndefined();
  });

  it("retries transient failures and recovers", async () => {
    let calls = 0;
    const c = new SaveCoordinator(async () => {
      calls++;
      if (calls === 1) throw new Error("transient");
    }, { backoffMs: 1 });
    await expect(c.submit("y", 7)).resolves.toBeUndefined();
    expect(calls).toBe(2);
  });
});
