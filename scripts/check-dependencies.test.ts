import { describe, expect, it } from "vitest";
import {
  assertSafeMerge,
  assertPatchedGraph,
  assertRustReport,
  unmitigatedAdvisories,
} from "./check-dependencies";
import merge from "deepmerge";

describe("dependency security", () => {
  it("rejects an additional unpatched copy and incomplete or failed audit reports", () => {
    const packages = { deepmerge: ["deepmerge@4.3.1"] };
    expect(() => assertPatchedGraph(packages)).not.toThrow();
    expect(() =>
      assertPatchedGraph({ ...packages, "nested/deepmerge": ["deepmerge@4.2.0"] }),
    ).toThrow();
    expect(() => assertPatchedGraph({})).toThrow();
    const good = { vulnerabilities: { found: false, count: 0, list: [] } };
    expect(() => assertRustReport(good, 0)).not.toThrow();
    for (const report of [
      {},
      { vulnerabilities: {} },
      { ...good, error: "offline" },
      { vulnerabilities: { found: true, count: 1, list: [{}] } },
    ]) {
      expect(() => assertRustReport(report, 0)).toThrow();
    }
    expect(() => assertRustReport(good, 1)).toThrow();
    expect(() => unmitigatedAdvisories(null as never, true)).toThrow();
    expect(() => unmitigatedAdvisories({ error: "offline" } as never, true)).toThrow();
  });
  it("rejects the actual poisoning counterexample and accepts the installed mitigation", () => {
    const unsafe = (target: object, source: object) => Object.assign({}, target, source);
    expect(() => assertSafeMerge(unsafe)).toThrow(/prototype-poisoning/);
    expect(() => assertSafeMerge(merge)).not.toThrow();
  });
  it("never suppresses other advisories or an unverified patch", () => {
    const report = { deepmerge: [{ url: "https://github.com/advisories/GHSA-gcqx-w48m-w2h4" }] };
    expect(unmitigatedAdvisories(report, false)).toHaveLength(1);
    expect(unmitigatedAdvisories(report, true)).toEqual([]);
    expect(
      unmitigatedAdvisories({ ...report, other: [{ url: report.deepmerge[0].url }] }, true),
    ).toHaveLength(1);
    expect(
      unmitigatedAdvisories({ deepmerge: [{ url: "https://github.com/advisories/new" }] }, true),
    ).toHaveLength(1);
  });
});
