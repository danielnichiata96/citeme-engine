// js/index.d.ts

export interface FormatResult {
  reference: string;
  inText: string;
}

export declare class WasmCitationEngine {
  constructor();
  loadStyle(name: string, cslXml: string): void;
  loadLocale(localeCode: string, localeXml: string): void;
  hasStyle(name: string): boolean;
  formatBatch(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
  formatOne(cslJsonStr: string, styleName: string, localeCode: string, abntPostProcess: boolean): string;
  version(): string;
}

export declare function createEngine(): Promise<WasmCitationEngine>;
