import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path: string) => readFileSync(resolve(root, path), "utf8");

// Match `git rev-parse --local-env-vars`: don't let a parent hook route these
// repository-owned commands to another worktree, index or object database.
function gitEnvironment(): NodeJS.ProcessEnv {
  const environment = { ...process.env };
  const localNames = new Set([
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
  ]);
  for (const name of Object.keys(environment)) {
    if (localNames.has(name.toUpperCase())) delete environment[name];
  }
  return environment;
}

function run(command: string, args: string[], input?: string): string {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: "utf8",
    input,
    timeout: 30_000,
    maxBuffer: 4 * 1024 * 1024,
    env: { ...(command === "git" ? gitEnvironment() : process.env), RUSTUP_AUTO_INSTALL: "0" },
  });
  if (result.error || result.status !== 0) {
    throw new Error(
      `${command} failed: ${result.error?.message ?? result.stderr?.slice(-2000) ?? result.status}`,
    );
  }
  return result.stdout;
}

export function checkVersion(name: string, actual: string, expected: string) {
  if (actual !== expected) throw new Error(`${name}: expected ${expected}, got ${actual}`);
}

export function checkRustFile(path: string, contents: Buffer, eol: string) {
  if (eol !== "lf") throw new Error(`${path}: Git eol must be lf, got ${eol}`);
  if (contents.includes(13)) throw new Error(`${path}: CR/CRLF found; rustfmt requires LF`);
}

export function checkWorkflow(workflow: any, expectedBun: string, required: string[]) {
  const steps = Object.values(workflow.jobs).flatMap((job: any) => job.steps ?? []);
  const setup = steps.filter((step: any) => step.uses?.startsWith("oven-sh/setup-bun@"));
  if (!setup.length) throw new Error("CI Bun setup is missing");
  for (const step of setup) checkVersion("CI Bun", String(step.with?.["bun-version"]), expectedBun);
  const runs = steps.flatMap((step: any) =>
    String(step.run ?? "")
      .split(/\r?\n/)
      .map((line) => line.trim()),
  );
  for (const name of required) {
    if (runs.filter((line: string) => line === `bun run ${name}`).length !== 1) {
      throw new Error(`CI must run exactly once: bun run ${name}`);
    }
    for (const job of Object.values(workflow.jobs) as any[]) {
      for (const step of job.steps ?? []) {
        if (
          String(step.run ?? "")
            .split(/\r?\n/)
            .some((line) => line.trim() === `bun run ${name}`)
        ) {
          if (
            step.if !== undefined ||
            job.if !== undefined ||
            step["continue-on-error"] !== undefined ||
            job["continue-on-error"] !== undefined
          ) {
            throw new Error(`CI command must be unconditional and fatal: ${name}`);
          }
        }
      }
    }
  }
}

function installHooks() {
  const current = spawnSync("git", ["config", "--get", "core.hooksPath"], {
    cwd: root,
    encoding: "utf8",
    timeout: 30_000,
    env: gitEnvironment(),
  });
  if (current.error || ![0, 1].includes(current.status ?? -1))
    throw new Error("Cannot inspect core.hooksPath");
  const configured = current.stdout.trim();
  if (configured && configured !== ".githooks")
    throw new Error(`Existing core.hooksPath: ${configured}; refusing to replace it`);
  if (!configured) {
    const hooks = resolve(root, run("git", ["rev-parse", "--git-path", "hooks"]).trim());
    if (readdirSync(hooks).some((name) => !name.endsWith(".sample")))
      throw new Error("Existing Git hooks; refusing to replace them");
  }
  run("git", ["config", "--local", "core.hooksPath", ".githooks"]);
  console.log("Repository hooks enabled. Undo: git config --local --unset core.hooksPath");
}

