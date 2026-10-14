import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import {createHash} from 'node:crypto';
import {argumentsOf} from './browser.mjs';
import {command} from './parity.mjs';

async function main() {
  const o = argumentsOf();
  if (o.help) {
    console.log('hyperframes-benchmark.mjs --project DIRECTORY --output NEW_DIRECTORY --director EXE --hyperframes-cli MODULE [--chrome EXE] [--workers 1|auto]');
    return;
  }
  for (const key of ['project', 'output', 'director', 'hyperframes-cli']) if (!o[key]) throw new Error(`--${key} is required`);
  const project = path.resolve(o.project), output = path.resolve(o.output);
  await fs.mkdir(output);
  const input = path.join(project, 'index.html'), source = await fs.readFile(input);
  const env = {...process.env, DO_NOT_TRACK:'1'};
  if (o.chrome) env.HYPERFRAMES_BROWSER_PATH = path.resolve(o.chrome);
  const version = await command(process.execPath, [o['hyperframes-cli'], '--version'], {env});
  const native = [], hyperframes = [];
  for (let run = 0; run < 2; run++) {
    const kind = run === 0 ? 'first' : 'warm';
    for (const engine of ['native', 'hyperframes']) {
      const video = path.join(output, `${engine}-${kind}.mp4`);
      const exe = engine === 'native' ? path.resolve(o.director) : process.execPath;
      const args = engine === 'native' ? ['html', 'render', input, '-o', video] : [path.resolve(o['hyperframes-cli']), 'render', project, '--workers', String(o.workers || '1'), '--output', video, '--quality', 'looks'];
      const start = performance.now(), result = await command(exe, args, {env}), wallMs = performance.now() - start;
      await fs.writeFile(path.join(output, `${engine}-${kind}.log`), result.stdout + '\n' + result.stderr);
      const probe = JSON.parse((await command('ffprobe', ['-v','error','-count_frames','-show_streams','-show_format','-of','json',video])).stdout);
      const stream = probe.streams.find(s => s.codec_type === 'video');
      if (!stream || Number(stream.nb_read_frames) <= 0) throw new Error(`${video} contains no video frames`);
      (engine === 'native' ? native : hyperframes).push({kind,wall_ms:wallMs,video,width:stream.width,height:stream.height,fps:stream.r_frame_rate,frames:Number(stream.nb_read_frames),duration:Number(probe.format.duration),size_bytes:Number(probe.format.size)});
      console.log(`${engine} ${kind}: ${wallMs.toFixed(1)} ms, ${stream.nb_read_frames} frames`);
    }
  }
  for (let i=0;i<2;i++) for (const key of ['width','height','fps','frames']) if(native[i][key] !== hyperframes[i][key]) throw new Error(`Engine ${key} mismatch`);
  const comparison = await command('ffmpeg', ['-hide_banner','-i',hyperframes[0].video,'-i',native[0].video,'-lavfi','ssim','-f','null','-']);
  const ssim = Number(/All:([\d.]+)/.exec(comparison.stderr)?.[1]);
  if (!Number.isFinite(ssim)) throw new Error('No encoded-video SSIM result');
  const report = {input,sha256:createHash('sha256').update(source).digest('hex'),machine:{platform:process.platform,cpu:os.cpus()[0]?.model,node:process.version},hyperframes_version:version.stdout.trim(),workers:String(o.workers || '1'),native,hyperframes,encoded_ssim:ssim,notes:['Wall times include process startup, compilation, rendering and encoding in fresh processes. First/warm describe OS/application cache state; the OS cache is not purged.','HyperFrames uses its actual installed CLI renderer. Stage timings and capture/GPU modes are preserved in logs.','Both renderers use their default software H.264 encoder settings. The encoded SSIM also includes their encoder differences; parity.mjs compares unencoded PNGs separately.']};
  await fs.writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify({output,encoded_ssim:ssim,native_ms:native.map(r=>r.wall_ms),hyperframes_ms:hyperframes.map(r=>r.wall_ms)}));
}
main().catch(e=>{console.error(e.stack || e);process.exitCode=2;});
