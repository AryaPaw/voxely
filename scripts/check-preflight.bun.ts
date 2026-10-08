import { describe, expect, test } from "bun:test";
import { checkRustFile, checkVersion, checkWorkflow } from "./check-preflight";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

describe("verification preflight", () => {
  test("rejects compiler/runtime version drift", () => {
    expect(() => checkVersion("Rust", "1.99.0", "1.98.1")).toThrow("expected 1.98.1");
    expect(() => checkVersion("Bun", "1.4.3", "1.4.2")).toThrow("expected 1.4.2");
    expect(() => checkVersion("Rust", "1.98.1", "1.98.1")).not.toThrow();
  });
  test("rejects CRLF and checkout attributes that allow CRLF", () => {
    expect(() => checkRustFile("a.rs", Buffer.from("fn main() {}\r\n"), "lf")).toThrow("CR/CRLF");
    expect(() => checkRustFile("a.rs", Buffer.from("fn main() {}\n"), "unspecified")).toThrow(
      "Git eol",
    );
    expect(() => checkRustFile("a.rs", Buffer.from("fn main() {}\n"), "lf")).not.toThrow();
  });
  const workflow = (version = "1.4.2", run = "bun run verify:frontend") => ({
    jobs: {
      windows: {
        steps: [{ uses: "oven-sh/setup-bun@pinned", with: { "bun-version": version } }, { run }],
      },
    },
  });
  test("rejects CI version drift and missing or duplicate shared commands", () => {
    expect(() => checkWorkflow(workflow(), "1.4.2", ["verify:frontend"])).not.toThrow();
    expect(() => checkWorkflow(workflow("1.4.3"), "1.4.2", [])).toThrow("CI Bun");
    expect(() =>
      checkWorkflow(workflow("1.4.2", "# bun run verify:frontend"), "1.4.2", ["verify:frontend"]),
    ).toThrow("exactly once");
    expect(() =>
      checkWorkflow(
        workflow("1.4.2", "bun run verify:frontend\nbun run verify:frontend"),
        "1.4.2",
        ["verify:frontend"],
      ),
    ).toThrow("exactly once");
  });
  test("rejects skipped and non-fatal CI checks", () => {
    for (const flag of ["if", "continue-on-error"]) {
      const fixture = workflow();
      (fixture.jobs.windows.steps[1] as any)[flag] = false;
      expect(() => checkWorkflow(fixture, "1.4.2", ["verify:frontend"])).toThrow(
        "unconditional and fatal",
      );
    }
  });

  test("real Windows checkout and pre-push hook reject broken candidates", () => {
    const source = resolve(dirname(fileURLToPath(import.meta.url)), "..");
    const temporary = mkdtempSync(join(tmpdir(), "voxely-preflight-"));
    const fixture = join(temporary, "work");
    mkdirSync(fixture);
    const command = (args: string[], cwd = fixture, inherited: NodeJS.ProcessEnv = {}) => {
      const isolatedEnv = Object.fromEntries(
        Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith("GIT_")),
      );
      const result = spawnSync(args[0], args.slice(1), {
        cwd,
        encoding: "utf8",
        timeout: 30_000,
        maxBuffer: 1024 * 1024,
        env: {
          ...isolatedEnv,
          RUSTUP_AUTO_INSTALL: "0",
          GIT_CONFIG_GLOBAL: join(temporary, "no-global-config"),
          GIT_CONFIG_NOSYSTEM: "1",
          ...inherited,
        },
      });
      if (result.error) throw result.error;
      return { code: result.status, output: result.stdout + result.stderr };
    };
    const git = (...args: string[]) => {
      const result = command(["git", ...args]);
      if (result.code !== 0) throw new Error(result.output);
      return result.output;
    };
    const savedGit = Object.fromEntries(
      ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"].map((key) => [key, process.env[key]]),
    );
    try {
      const sentinel = join(temporary, "sentinel");
      expect(command(["git", "init", "-q", sentinel], temporary).code).toBe(0);
      const sentinelGit = join(sentinel, ".git");
      const sentinelConfig = readFileSync(join(sentinelGit, "config"), "utf8");
      const sentinelHead = readFileSync(join(sentinelGit, "HEAD"), "utf8");
      process.env.GIT_DIR = sentinelGit;
      process.env.GIT_WORK_TREE = sentinel;
      process.env.GIT_INDEX_FILE = join(sentinelGit, "index");
      for (const path of [
        "scripts/check-preflight.ts",
        "scripts/check-rust-coverage.ps1",
        ".githooks/pre-push",
        ".github/workflows/verify.yml",
        ".github/workflows/release.yml",
        "rust-toolchain.toml",
        ".gitattributes",
      ]) {
        mkdirSync(dirname(join(fixture, path)), { recursive: true });
        cpSync(join(source, path), join(fixture, path));
      }
      const pkg = JSON.parse(readFileSync(join(source, "package.json"), "utf8"));
      pkg.scripts["verify:preflight"] = "bun scripts/check-preflight.ts";
      pkg.scripts["verify:frontend"] = 'bun -e "process.exit(23)"';
      writeFileSync(join(fixture, "package.json"), JSON.stringify(pkg));
      writeFileSync(join(fixture, "fixture.rs"), "fn main() {}\n");
      git("init", "-q");
      // Disposable fixture identity, never the user's repository configuration.
      git("config", "user.name", "Voxely guard fixture");
      git("config", "user.email", "fixture@example.invalid");
      git("add", ".");
      git("commit", "-qm", "fixture");
      const clone = join(temporary, "checkout");
      expect(
        command(
          ["git", "-c", "core.autocrlf=true", "clone", "--no-local", fixture, clone],
          temporary,
        ).code,
      ).toBe(0);
      expect(readFileSync(join(clone, "fixture.rs")).includes(13)).toBe(false);
      expect(command(["bun", "scripts/check-preflight.ts"], clone).code).toBe(0);
      const routing = {
        GIT_DIR: sentinelGit,
        GIT_WORK_TREE: sentinel,
        GIT_INDEX_FILE: join(sentinelGit, "index"),
        GIT_COMMON_DIR: sentinelGit,
      };
      const routed = command(["bun", "scripts/check-preflight.ts"], clone, routing);
      expect(routed.code).toBe(0);
      expect(routed.output).toContain("1 Rust files with LF");
      expect(command(["bun", "scripts/check-preflight.ts", "--push"], clone, routing).code).toBe(0);
      expect(
        command(["bun", "scripts/check-preflight.ts", "--install-hooks"], clone, routing).code,
      ).toBe(0);
      expect(command(["git", "config", "--get", "core.hooksPath"], clone).output.trim()).toBe(
        ".githooks",
      );
      const mixedRouting = Object.fromEntries(
        Object.entries(routing).map(([key, value]) => [key.toLowerCase(), value]),
      );
      expect(command(["bun", "scripts/check-preflight.ts"], clone, mixedRouting).code).toBe(0);
      expect(
        command(["bun", "scripts/check-preflight.ts", "--push"], clone, mixedRouting).code,
      ).toBe(0);
      expect(
        command(["bun", "scripts/check-preflight.ts", "--install-hooks"], clone, mixedRouting).code,
      ).toBe(0);
      writeFileSync(join(fixture, "fixture.rs"), "fn main() {}\r\n");
      const crlf = command(["bun", "scripts/check-preflight.ts"]);
      expect(crlf.code).not.toBe(0);
      expect(crlf.output).toContain("CR/CRLF");
      writeFileSync(join(fixture, "fixture.rs"), "fn main() {}\n");
      mkdirSync(join(fixture, "src"));
      writeFileSync(join(fixture, "src/uncommitted.ts"), "export {};\n");
      const untracked = command(["bun", "scripts/check-preflight.ts", "--push"]);
      expect(untracked.code).not.toBe(0);
      expect(untracked.output).toContain("Untracked verification inputs");
      rmSync(join(fixture, "src/uncommitted.ts"));
      expect(command(["bun", "scripts/check-preflight.ts", "--install-hooks"]).code).toBe(0);
      const shell = Bun.which("sh") ?? resolve(dirname(Bun.which("git")!), "../bin/sh.exe");
      const routedHook = command([shell, ".githooks/pre-push"], fixture, mixedRouting);
      expect(routedHook.code).not.toBe(0);
      expect(routedHook.output).toContain('script "verify:frontend" exited with code 23');
      git("init", "--bare", "-q", join(temporary, "remote.git"));
      const pushed = command([
        "git",
        "push",
        join(temporary, "remote.git"),
        "HEAD:refs/heads/main",
      ]);
      expect(pushed.code).not.toBe(0);
      expect(pushed.output).toContain('script "verify:frontend" exited with code 23');
      expect(command(["git", "--git-dir", join(temporary, "remote.git"), "show-ref"]).code).toBe(1);
      // A successful verify that changes its inputs must still block publication.
      pkg.scripts.verify = "bun run verify:preflight";
      pkg.scripts["verify:preflight"] =
        `bun -e "require('fs').appendFileSync('fixture.rs', '// changed')"`;
      writeFileSync(join(fixture, "package.json"), JSON.stringify(pkg));
      git("add", "package.json");
      git("commit", "-qm", "mutating gate fixture");
      expect(
        command(["git", "push", join(temporary, "remote.git"), "HEAD:refs/heads/main"]).code,
      ).not.toBe(0);
      expect(command(["git", "--git-dir", join(temporary, "remote.git"), "show-ref"]).code).toBe(1);
      mkdirSync(join(fixture, "src-tauri"));
      const missingTool = spawnSync(
        Bun.which("pwsh")!,
        ["-NoProfile", "-File", join(fixture, "scripts/check-rust-coverage.ps1")],
        {
          cwd: fixture,
          encoding: "utf8",
          timeout: 10_000,
          env: { ...process.env, PATH: "" },
        },
      );
      expect(missingTool.status).not.toBe(0);
      expect(missingTool.stdout + missingTool.stderr).toContain("cargo-llvm-cov is required");
      expect(readFileSync(join(sentinelGit, "config"), "utf8")).toBe(sentinelConfig);
      expect(readFileSync(join(sentinelGit, "HEAD"), "utf8")).toBe(sentinelHead);
      expect(existsSync(join(sentinelGit, "index"))).toBe(false);
    } finally {
      for (const [key, value] of Object.entries(savedGit)) {
        if (value === undefined) delete process.env[key];
        else process.env[key] = value;
      }
      if (
        dirname(temporary) !== resolve(tmpdir()) ||
        !temporary.startsWith(join(tmpdir(), "voxely-preflight-"))
      )
        throw new Error("Unsafe fixture cleanup");
      rmSync(temporary, { recursive: true, force: true });
    }
  }, 120_000);
});
