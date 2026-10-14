import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import {fileURLToPath} from 'node:url';
import { spawn } from 'node:child_process';
import { argumentsOf, dependencies, openComposition, seek } from './browser.mjs';
import { capture } from './capture.mjs';

export function pixelDiff(reference, native) {
  if (reference.width !== native.width || reference.height !== native.height) throw new Error('Frame dimension mismatch');
  let absolute = 0, squared = 0, changed = 0, max = 0;
  for (let i = 0; i < reference.data.length; i += 4) {
    let differs = false;
    for (let c = 0; c < 3; c++) { const d = Math.abs(reference.data[i+c] - native.data[i+c]); absolute += d; squared += d*d; max = Math.max(max, d); differs ||= d !== 0; }
    changed += Number(differs);
  }
  const channels = reference.width * reference.height * 3;
  return { mae: absolute / channels, rmse: Math.sqrt(squared / channels), max_channel_error: max, changed_pixels: changed, changed_pixel_fraction: changed / (channels/3) };
}
export async function command(exe, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(exe, args, { windowsHide: true, ...options }); let stdout = '', stderr = '';
    child.stdout.on('data', data => stdout += data); child.stderr.on('data', data => stderr += data);
    child.on('error', reject); child.on('close', status => status === 0 ? resolve({ stdout, stderr }) : reject(new Error(`${exe} exited ${status}: ${stderr.slice(-5000)}${stdout.slice(-1000)}`)));
  });
}

async function browserPeak(pid){
  if(process.platform!=='win32')return null;
  const script=`$all=Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,PeakWorkingSetSize; $ids=[Collections.Generic.HashSet[uint32]]::new(); [void]$ids.Add(${Number(pid)}); do { $added=$false; foreach($p in $all){if($ids.Contains($p.ParentProcessId) -and $ids.Add($p.ProcessId)){$added=$true}} }while($added); [int64](($all | Where-Object {$ids.Contains($_.ProcessId)} | Measure-Object -Property PeakWorkingSetSize -Sum).Sum * 1024)`;
  const result=await command('powershell.exe',['-NoProfile','-Command',script]);const n=Number(result.stdout.trim());return Number.isFinite(n)&&n>0?n:null;
}