function checkPush(input: string) {
  const head = run("git", ["rev-parse", "HEAD"]).trim();
  for (const line of input.trim().split(/\r?\n/).filter(Boolean)) {
    const fields = line.split(" ");
    if (fields.length !== 4) throw new Error("Invalid pre-push input");
    if (!/^0+$/.test(fields[1]) && fields[1] !== head)
      throw new Error("Push must target the checked HEAD; check out the intended commit first");
  }
  run("git", ["diff", "--quiet", "HEAD", "--"]);
  const untracked = run("git", [
    "ls-files",
    "--others",
    "--exclude-standard",
    "--",
    "src/",
    "src-tauri/",
    "scripts/",
    ".github/",
    ".githooks/",
    ":(top,glob)*.json",
    ":(top,glob)*.toml",
    ":(top,glob)*.ts",
  ]);
  if (untracked.trim())
    throw new Error(
      "Untracked verification inputs found; include intended files in the commit before pushing",
    );
  return head;
}

function main() {
  if (process.argv.includes("--install-hooks")) return installHooks();
  const pkg = JSON.parse(read("package.json"));
  const expectedBun = /^bun@(\d+\.\d+\.\d+)$/.exec(pkg.packageManager)?.[1];
  const toolchain = Bun.TOML.parse(read("rust-toolchain.toml")) as any;
  const expectedRust = toolchain.toolchain?.channel;
  if (!expectedBun || !/^\d+\.\d+\.\d+$/.test(expectedRust))
    throw new Error("Bun and Rust must have exact version pins");
  checkVersion("Bun", Bun.version, expectedBun);
  for (const name of ["RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"]) {
    if (process.env[name])
      throw new Error(`${name} overrides the canonical compiler; unset it before verification`);
  }
  checkVersion("Rust", run("rustc", ["--version"]).trim().split(/\s+/)[1], expectedRust);
  const required = pkg.scripts.verify.split(" && ").map((command: string) => {
    if (!/^bun run verify:[\w:-]+$/.test(command))
      throw new Error(`verify must use shared commands: ${command}`);
    const name = command.slice("bun run ".length);
    if (!pkg.scripts[name]) throw new Error(`Missing verification script: ${name}`);
    return name;
  });
  checkWorkflow(Bun.YAML.parse(read(".github/workflows/verify.yml")), expectedBun, required);
  checkWorkflow(Bun.YAML.parse(read(".github/workflows/release.yml")), expectedBun, []);
  const files = run("git", ["ls-files", "-z", "--", "*.rs"]).split("\0").filter(Boolean);
  if (!files.length) throw new Error("No tracked Rust files found");
  const attributes = run(
    "git",
    ["check-attr", "-z", "eol", "--stdin"],
    files.join("\0") + "\0",
  ).split("\0");
  for (let i = 0; i < files.length; i++) {
    if (attributes[i * 3] !== files[i]) throw new Error("Invalid Git attribute response");
    checkRustFile(files[i], readFileSync(resolve(root, files[i])), attributes[i * 3 + 2]);
  }
  const pushGate = process.argv.includes("--push-gate");
  if (process.argv.includes("--push") || pushGate) {
    const checkedHead = checkPush(readFileSync(0, "utf8"));
    if (pushGate) {
      const result = spawnSync(process.execPath, ["run", "verify"], {
        cwd: root,
        stdio: "inherit",
        timeout: 3_600_000,
        env: gitEnvironment(),
      });
      if (result.error || result.status !== 0)
        throw new Error(`Verification failed: ${result.error?.message ?? result.status}`);
      if (checkPush("") !== checkedHead) throw new Error("HEAD changed during verification");
    }
  }
  console.log(
    `Preflight PASS: Rust ${expectedRust}, Bun ${expectedBun}, ${files.length} Rust files with LF, shared CI commands`,
  );
}

if (import.meta.main) {
  try {
    main();
  } catch (error) {
    console.error(`Preflight FAIL: ${(error as Error).message}`);
    process.exitCode = 1;
  }
}
