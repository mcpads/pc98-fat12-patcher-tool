/* tslint:disable */
/* eslint-disable */

export function applyPatchPackage(source: Uint8Array, _package: Uint8Array): Uint8Array;

export function classifyPatchArtifactInput(input: Uint8Array, artifact: Uint8Array): string;

export function materializePatchArtifactMember(input: Uint8Array, artifact: Uint8Array, member_key: string): Uint8Array;

export function maximumPatchArtifactBytes(): number;

export function maximumPatchPackageBytes(): number;

export function readPatchArtifactDefinition(artifact: Uint8Array): string;

export function readPatchPackageRecipe(_package: Uint8Array): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly applyPatchPackage: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly classifyPatchArtifactInput: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly materializePatchArtifactMember: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly maximumPatchArtifactBytes: () => number;
    readonly maximumPatchPackageBytes: () => number;
    readonly readPatchArtifactDefinition: (a: number, b: number) => [number, number, number, number];
    readonly readPatchPackageRecipe: (a: number, b: number) => [number, number, number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
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
