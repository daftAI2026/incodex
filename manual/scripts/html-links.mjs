import { parse } from 'parse5';

function visit(node, links, ids) {
  if (node.tagName) {
    const attributes = new Map(node.attrs.map(({ name, value }) => [name, value]));
    const rel = (attributes.get('rel') ?? '').toLowerCase().split(/\s+/);
    if (node.tagName === 'link' && rel.includes('canonical')) return;

    if (attributes.has('id')) ids.add(attributes.get('id'));
    for (const name of ['href', 'src', 'xlink:href']) {
      if (attributes.has(name)) links.push(attributes.get(name));
    }
  }

  for (const child of node.childNodes ?? []) visit(child, links, ids);
  if (node.tagName === 'template' && node.content) {
    for (const child of node.content.childNodes) visit(child, links, ids);
  }
}

export function collectHtmlLinks(source) {
  const document = parse(source);
  const links = [];
  const ids = new Set();
  visit(document, links, ids);
  return { links, ids };
}
