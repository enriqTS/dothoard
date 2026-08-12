import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { visit } from 'unist-util-visit';

const base = process.env.BASE ?? '/dothoard';

// docs/*.md link to each other with GitHub-relative paths (e.g. `safety.md#topic`)
// so they render correctly when read directly on GitHub. Rewrite those to
// Starlight's clean routes (e.g. `/dothoard/safety/#topic`) at build time.
function remarkFixDocLinks() {
  return (tree) => {
    visit(tree, 'link', (node) => {
      const match = node.url.match(/^([\w-]+)\.md(#.*)?$/);
      if (!match) return;
      const [, slug, hash = ''] = match;
      node.url = `${base}/${slug}/${hash}`;
    });
  };
}

export default defineConfig({
  site: 'https://enriqts.github.io',
  base,
  markdown: {
    remarkPlugins: [remarkFixDocLinks],
  },
  integrations: [
    starlight({
      title: 'dothoard',
      description: 'Safe, Git-native dotfile backups for Linux.',
      sidebar: [
        { label: 'Start here', items: [{ label: 'Quick start', slug: 'quick-start' }, { label: 'TUI guide', slug: 'tui' }] },
        { label: 'Use dothoard', items: [{ label: 'Configuration', slug: 'configuration' }, { label: 'Ignore rules', slug: 'ignore-rules' }, { label: 'Multiple-machine namespaces', slug: 'namespaces' }] },
        { label: 'Safety and Git', items: [{ label: 'Safety model and limitations', slug: 'safety' }, { label: 'Git authentication', slug: 'authentication' }] },
        { label: 'Help', items: [{ label: 'Troubleshooting', slug: 'troubleshooting' }, { label: 'FAQ', slug: 'faq' }] },
        { label: 'Community', items: [{ label: 'Development', slug: 'development' }, { label: 'Contributing', slug: 'contributing' }, { label: 'Experimental releases', slug: 'releases' }] }
      ]
    })
  ]
});
