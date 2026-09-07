// Wrappers tipados de los comandos Tauri del hito 2.

import { invoke } from "@tauri-apps/api/core";

export type ServerType = "paper" | "vanilla";

export interface ServerInfo {
  name: string;
  type: ServerType;
  version: string;
  ram_mb: number;
  state: string;
}

export interface VersionItem {
  id: string;
  kind: string;
  min_java: number | null;
}

export interface JavaInfo {
  path: string;
  version: number;
  raw: string;
}

/** Error rico del backend: `{ kind, message }`. */
export interface ServerError {
  kind: string;
  message: string;
}

export interface CreateInput {
  name: string;
  server_type: ServerType;
  version: string;
  ram_mb: number;
  accept_eula: boolean;
}

export interface DownloadProgress {
  server: string;
  downloaded: number;
  total: number | null;
  pct: number | null;
}

/** Extrae el mensaje legible de un fallo de `invoke`. */
export function errMsg(e: unknown): string {
  if (typeof e === "string") return e;
  if (typeof e === "object" && e !== null && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}

export const api = {
  listServers: () => invoke<ServerInfo[]>("list_servers"),
  listVersions: (server_type: ServerType, include_snapshots: boolean) =>
    invoke<VersionItem[]>("list_versions", { server_type, include_snapshots }),
  createServer: (input: CreateInput) => invoke<ServerInfo>("create_server", { input }),
  deleteServer: (name: string) => invoke<void>("delete_server", { name }),
  detectJava: () => invoke<JavaInfo>("detect_java"),
  hostRamMb: () => invoke<number>("host_ram_mb"),
  requiredJava: (mc_version: string) => invoke<number>("required_java", { mc_version }),
};
