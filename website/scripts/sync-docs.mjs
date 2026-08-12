import { mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

const source = new URL('../../docs/', import.meta.url);
const destination = new URL('../src/content/docs/', import.meta.url);

await mkdir(destination, { recursive: true });
for (const entry of await readdir(destination)) {
  if (entry !== 'index.md') await rm(join(destination.pathname, entry), { recursive: true });
}

for (const entry of await readdir(source)) {
  if (!entry.endsWith('.md') || entry === 'README.md') continue;

  const content = await readFile(join(source.pathname, entry), 'utf8');
  const title = content.match(/^#\s+(.+)$/m)?.[1];
  if (!title) throw new Error(`${entry} must begin with a level-one heading`);

  await writeFile(
    join(destination.pathname, entry),
    `---\ntitle: ${JSON.stringify(title)}\n---\n\n${content}`,
  );
}
