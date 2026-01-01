/* tslint:disable */
/* eslint-disable */
/**
 * The `ReadableStreamType` enum.
 *
 * *This API requires the following crate features to be activated: `ReadableStreamType`*
 */

type ReadableStreamType = "bytes";

export class IntoUnderlyingByteSource {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  pull(controller: ReadableByteStreamController): Promise<any>;
  start(controller: ReadableByteStreamController): void;
  cancel(): void;
  readonly autoAllocateChunkSize: number;
  readonly type: ReadableStreamType;
}

export class IntoUnderlyingSink {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  abort(reason: any): Promise<any>;
  close(): Promise<any>;
  write(chunk: any): Promise<any>;
}

export class IntoUnderlyingSource {
  private constructor();
  free(): void;
  [Symbol.dispose](): void;
  pull(controller: ReadableStreamDefaultController): Promise<any>;
  cancel(): void;
}

export function main(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly main: () => void;
  readonly __wbg_intounderlyingbytesource_free: (a: number, b: number) => void;
  readonly __wbg_intounderlyingsource_free: (a: number, b: number) => void;
  readonly intounderlyingbytesource_autoAllocateChunkSize: (a: number) => number;
  readonly intounderlyingbytesource_cancel: (a: number) => void;
  readonly intounderlyingbytesource_pull: (a: number, b: any) => any;
  readonly intounderlyingbytesource_start: (a: number, b: any) => void;
  readonly intounderlyingbytesource_type: (a: number) => number;
  readonly intounderlyingsource_cancel: (a: number) => void;
  readonly intounderlyingsource_pull: (a: number, b: any) => any;
  readonly __wbg_intounderlyingsink_free: (a: number, b: number) => void;
  readonly intounderlyingsink_abort: (a: number, b: any) => any;
  readonly intounderlyingsink_close: (a: number) => any;
  readonly intounderlyingsink_write: (a: number, b: any) => any;
  readonly wasm_bindgen__convert__closures_____invoke__h6ee248ed02ce3cbc: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__h201fa62fa5c0bcee: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__hb075b581bcd67d53: (a: number, b: number) => void;
  readonly wasm_bindgen__closure__destroy__h377e8c1898757784: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h0127d97b2d812171: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__hd2c7508029bc0c1a: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__h4b2e598a15dfdadf: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__he1c0e816b3fc610d: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__convert__closures_____invoke__hdb83041be653f6bd: (a: number, b: number) => void;
  readonly wasm_bindgen__closure__destroy__h7cce7eb7d9b3cae2: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h4cfd6ae0275b3657: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__hf642255ef13d4e6e: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h02e15d5ac2ae5f85: (a: number, b: number, c: any, d: any) => void;
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_externrefs: WebAssembly.Table;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
