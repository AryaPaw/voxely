import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import path from "node:path";
import scenes from "./scenes.json";
import { publishShots } from "./publish";

if (process.platform !== "win32") throw new Error("Native README screenshots require Windows");
const args = process.argv.slice(2);
if (args.some((arg) => arg !== "--pilot")) throw new Error("Usage: bun run docs:shots [--pilot]");
const pilot = args.includes("--pilot");
const root = path.resolve(import.meta.dirname, "../..");
const work = path.join(root, ".local/readme-shots", `run-${crypto.randomUUID()}`);
await mkdir(work, { recursive: true });
await writeFile(
  path.join(work, "session.json"),
  JSON.stringify({ format: "voxely-native-readme", version: 1 }),
);

async function run(command: string[], timeout: number) {
  const child = Bun.spawn(command, {
    cwd: root,
    env: { ...process.env, VOXELY_SHOTS_WORK: work },
    stdout: "inherit",
    stderr: "inherit",
    timeout,
  });
  if ((await child.exited) !== 0) throw new Error(`Screenshot step failed. Diagnostics: ${work}`);
}

async function generate() {
  await run(
    [
      process.execPath,
      "x",
      "--bun",
      "tsc",
      "--noEmit",
      "--project",
      "scripts/readme-shots/tsconfig.json",
    ],
    120_000,
  );
  console.log(`Native screenshot workspace: ${work}`);
  await run(
    [
      process.execPath,
      "x",
      "--bun",
      "vite",
      "build",
      "--config",
      "scripts/readme-shots/native.vite.config.ts",
    ],
    120_000,
  );
  await run(
    [
      "pwsh",
      "-NoProfile",
      "-File",
      "scripts/readme-shots/capture-native.ps1",
      "-Work",
      work,
      ...(pilot ? ["-Pilot"] : []),
    ],
    900_000,
  );

  const captures = JSON.parse(await readFile(path.join(work, "capture.json"), "utf8"));
  const entries = Array.isArray(captures) ? captures : [captures];
  const expected = pilot ? scenes.slice(0, 1) : scenes;
  if (
    entries.length !== expected.length ||
    expected.some((scene, index) => entries[index].section !== scene.section)
  )
    throw new Error("Incomplete screenshot series");
  if (
    entries
      .filter((entry) => entry.section !== "hud")
      .some(
        (entry) =>
          entry.width !== entries[0].width ||
          entry.height !== entries[0].height ||
          entry.scale !== entries[0].scale,
      )
  ) {
    throw new Error("Capture geometry changed during the series; images weren't published");
  }
  if (!pilot) {
    await rename(path.join(work, "shot-hud.png"), path.join(work, "hud.png"));
    await publishShots(
      work,
      path.join(root, "docs/images"),
      entries.map((entry) => (entry.section === "hud" ? "hud.png" : `shot-${entry.section}.png`)),
    );
  }
  console.log(
    pilot
      ? `Pilot: ${path.join(work, "shot-history.png")}`
      : "README screenshots updated in docs/images.",
  );
  console.log(`Native metadata and logs: ${work}`);
}

try {
  await generate();
} finally {
  await run(
    ["pwsh", "-NoProfile", "-File", "scripts/readme-shots/archive.ps1", "-Work", work],
    60_000,
  );
}
