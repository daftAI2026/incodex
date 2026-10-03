const repository = 'https://github.com/daftAI2026/incodex';
const latestReleaseApi = 'https://api.github.com/repos/daftAI2026/incodex/releases/latest';
const requiredAssets = ['incodex-darwin-arm64', 'incodex-darwin-x64', 'incodex-windows-x64.exe', 'SHA256SUMS'];

type Badge = Pick<HTMLAnchorElement, 'textContent' | 'href'>;
type FetchRelease = (url: string, init: RequestInit) => Promise<Pick<Response, 'ok' | 'json'>>;

function version(tag: unknown): number[] | null {
  if (typeof tag !== 'string' || !/^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag)) return null;
  const parts = tag.slice(1).split('.').map(Number);
  return parts.every(Number.isSafeInteger) ? parts : null;
}

export function selectStableRelease(data: unknown, baseline: string): { tag: string; url: string } | null {
  if (!data || typeof data !== 'object') return null;
  const release = data as Record<string, unknown>;
  const next = version(release.tag_name);
  const current = version(baseline);
  if (!next || !current || release.draft !== false || release.prerelease !== false ||
      typeof release.published_at !== 'string' || !Number.isFinite(Date.parse(release.published_at))) return null;
  for (let i = 0; i < 3; i++) {
    if (next[i]! < current[i]!) return null;
    if (next[i]! > current[i]!) break;
  }
  const url = `${repository}/releases/tag/${release.tag_name}`;
  if (release.html_url !== url || !Array.isArray(release.assets)) return null;
  const assets: unknown[] = release.assets;
  if (!requiredAssets.every(name => assets.some(asset => {
    if (!asset || typeof asset !== 'object') return false;
    const value = asset as Record<string, unknown>;
    return value.name === name && value.state === 'uploaded' && typeof value.size === 'number' && value.size > 0;
  }))) return null;
  return { tag: release.tag_name as string, url };
}

export async function updateReleaseBadge(link: Badge, baseline: string, fetchRelease: FetchRelease = fetch, isCurrent: () => boolean = () => true): Promise<boolean> {
  try {
    const response = await fetchRelease(latestReleaseApi, {
      credentials: 'omit', cache: 'default', referrerPolicy: 'no-referrer',
      headers: { Accept: 'application/vnd.github+json' }, signal: AbortSignal.timeout(5000),
    });
    if (!response.ok) return false;
    const release = selectStableRelease(await response.json(), baseline);
    if (!release || !isCurrent()) return false;
    if (release.tag !== baseline) link.href = release.url;
    link.textContent = release.tag;
    return true;
  } catch {
    return false;
  }
}

export function initReleaseBadge(link: HTMLAnchorElement, fetchRelease: FetchRelease = fetch): () => void {
  let active = true;
  const baseline = link.dataset.releaseBaseline;
  if (baseline) {
    void updateReleaseBadge(link, baseline, fetchRelease, () => active).then(updated => {
      if (updated && active && link.dataset.latestLabel) {
        const label = `${link.dataset.latestLabel}: ${link.textContent}`;
        link.setAttribute('aria-label', label);
        link.title = label;
      }
    });
  }
  return () => { active = false; };
}
