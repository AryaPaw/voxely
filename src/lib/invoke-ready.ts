export const MANAGED_RETRY_MS = 50;
export const MANAGED_DEADLINE_MS = 10_000;

export function isStateNotManagedError(error: unknown): boolean {
  return errorText(error).toLowerCase().includes("state not managed");
}

export async function invokeWhenManaged<T>(
  run: (cmd: string, args?: Record<string, unknown>) => Promise<T>,
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const started = Date.now();
  for (;;) {
    try {
      return await run(cmd, args);
    } catch (error) {
      if (!isStateNotManagedError(error) || Date.now() - started >= MANAGED_DEADLINE_MS) {
        throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, MANAGED_RETRY_MS));
    }
  }
}

function errorText(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message: unknown }).message;
    if (typeof message === "string") {
      return message;
    }
  }
  return String(error ?? "");
}
