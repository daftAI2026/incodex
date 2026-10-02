import { readFile, readdir } from 'node:fs/promises';
import assert from 'node:assert/strict';

const root = new URL('../', import.meta.url);
const parse = await readFile(new URL('../crates/incodex-cli/src/parse.rs', root), 'utf8');
const help = await readFile(new URL('../crates/incodex-cli/src/help.rs', root), 'utf8');
const commandParser = parse.split('pub fn as_str')[0];
const commands = [...commandParser.matchAll(/"([a-z-]+)" => Ok\(Self::(\w+)\)/g)]
  .filter(([, command]) => !['help', 'version'].includes(command));
assert(commands.length > 0, 'Native command inventory was not recognized');
for (const locale of ['en', 'zh']) {
  const index = await readFile(new URL(`src/content/docs/${locale}/commands/index.mdx`, root), 'utf8');
  for (const [, command, variant] of commands) {
    const page = await readFile(new URL(`src/content/docs/${locale}/commands/${command}.mdx`, root), 'utf8');
    assert(index.includes(`/commands/${command}/`), `${locale}: ${command} missing from index`);
    assert(page.includes(`incodex ${command}`) || page.includes('inc update'), `${locale}: ${command} missing usage`);
    const sections = [...help.matchAll(/CliCommand::(\w+) => ([\s\S]*?)(?=\n        CliCommand::|\n    }\n})/g)]
      .filter(([, name]) => name === variant).map(([, , body]) => body).join('\n');
    const flags = new Set(sections.match(/--[a-z]+(?:-[a-z]+)*/g) ?? []);
    for (const flag of flags) assert(page.includes(flag), `${locale}: ${command} omits native help flag ${flag}`);
  }
}
async function slugs(locale) {
  const base = new URL(`src/content/docs/${locale}/`, root);
  const entries = await readdir(base, { recursive: true });
  return entries.filter(x => x.endsWith('.mdx')).sort();
}
assert.deepEqual(await slugs('en'), await slugs('zh'), 'Language pages must have matching slugs');
console.log(`Manual coverage: ${commands.length} public commands, both languages and native help flags`);
