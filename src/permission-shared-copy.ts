type Copy = Record<string, string>;

// Reuse the existing localized instructions, not installation-specific claims.
// In particular, an uninstalled user must never be told to reinstall just to
// check whether macOS has granted permission.
export function sharedPermissionCopy(catalog: Record<string, Copy>): Record<string, Copy> {
  return Object.fromEntries(Object.entries(catalog).map(([locale, copy]) => [locale, {
    ...copy,
    body: copy.addedTitle,
    errorBody: copy.errorBody.replaceAll("incodex install", "incodex doctor"),
  }]));
}
