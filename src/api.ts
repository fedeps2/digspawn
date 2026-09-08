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

export interface ServerStateEvent {
  server: string;
  state: string;
}

export interface LogLine {
  server: string;
  line: string;
}

export interface RuntimeProgress {
  server: string;
  version: number;
  downloaded: number;
  total: number | null;
  pct: number | null;
}

export interface HostStats {
  total_mb: number;
  used_mb: number;
  cpu_pct: number;
}

export interface ServerStat {
  name: string;
  pid: number;
  ram_mb: number;
  cpu_pct: number;
}

export interface AllStats {
  host: HostStats;
  servers: ServerStat[];
}

export interface Preflight {
  can_start: boolean;
  free_mb: number;
  needed_mb: number;
  warnings: string[];
}

export interface LogFile {
  file: string;
  kind: string; // "crash" | "latest" | "rotated"
  size: number;
  modified: number; // unix timestamp
}

export interface UpdateCheck {
  current: string;
  latest: string;
  url: string;
  notes: string;
  required: boolean;
  available: boolean;
  checked: boolean;
}

export interface ImportInput {
  path: string;
  name: string;
  server_type: ServerType;
  version: string;
  ram_mb: number;
  accept_eula: boolean;
}

export interface PluginInfo {
  file: string;
  enabled: boolean;
  size: number;
}

export interface Settings {
  check_updates_on_start: boolean;
  default_ram_mb: number;
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
    invoke<VersionItem[]>("list_versions", { serverType: server_type, includeSnapshots: include_snapshots }),
  createServer: (input: CreateInput) => invoke<ServerInfo>("create_server", { input }),
  deleteServer: (name: string) => invoke<void>("delete_server", { name }),
  detectJava: () => invoke<JavaInfo>("detect_java"),
  hostRamMb: () => invoke<number>("host_ram_mb"),
  requiredJava: (server_type: ServerType, mc_version: string) =>
    invoke<number>("required_java", { serverType: server_type, mcVersion: mc_version }),
  startServer: (name: string) => invoke<void>("start_server", { name }),
  stopServer: (name: string) => invoke<void>("stop_server", { name }),
  restartServer: (name: string) => invoke<void>("restart_server", { name }),
  sendCommand: (name: string, cmd: string) => invoke<void>("send_command", { name, cmd }),
  readLog: (name: string, max_lines: number) => invoke<string[]>("read_log", { name, maxLines: max_lines }),
  getProperties: (name: string) => invoke<Record<string, string>>("get_properties", { name }),
  setProperties: (name: string, kvs: Record<string, string>) =>
    invoke<void>("set_properties", { name, kvs }),
  setRam: (name: string, ram_mb: number) => invoke<void>("set_ram", { name, ramMb: ram_mb }),
  serverStats: () => invoke<AllStats>("server_stats"),
  preflight: (name: string) => invoke<Preflight>("preflight", { name }),
  listLogFiles: (name: string) => invoke<LogFile[]>("list_log_files", { name }),
  readLogFile: (name: string, file: string, max_lines: number) =>
    invoke<string[]>("read_log_file", { name, file, maxLines: max_lines }),
  setIcon: (name: string, data_url: string) => invoke<void>("set_icon", { name, dataUrl: data_url }),
  getIcon: (name: string) => invoke<string | null>("get_icon", { name }),
  localIps: () => invoke<string[]>("local_ips"),
  checkUpdate: () => invoke<UpdateCheck>("check_update"),
  importServer: (input: ImportInput) => invoke<ServerInfo>("import_server", { input }),
  listPlugins: (name: string) => invoke<PluginInfo[]>("list_plugins", { name }),
  importPlugin: (name: string, path: string) => invoke<string>("import_plugin", { name, path }),
  deletePlugin: (name: string, file: string) => invoke<void>("delete_plugin", { name, file }),
  setPluginEnabled: (name: string, file: string, enabled: boolean) =>
    invoke<string>("set_plugin_enabled", { name, file, enabled }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
};
