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
  parseBibtex(input: string, maxEntries?: number): string;
  parseRis(input: string, maxEntries?: number): string;
  parseCslJson(input: string, maxEntries?: number): string;
  parseMedline(input: string, maxEntries?: number): string;
  detectFormat(input: string): 'bibtex' | 'ris' | 'csl-json' | 'medline' | 'unknown';
  parseAuto(input: string, maxEntries?: number): string;
  version(): string;
}

export declare function createEngine(): Promise<WasmCitationEngine>;
