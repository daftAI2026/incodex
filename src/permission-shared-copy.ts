import { ACCESSIBILITY_OFFICIAL_COPY } from "./runtime/incognito-accessibility-official-copy-data.ts";

type Copy = Record<string, string>;

// Preserve the already-localized install explanation. The native CLI selects
// this or the official-app explanation after verifying the target identity.
export function sharedPermissionCopy(catalog: Record<string, Copy>): Record<string, Copy> {
  const sourceLocales = Object.keys(catalog).sort();
  const officialLocales = Object.keys(ACCESSIBILITY_OFFICIAL_COPY).sort();
  if (JSON.stringify(sourceLocales) !== JSON.stringify(officialLocales)) {
    throw new Error("official Accessibility copy must cover exactly the source locales");
  }
  return Object.fromEntries(Object.entries(catalog).map(([locale, copy]) => {
    const officialBody = ACCESSIBILITY_OFFICIAL_COPY[locale];
    if (!officialBody?.trim()) throw new Error(`missing official Accessibility reason for ${locale}`);
    return [locale, {
      ...copy,
      officialBody,
      errorBody: copy.errorBody.replaceAll("incodex install", "incodex accessibility"),
    }];
  }));
}
