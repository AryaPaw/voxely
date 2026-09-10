export async function listen(_event: string, _handler: (event: { payload: unknown }) => void) {
  return () => undefined;
}
