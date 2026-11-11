import { expect, it } from "vitest";
import { dirname, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import config from "../vite.config";

it("keeps frontend cleanup disjoint from Rust coverage and critical-test reports", () => {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  const frontend = resolve(root, config.test?.coverage?.reportsDirectory ?? "coverage");
  const rust = resolve(root, "coverage/rust");
  expect(relative(root, frontend)).not.toBe("");
  expect(relative(root, frontend).startsWith(`..${sep}`)).toBe(false);
  expect(relative(frontend, rust).startsWith(`..${sep}`)).toBe(true);
  expect(relative(rust, frontend).startsWith(`..${sep}`)).toBe(true);
});
