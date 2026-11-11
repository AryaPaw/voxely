import { copyFile, mkdir, mkdtemp, rename, rm, stat } from "node:fs/promises";
import path from "node:path";

// Prepare the entire series first. Each replacement is atomic; any failure
// restores every replacement already made, including previously absent files.
export async function publishShots(
  work: string,
  destination: string,
  names: string[],
  replace: typeof rename = rename,
) {
  if (
    names.some((name) => !/^(shot-[a-z-]+|hud)\.png$/.test(name)) ||
    new Set(names).size !== names.length
  ) {
    throw new Error("Invalid screenshot publication set");
  }
  const stage = await mkdtemp(path.join(destination, ".readme-stage-"));
  const changed: string[] = [];
  const existing = new Set<string>();
  let recoverable = true;
  try {
    await mkdir(path.join(stage, "previous"));
    for (const name of names) {
      await copyFile(path.join(work, name), path.join(stage, name));
      try {
        await stat(path.join(destination, name));
        await copyFile(path.join(destination, name), path.join(stage, "previous", name));
        existing.add(name);
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
      }
    }
    for (const name of names) {
      await replace(path.join(stage, name), path.join(destination, name));
      changed.push(name);
    }
  } catch (failure) {
    const errors: unknown[] = [failure];
    for (const name of changed.reverse()) {
      try {
        if (existing.has(name))
          await replace(path.join(stage, "previous", name), path.join(destination, name));
        else await rm(path.join(destination, name));
      } catch (error) {
        errors.push(error);
        recoverable = false;
      }
    }
    if (!recoverable)
      throw new AggregateError(
        errors,
        `Publication rollback failed; originals retained in ${stage}`,
      );
    throw failure;
  } finally {
    if (recoverable) await rm(stage, { recursive: true });
  }
}
