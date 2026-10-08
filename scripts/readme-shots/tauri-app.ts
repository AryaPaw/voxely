import packageJson from "../../package.json";

export async function getVersion(): Promise<string> {
  return packageJson.version;
}
