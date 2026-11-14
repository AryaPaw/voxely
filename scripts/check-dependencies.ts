import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);
const patchedAdvisory = "GHSA-gcqx-w48m-w2h4";

type AdvisoryMap = Record<string, Array<{ url?: string; title?: string }>>;

// Проверяется именно установленный код: номер upstream-пакета после Bun patch не меняется.
export function assertSafeMerge(merge: any): void {
  const hostile = () => JSON.parse('{"__proto__":{"voxelyPoisoned":true}}');
  for (const [target, source] of [
    [hostile(), {}],
    [{}, hostile()],
    [Object.create(null), hostile()],
    [hostile(), hostile()],
  ]) {
    const result = merge(target, source);
    if (Object.getPrototypeOf(result) !== Object.prototype || result.voxelyPoisoned !== undefined) {
      throw new Error("deepmerge prototype-poisoning regression: installed patch isn't effective");
    }
  }
  const nested = merge({ nested: hostile() }, { nested: {} });
  if (nested.nested.voxelyPoisoned !== undefined) throw new Error("Nested merge remains unsafe");
  const normal = merge({ values: [1], nested: { a: 1 } }, { values: [2], nested: { b: 2 } });
  if (JSON.stringify(normal) !== '{"values":[1,2],"nested":{"a":1,"b":2}}') {
    throw new Error("deepmerge patch changed ordinary merges");
  }
}

export function unmitigatedAdvisories(report: AdvisoryMap, verifiedPatch: boolean): string[] {
  if (!report || typeof report !== "object" || Array.isArray(report)) {
    throw new Error("Invalid JS advisory report");
  }
  for (const entries of Object.values(report)) {
    if (!Array.isArray(entries)) throw new Error("Invalid JS advisory entries");
  }
  return Object.entries(report).flatMap(([name, advisories]) =>
    advisories
      .filter(
        (entry) =>
          !(
            verifiedPatch &&
            name === "deepmerge" &&
            entry.url === `https://github.com/advisories/${patchedAdvisory}`
          ),
      )
      .map((entry) => `${name}: ${entry.url ?? entry.title ?? "unknown advisory"}`),
  );
}

export function assertPatchedGraph(packages: Record<string, unknown>): void {
  const entries = Object.entries(packages).filter(
    ([name, entry]) =>
      name === "deepmerge" ||
      name.endsWith("/deepmerge") ||
      (Array.isArray(entry) && typeof entry[0] === "string" && entry[0].includes("deepmerge@")),
  );
  if (
    !entries.length ||
    entries.some(([, entry]) => !Array.isArray(entry) || entry[0] !== "deepmerge@4.3.1")
  ) {
    throw new Error("Unverified deepmerge version in the dependency graph");
  }
}

export function assertRustReport(report: any, status: number): void {
  const found = report?.vulnerabilities;
  if (
    status !== 0 ||
    report?.error ||
    found?.found !== false ||
    found?.count !== 0 ||
    !Array.isArray(found?.list) ||
    found.list.length !== 0
  ) {
    throw new Error(`Rust audit failed: ${JSON.stringify(report?.error ?? found)}`);
  }
}

function verifyInstalledPatch(): void {
  const pkg = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));
  const patch = pkg.patchedDependencies?.["deepmerge@4.3.1"];
  if (patch !== "patches/deepmerge@4.3.1.patch" || !existsSync(resolve(root, patch))) {
    throw new Error("Required deepmerge patch is missing from the installation contract");
  }
  const lock = Bun.JSONC.parse(readFileSync(resolve(root, "bun.lock"), "utf8"));
  assertPatchedGraph(lock.packages);
  const cjs = require.resolve("deepmerge");
  const packageRoot = resolve(dirname(cjs), "..");
  const installed = JSON.parse(readFileSync(resolve(packageRoot, "package.json"), "utf8"));
  if (installed.version !== "4.3.1")
    throw new Error("Re-evaluate the upstream deepmerge mitigation");
  for (const entry of [cjs, resolve(packageRoot, "dist/umd.js")]) {
    assertSafeMerge(require(entry));
  }
}

function runJson(command: string, args: string[]): { status: number; data: any } {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    timeout: 180_000,
    maxBuffer: 8 * 1024 * 1024,
  });
  if (result.error || result.status === null || ![0, 1].includes(result.status)) {
    throw new Error(
      `${command} audit failed: ${result.error?.message ?? result.stderr.slice(-1500)}`,
    );
  }
  try {
    return { status: result.status, data: JSON.parse(result.stdout) };
  } catch {
    throw new Error(`${command} audit returned an invalid report: ${result.stderr.slice(-1500)}`);
  }
}

if (import.meta.main) {
  verifyInstalledPatch();
  const bun = runJson(process.execPath, ["audit", "--json"]);
  const outstanding = unmitigatedAdvisories(bun.data, true);
  if (outstanding.length) throw new Error(`Unmitigated JS advisories:\n${outstanding.join("\n")}`);
  if (bun.status !== 0 && !bun.data.deepmerge?.length)
    throw new Error("Bun audit failed without an advisory");
  console.log(
    `JS audit PASS; ${patchedAdvisory} mitigated by installed, regression-verified Bun patch`,
  );
  const localAudit = resolve(root, ".local/tools/bin/cargo-audit.exe");
  const rust = runJson(existsSync(localAudit) ? localAudit : "cargo-audit", [
    "audit",
    "--db",
    resolve(root, ".local/security/advisory-db"),
    "--file",
    resolve(root, "src-tauri/Cargo.lock"),
    "--json",
  ]);
  assertRustReport(rust.data, rust.status);
  console.log(
    "Rust audit PASS: no known vulnerabilities; informational warnings remain diagnostic",
  );
  for (const [kind, entries] of Object.entries(rust.data.warnings ?? {})) {
    if (!Array.isArray(entries)) throw new Error("Invalid RustSec warning report");
    for (const entry of entries.slice(0, 12)) {
      console.log(
        `RustSec ${kind}: ${entry.package?.name}@${entry.package?.version} ${entry.advisory?.id ?? ""}`,
      );
    }
    if (entries.length > 12)
      console.log(`RustSec ${kind}: ${entries.length - 12} additional warnings`);
  }
}
