import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import {createHash} from 'node:crypto';
import {argumentsOf, dependencies} from './browser.mjs';
import {command, pixelDiff} from './parity.mjs';

async function probe(video) {
  const meta = JSON.parse((await command('ffprobe',['-v','error','-count_frames','-show_streams','-show_format','-of','json',video])).stdout);
  const stream = meta.streams.find(s=>s.codec_type==='video');
  if (!stream || Number(stream.nb_read_frames)<=0) throw new Error(`${video}: no video frames`);
  const [num,den]=stream.r_frame_rate.split('/').map(Number),fps=num/den;
  if(!Number.isFinite(fps)||fps<=0)throw new Error(`${video}: invalid frame rate`);
  const expected=Number(stream.nb_read_frames)/fps,tolerance=1/fps+.025;
  for(const s of meta.streams) if(['video','audio'].includes(s.codec_type) && (!Number.isFinite(Number(s.duration)) || Math.abs(Number(s.duration)-expected)>tolerance))throw new Error(`${video}: ${s.codec_type} duration ${s.duration} differs from ${expected}`);
  if(!Number.isFinite(Number(meta.format.duration))||Math.abs(Number(meta.format.duration)-expected)>tolerance)throw new Error(`${video}: MP4 duration differs from video duration`);
  return {width:stream.width,height:stream.height,fps,frames:Number(stream.nb_read_frames),duration:Number(meta.format.duration),codec:stream.tags?.encoder,size_bytes:Number(meta.format.size)};
}
async function ssim(args) {
  const result=await command('ffmpeg',['-hide_banner',...args,'-lavfi','ssim','-f','null','-']);
  const score=Number(/All:([\d.]+)/.exec(result.stderr)?.[1]);
  if(!Number.isFinite(score))throw new Error('FFmpeg did not report SSIM');
  return score;
}
async function main() {
  const o=argumentsOf();
  if(o.help){console.log('gpu-benchmark.mjs --input HTML_OR_CAPTURE --director VULKAN_EXE --output NEW_DIRECTORY [--modules NODE_MODULES] [--encoders software,nvenc] [--min-ssim .99] [--max-mae 2]');return;}
  for(const key of ['input','director','output'])if(!o[key])throw new Error(`--${key} is required`);
  const input=path.resolve(o.input),output=path.resolve(o.output);
  const encoders=String(o.encoders||'software,nvenc').split(',');
  if(!encoders.includes('software')||encoders.some(e=>!['software','nvenc'].includes(e)))throw new Error('--encoders must include software and may include nvenc');
  const {PNG}=dependencies(o.modules)('pngjs');
  await fs.mkdir(output);
  const cases=[];
  for(const gpu of [false,true])for(const encoder of encoders){
    const name=`${gpu?'gpu':'cpu'}-${encoder}`,args=['html','benchmark',input,'--encoder',encoder,'--export',path.join(output,`${name}.mp4`)];
    if(gpu)args.push('--gpu');
    if(encoder==='software')args.push('--frames-dir',path.join(output,name));
    const result=await command(o.director,args);
    await fs.writeFile(path.join(output,`${name}.log`),result.stdout+'\n'+result.stderr);
    const metrics=JSON.parse(result.stdout.trim().split('\n').at(-1));
    if(metrics.renderer!==(gpu?'vulkan':'cpu')||metrics.encoder!==(encoder==='nvenc'?'h264_nvenc':'libx264'))throw new Error(`${name}: requested backend/encoder was not used`);
    const media=await probe(path.join(output,`${name}.mp4`));
    const exports=[];
    for(const kind of ['first','warm']){
      const video=path.join(output,`${name}-${kind}.mp4`),renderArgs=['html','render',input,'--encoder',encoder,'-o',video];
      if(gpu)renderArgs.push('--gpu');
      const started=performance.now(),rendered=await command(o.director,renderArgs),wall=performance.now()-started;
      await fs.writeFile(path.join(output,`${name}-${kind}.log`),rendered.stdout+'\n'+rendered.stderr);
      exports.push({kind,process_wall_ms:wall,...await probe(video)});
    }
    cases.push({name,gpu,encoder,metrics,media,exports});
    console.log(JSON.stringify({case:name,fps:metrics.runs.map(r=>r.fps),export_ms:metrics.export_ms,process_export_ms:exports.map(e=>e.process_wall_ms)}));
  }
  const cpu=cases.find(c=>c.name==='cpu-software'),gpu=cases.find(c=>c.name==='gpu-software');
  for(const c of cases)for(const key of ['width','height','fps','frames'])if(c.media[key]!==cpu.media[key])throw new Error(`${c.name}: ${key} differs from CPU reference`);
  const pixels=[];
  for(let f=0;f<cpu.media.frames;f++){
    const name=`${String(f).padStart(6,'0')}.png`;
    const a=PNG.sync.read(await fs.readFile(path.join(output,'cpu-software',name))),b=PNG.sync.read(await fs.readFile(path.join(output,'gpu-software',name)));
    pixels.push({time:f/cpu.media.fps,...pixelDiff(a,b)});
  }
  const score=await ssim(['-framerate',String(cpu.media.fps),'-i',path.join(output,'cpu-software','%06d.png'),'-framerate',String(cpu.media.fps),'-i',path.join(output,'gpu-software','%06d.png')]);
  const encoded=[];
  for(const c of cases)if(c!==cpu)encoded.push({case:c.name,ssim:await ssim(['-i',path.join(output,'cpu-software.mp4'),'-i',path.join(output,`${c.name}.mp4`)])});
  const passed=score>=Number(o['min-ssim']||.99)&&pixels.every(f=>f.mae<=Number(o['max-mae']||2));
  const report={input,sha256:createHash('sha256').update(await fs.readFile(input)).digest('hex'),machine:{platform:process.platform,cpu:os.cpus()[0]?.model,node:process.version},cases,parity:{ssim:score,mean_mae:pixels.reduce((s,f)=>s+f.mae,0)/pixels.length,pixels,passed},encoded_comparisons:encoded,notes:['Render FPS includes native layout, paint, submission and synchronous RGBA readback in both modes. PNG encoding is outside that timer.','Fresh-process export timings include startup, compilation, GPU setup, rendering, encoding and muxing. First/warm describe cache state; OS and driver caches are not purged.','GPU surface creation fails on unavailable Vulkan; requested hardware modes never substitute CPU/software.','Process RSS excludes FFmpeg and GPU VRAM. Encoded SSIM includes codec-quality differences.','All MP4s are probed for frame count, FPS, dimensions and audio/video/container duration alignment.']};
  await fs.writeFile(path.join(output,'report.json'),JSON.stringify(report,null,2));
  console.log(JSON.stringify({output,passed,ssim:score,mean_mae:report.parity.mean_mae}));
  if(!passed)process.exitCode=1;
}
main().catch(e=>{console.error(e.stack||e);process.exitCode=2;});
