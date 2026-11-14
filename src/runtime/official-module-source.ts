/**
 * [INPUT]: 依赖 packaged module 的 fetch 字节流与调用方读取预算。
 * [OUTPUT]: 提供有界源码读取、大小校验及只合并在途请求的读取器。
 * [POS]: 官方模块适配的叶子 IO 边界；不缓存已完成源码，不选择组件或执行模块。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
export function assertOfficialModuleSourceSize(source: string, maxCharacters: number): void {
  if (source.length > maxCharacters) throw new Error("Unexpected official renderer module size");
}

export type OfficialModuleSourceReader = (url: string, maxCharacters?: number) => Promise<string>;

export function createOfficialModuleSourceReader(read: OfficialModuleSourceReader = readOfficialModuleSource): OfficialModuleSourceReader {
  // 只合并同一预算的在途读取；请求结束即释放，不保留源码缓存。
  const pending = new Map<string, Promise<string>>();
  return (url, maxCharacters) => {
    const key = JSON.stringify([url, maxCharacters ?? null]);
    const existing = pending.get(key);
    if (existing) return existing;
    const reading = read(url, maxCharacters);
    pending.set(key, reading);
    const release = () => { if (pending.get(key) === reading) pending.delete(key); };
    void reading.then(release, release);
    return reading;
  };
}

export async function readOfficialModuleSource(url: string, maxCharacters = 2_000_000): Promise<string> {
  const response = await fetch(url, { signal: AbortSignal.timeout(5000), redirect: "error" });
  if (!response.ok) throw new Error("Cannot read official renderer module");
  const declaredBytes = Number(response.headers.get("content-length"));
  if (Number.isFinite(declaredBytes) && declaredBytes > maxCharacters) {
    throw new Error("Unexpected official renderer module size");
  }
  if (!response.body) {
    const source = await response.text();
    assertOfficialModuleSourceSize(source, maxCharacters);
    return source;
  }
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const chunks: string[] = [];
  let totalBytes = 0;
  while (true) {
    const chunk = await reader.read();
    if (chunk.done) break;
    totalBytes += chunk.value.byteLength;
    if (totalBytes > maxCharacters) {
      await reader.cancel();
      throw new Error("Unexpected official renderer module size");
    }
    chunks.push(decoder.decode(chunk.value, { stream: true }));
  }
  chunks.push(decoder.decode());
  const source = chunks.join("");
  assertOfficialModuleSourceSize(source, maxCharacters);
  return source;
}
