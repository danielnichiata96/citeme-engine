// js/index.d.ts

export interface FormatResult {
  reference: string;
  inText: string;
}

export interface ParseResult {
  entries: unknown[];
  errors: Array<{ preview: string; error: string }>;
  format: string;
  truncated: boolean;
  scannedEntries: number;
}

export declare class WasmCitationEngine {
  constructor();
  loadStyle(name: string, cslXml: string): void;
  loadLocale(localeCode: string, localeXml: string): void;
  hasStyle(name: string): boolean;
  formatBatch(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
  formatOne(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
  formatBatchProse(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
  formatOneProse(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
  parseBibtex(input: string, maxEntries?: number): string;
  parseRis(input: string, maxEntries?: number): string;
  parseCslJson(input: string, maxEntries?: number): string;
  parseMedline(input: string, maxEntries?: number): string;
  detectFormat(input: string): 'bibtex' | 'ris' | 'csl-json' | 'medline' | 'unknown';
  parseAuto(input: string, maxEntries?: number): string;
  exportBibtex(cslJsonStr: string): string;
  exportRis(cslJsonStr: string): string;
  exportHayagriva(cslJsonStr: string): string;
  version(): string;
}

export declare function createEngine(): Promise<WasmCitationEngine>;

/**
 * Initialize the Wasm module. Call before constructing WasmCitationEngine.
 * Pass `{ module_or_path }` to specify an explicit URL for the .wasm binary
 * (required in bundled environments where import.meta.url is unreliable).
 */
export default function init(options?: { module_or_path: string | URL | RequestInfo | BufferSource | WebAssembly.Module }): Promise<void>;
