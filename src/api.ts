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
  sha256: string;
  available: boolean;
  checked: boolean;
}

export interface UpdateProgress {
  downloaded: number;
  total: number | null;
  pct: number | null;
}

export interface DownloadReport {
  staged: boolean;
  total: number | null;
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

export interface SearchHit {
  project_id: string;
  title: string;
  author: string;
  description: string;
  downloads: number;
  icon_url: string | null;
  game_versions: string[];
}

export interface GalleryItem {
  url: string;
  title: string;
}

export interface ProjectDetails {
  project_id: string;
  title: string;
  body: string;
  gallery: GalleryItem[];
}

export interface InstallReport {
  installed: string[];
  skipped: string[];
  optional_deps: string[];
  version_label: string;
}

export interface PluginProgress {
  server: string;
  file: string;
  downloaded: number;
  total: number | null;
  pct: number | null;
}

export interface CrashDiagnosis {
  cause: string;
  hint: string;
}

export interface BackupInfo {
  file: string;
  scope: string; // "full" | "world"
  kind: string; // "manual" | "auto" | "onstart"
  size: number;
  modified: number; // unix timestamp
}

export interface BackupProgress {
  server: string;
  file: string;
  files_done: number;
  files_total: number;
  bytes_done: number;
  bytes_total: number;
  pct: number | null;
}

export interface BackupStateEvent {
  server: string;
  backing_up: boolean;
}

export interface BackupConfig {
  auto_enabled: boolean;
  auto_hours: number;
  auto_scope: string; // "full" | "world"
  keep_count: number;
  keep_gb: number;
  on_start_enabled: boolean;
  on_start_scope: string; // "full" | "world"
  onstart_keep_count: number;
  onstart_keep_gb: number;
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
  renameServer: (old_name: string, new_name: string) =>
    invoke<string>("rename_server", { oldName: old_name, newName: new_name }),
  serverDirPath: (name: string) => invoke<string>("server_dir_path", { name }),
  detectJava: () => invoke<JavaInfo>("detect_java"),
  hostRamMb: () => invoke<number>("host_ram_mb"),
  requiredJava: (server_type: ServerType, mc_version: string) =>
    invoke<number>("required_java", { serverType: server_type, mcVersion: mc_version }),
  startServer: (name: string) => invoke<void>("start_server", { name }),
  stopServer: (name: string) => invoke<void>("stop_server", { name }),
  forceStop: (name: string) => invoke<void>("force_stop", { name }),
  restartServer: (name: string) => invoke<void>("restart_server", { name }),
  sendCommand: (name: string, cmd: string) => invoke<void>("send_command", { name, cmd }),
  readLog: (name: string, max_lines: number) => invoke<string[]>("read_log", { name, maxLines: max_lines }),
  diagnoseCrash: (name: string) => invoke<CrashDiagnosis | null>("diagnose_crash", { name }),
  listBackups: (name: string) => invoke<BackupInfo[]>("list_backups", { name }),
  createBackup: (name: string, scope: string) => invoke<BackupInfo>("create_backup", { name, scope }),
  restoreBackup: (name: string, file: string) => invoke<void>("restore_backup", { name, file }),
  deleteBackup: (name: string, file: string) => invoke<void>("delete_backup", { name, file }),
  isBackingUp: (name: string) => invoke<boolean>("is_backing_up", { name }),
  getBackupConfig: (name: string) => invoke<BackupConfig>("get_backup_config", { name }),
  setBackupConfig: (name: string, cfg: BackupConfig) => invoke<BackupConfig>("set_backup_config", { name, cfg }),
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
  downloadUpdate: () => invoke<DownloadReport>("download_update"),
  applyUpdate: () => invoke<void>("apply_update"),
  rollbackAvailable: () => invoke<boolean>("rollback_available"),
  rollbackUpdate: () => invoke<void>("rollback_update"),
  importServer: (input: ImportInput) => invoke<ServerInfo>("import_server", { input }),
  // TEMPORAL diagnóstico: no await, best-effort.
  debugLog: (msg: string) => invoke<void>("debug_log", { msg }).catch(() => undefined),
  listPlugins: (name: string) => invoke<PluginInfo[]>("list_plugins", { name }),
  importPlugin: (name: string, path: string) => invoke<string>("import_plugin", { name, path }),
  deletePlugin: (name: string, file: string) => invoke<void>("delete_plugin", { name, file }),
  setPluginEnabled: (name: string, file: string, enabled: boolean) =>
    invoke<string>("set_plugin_enabled", { name, file, enabled }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  searchPlugins: (query: string, category: string | null) =>
    invoke<SearchHit[]>("search_plugins", { query, category }),
  pluginDetails: (project_id: string) =>
    invoke<ProjectDetails>("plugin_details", { projectId: project_id }),
  installPlugin: (server_name: string, project_id: string) =>
    invoke<InstallReport>("install_plugin", { serverName: server_name, projectId: project_id }),
};
