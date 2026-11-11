import { afterEach, describe, expect, it } from "vitest";
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { publishShots } from "./publish";
import { rename } from "node:fs/promises";

let root = "";
afterEach(async () => {
  if (root) await rm(root, { recursive: true });
});

describe("native screenshot publication", () => {
  it("restores all existing images when the second replacement fails", async () => {
    root = await mkdtemp(path.join(tmpdir(), "voxely-shots-test-"));
    const work = path.join(root, "work"),
      output = path.join(root, "output");
    await mkdir(work);
    await mkdir(output);
    for (const name of ["shot-history.png", "shot-statistics.png"]) {
      await writeFile(path.join(work, name), `new ${name}`);
      await writeFile(path.join(output, name), `original ${name}`);
    }
    let replacements = 0;
    const replace: typeof rename = async (from, to) => {
      if (++replacements === 2)
        throw Object.assign(new Error("locked destination"), { code: "EPERM" });
      return rename(from, to);
    };
    await expect(
      publishShots(work, output, ["shot-history.png", "shot-statistics.png"], replace),
    ).rejects.toThrow("locked destination");
    for (const name of ["shot-history.png", "shot-statistics.png"]) {
      expect(await readFile(path.join(output, name), "utf8")).toBe(`original ${name}`);
    }
    expect((await readdir(output)).sort()).toEqual(["shot-history.png", "shot-statistics.png"]);
  });
});