async function main() {
  const options = argumentsOf();
  if (options.help) { console.log('parity.mjs --input HTML --output NEW_DIRECTORY --director EXE [--capture] [--modules NODE_MODULES] [--chrome EXE] [--fps 30] [--duration SECONDS] [--root ASSET_ROOT] [--max-mae 2] [--min-ssim .99] [--frames COUNT]'); return; }
  if (!options.input || !options.output || !options.director) throw new Error('--input, --output and --director are required');
  const output = path.resolve(options.output); await fs.mkdir(output); await fs.mkdir(path.join(output, 'reference'));
  let nativeInput = path.resolve(options.input); let captureMs = null;
  if (options.capture) { const start = performance.now(); const data = await capture(options); captureMs = performance.now() - start; nativeInput = path.join(output, 'scene.json'); await fs.writeFile(nativeInput, JSON.stringify(data)); }
  const nativeArgs = ['html','benchmark',nativeInput,'--frames-dir',path.join(output,'native'),'--export',path.join(output,'native.mp4')];
  if (options.frames) nativeArgs.push('--frames',String(options.frames));
  const nativeStart = performance.now(); const native = await command(options.director, nativeArgs); const nativeWallMs = performance.now()-nativeStart;
  const nativeMetrics = JSON.parse(native.stdout.trim().split('\n').at(-1));
  const startup = performance.now(); const session = await openComposition(options); const browserCompileMs = performance.now()-startup;
  const count = nativeMetrics.runs[0].frames;
  if (session.meta.width !== nativeMetrics.width || session.meta.height !== nativeMetrics.height || session.meta.fps !== nativeMetrics.fps) throw new Error('Native/browser canvas or FPS mismatch');
  const runs = [];let browserPeakRss=null;
  try {
    for (let run = 0; run < 2; run++) {
      const start = performance.now();
      for (let frame = 0; frame < count; frame++) {
        await seek(session.page, frame / session.meta.fps, session.meta.duration);
        const png = await session.page.screenshot({ type: 'png', animations: 'allow', clip: { x: 0, y: 0, width: session.meta.width, height: session.meta.height } });
        if (run === 0) await fs.writeFile(path.join(output, 'reference', `${String(frame).padStart(6,'0')}.png`), png);
      }
      const ms = performance.now()-start; runs.push({ kind:run===0?'first':'warm', frames:count,capture_ms:ms,fps:count*1000/ms });
      if (session.errors.length) throw new Error(session.errors.join('\n'));
    }
    browserPeakRss=await browserPeak(session.browserPid);
  } finally { await session.close(); }
  const exportStart = performance.now();
  await command('ffmpeg',['-hide_banner','-loglevel','error','-framerate',String(nativeMetrics.fps),'-i',path.join(output,'reference','%06d.png'),'-frames:v',String(count),'-c:v','libx264','-pix_fmt','yuv420p',path.join(output,'reference.mp4')]);
  const browserExportMs = runs[0].capture_ms+performance.now()-exportStart;
  const ssim = await command('ffmpeg',['-hide_banner','-framerate',String(nativeMetrics.fps),'-i',path.join(output,'reference','%06d.png'),'-framerate',String(nativeMetrics.fps),'-i',path.join(output,'native','%06d.png'),'-lavfi','ssim','-frames:v',String(count),'-f','null','-']);
  const score = Number(/All:([\d.]+)/.exec(ssim.stderr)?.[1]); if (!Number.isFinite(score)) throw new Error('FFmpeg did not report SSIM');
  const { PNG } = dependencies(options.modules)('pngjs'); const pixels = [];
  for (let frame = 0; frame < count; frame++) {
    const filename = `${String(frame).padStart(6,'0')}.png`;
    const a = PNG.sync.read(await fs.readFile(path.join(output,'reference',filename))); const b = PNG.sync.read(await fs.readFile(path.join(output,'native',filename)));
    pixels.push({time:frame/nativeMetrics.fps,...pixelDiff(a,b)});
  }
  const mae = pixels.reduce((sum,f)=>sum+f.mae,0)/pixels.length;
  const passed = pixels.every(f=>f.mae <= Number(options['max-mae'] || 2)) && score >= Number(options['min-ssim'] || .99);
  const report = { input:path.resolve(options.input),machine:{platform:process.platform,arch:process.arch,cpu:os.cpus()[0]?.model,node:process.version},native:nativeMetrics,native_process_wall_ms:nativeWallMs,extraction_ms:captureMs,browser:{compile_ms:browserCompileMs,runs,export_ms:browserExportMs,peak_process_tree_rss_bound_bytes:browserPeakRss},parity:{ssim:score,mean_mae:mae,pixels,passed},notes:['First/warm describe in-process cache state, not an OS page-cache purge.','Native FPS times paint/layout; browser FPS includes seek and PNG screenshot encoding. Compare export totals for an end-to-end comparison.','Native RSS is its process high-water mark. Browser RSS sums live browser-process high-water marks, an upper bound rather than a simultaneous tree peak. FFmpeg is excluded from both.','Chromium reference uses the shared deterministic seeker and authored GSAP/CSS; it does not certify every upstream HyperFrames engine feature.']};
  await fs.writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify({ output,passed,ssim:score,mae,native_fps:nativeMetrics.runs.map(r=>r.fps),browser_capture_fps:runs.map(r=>r.fps),native_export_ms:nativeMetrics.export_ms,browser_export_ms:browserExportMs }));
  if (!passed) process.exitCode=1;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error=>{console.error(error.stack || error);process.exitCode=2;});
