// Placeholder written by hand for M0 step 2; replaced by the tauri-specta export in step 3.
import { invoke as TAURI_INVOKE } from '@tauri-apps/api/core';

export const commands = {
  async appInfo(): Promise<Result<AppInfo, ErrorDto>> {
    try {
      return { status: 'ok', data: await TAURI_INVOKE('app_info') };
    } catch (e) {
      if (e instanceof Error) throw e;
      else return { status: 'error', error: e as ErrorDto };
    }
  },
};

export type AppInfo = {
  version: string;
  os: string;
  arch: string;
  data_dir: string;
  log_dir: string;
  debug: boolean;
};
export type ErrorDto =
  | { kind: 'internal'; message: string }
  | { kind: 'invalid_input'; message: string }
  | { kind: 'not_found'; message: string }
  | { kind: 'io'; message: string };

export type Result<T, E> = { status: 'ok'; data: T } | { status: 'error'; error: E };
