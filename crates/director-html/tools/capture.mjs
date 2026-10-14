import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { argumentsOf, openComposition, seek, extract, freezeResources } from './browser.mjs';

export async function capture(options) {
  const session = await openComposition(options);
  try {
    const frames = [];
    const count = Math.ceil(session.meta.duration * session.meta.fps);
    for (let frame = 0; frame < count; frame++) {
      const time = frame / session.meta.fps;
      await seek(session.page, time, session.meta.duration);
      frames.push({ time, root: await extract(session.page) });
      if (session.errors.length) throw new Error(session.errors.join('\n'));
    }
    const resources = await freezeResources(session.page, frames, session.resources);
    return { format: 'director-html-capture/1', ...session.meta, sample_rate: session.meta.fps, ...resources, frames };
  } finally { await session.close(); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = argumentsOf();
  if (options.help) console.log('capture.mjs --input HTML --output JSON [--modules NODE_MODULES] [--chrome EXE] [--fps 30] [--duration SECONDS] [--composition ID]');
  else {
    try {
      if (!options.input || !options.output) throw new Error('--input and --output are required');
      const start = performance.now(); const result = await capture(options);
      await fs.mkdir(path.dirname(path.resolve(options.output)), { recursive: true });
      await fs.writeFile(options.output, JSON.stringify(result));
      console.log(JSON.stringify({ frames: result.frames.length, capture_ms: performance.now() - start, output: options.output, screenshots_in_capture: 0 }));
    } catch (error) { console.error(error.stack || error); process.exitCode = 1; }
  }
}
