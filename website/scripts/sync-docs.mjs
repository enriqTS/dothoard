import { cp, mkdir, readdir, rm } from 'node:fs/promises';
import { join } from 'node:path';

const source = new URL('../../docs/', import.meta.url);
const destination = new URL('../src/content/docs/', import.meta.url);

await mkdir(destination, { recursive: true });
for (const entry of await readdir(destination)) {
  if (entry !== 'index.md') await rm(join(destination.pathname, entry), { recursive: true });
}

for (const entry of await readdir(source)) {
  if (entry.endsWith('.md') && entry !== 'README.md') {
    await cp(join(source.pathname, entry), join(destination.pathname, entry));
  }
}
