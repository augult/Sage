// Packs PNGs into an Apple .icns container. Usage: node make-icns.mjs out.icns ic07=128.png ic08=256.png ...
import { readFileSync, writeFileSync } from 'node:fs';

const [out, ...pairs] = process.argv.slice(2);
const chunks = pairs.map((pair) => {
  const [type, file] = pair.split('=');
  const png = readFileSync(file);
  const head = Buffer.alloc(8);
  head.write(type, 0, 4, 'ascii');
  head.writeUInt32BE(png.length + 8, 4);
  return Buffer.concat([head, png]);
});
const body = Buffer.concat(chunks);
const header = Buffer.alloc(8);
header.write('icns', 0, 4, 'ascii');
header.writeUInt32BE(body.length + 8, 4);
writeFileSync(out, Buffer.concat([header, body]));
console.log(`${out}: ${body.length + 8} bytes, ${chunks.length} images`);
