import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';

const [html, css, mark, nginx] = await Promise.all([
  readFile(new URL('./index.html', import.meta.url), 'utf8'),
  readFile(new URL('./styles.css', import.meta.url), 'utf8'),
  readFile(new URL('../web/public/wcash-mark.svg', import.meta.url), 'utf8'),
  readFile(
    new URL('../deploy/nginx/wcashexplorer-landing.conf', import.meta.url),
    'utf8',
  ),
]);

const externalLinks = [...html.matchAll(/href="(https:\/\/[^"#]+)"/g)].map(
  ([, href]) => href,
);

assert.deepEqual(externalLinks, [
  'https://wcashexplorer.com/',
  'https://testnet.wcashexplorer.com/',
]);
assert.match(html, /<h1 id="page-title">Wcash Explorer<\/h1>/);
assert.match(html, /aria-label="Available network explorers"/);
assert.match(html, /src="\/wcash-mark\.svg"/);
assert.match(html, /https:\/\/wcashexplorer\.com\/og\.png/);
assert.doesNotMatch(html, /mainnet\.wcashexplorer\.com/i);
assert.match(css, /@keyframes coin-turn/);
assert.match(css, /prefers-reduced-motion: reduce/);
assert.match(mark, /<svg\b/);
assert.match(mark, /#7CFF6B/);
assert.match(
  nginx,
  /server_name wcashexplorer\.com www\.wcashexplorer\.com;/,
);
assert.match(nginx, /root \/var\/www\/wcashexplorer-landing;/);
assert.match(
  nginx,
  /ssl_certificate \/etc\/letsencrypt\/live\/wcashexplorer\.com\/fullchain\.pem;/,
);
assert.match(nginx, /return 308 https:\/\/wcashexplorer\.com\$request_uri;/);
assert.match(nginx, /location \^~ \/\.well-known\/acme-challenge\//);

console.log('Apex landing-page contract verified.');
