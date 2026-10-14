import fs from 'node:fs/promises';
import path from 'node:path';
import { argumentsOf } from './browser.mjs';
import { capture } from './capture.mjs';
import { command } from './parity.mjs';

const options = argumentsOf();
if (options.help) console.log('registry-audit.mjs --registry REGISTRY --output DIRECTORY --director EXE [--modules NODE_MODULES] [--chrome EXE] [--fps 2] [--names icon-camera,blur-in] [--portrait] [--resume]');
else {
  try {
    if (!options.registry || !options.output || !options.director) throw new Error('--registry, --output and --director are required');
    const directory = path.resolve(options.output); if(!options.resume)await fs.mkdir(directory);
    const registry = JSON.parse(await fs.readFile(path.join(options.registry, 'registry.json'), 'utf8'));
    const names = options.names?.split(',');
    const report = options.resume?JSON.parse(await fs.readFile(path.join(directory,'report.json'),'utf8')):{ registry:path.resolve(options.registry),sample_rate:Number(options.fps || 2),profile:options.portrait?'portrait':'original',items:[],claims:'Compilation/render smoke audit. Pixel parity requires a separate parity run.' };
    if(report.registry!==path.resolve(options.registry)||report.profile!==(options.portrait?'portrait':'original')||report.sample_rate!==Number(options.fps||2))throw new Error('Resume settings must match the recorded corpus');
    if(names)report.items=report.items.filter(item=>!names.includes(item.name));
    const done=new Set(report.items.map(i=>i.name));
    const queue = registry.items.filter(item => (!names || names.includes(item.name))&&!done.has(item.name));
    let saving=Promise.resolve();
    const save = () => {const data=JSON.stringify(report,null,2);saving=saving.then(async()=>{await fs.writeFile(path.join(directory,'report.tmp'),data);await fs.rename(path.join(directory,'report.tmp'),path.join(directory,'report.json'));});return saving;};
    // Two browser workers, no changes to the registry checkout.
    await Promise.all(Array.from({length:2}, async () => {
      for (;;) {
        const item = queue.shift(); if (!item) break;
        const started = performance.now();
        const input = path.resolve(options.registry, 'blocks', item.name, 'compositions', options.portrait ? 'portrait.html' : 'block.html');
        const file = path.join(directory, `${item.name}.json`);
        let stage = 'extraction';
        try {
          const scene = await capture({...options,input,fps:options.fps || 2});
          await fs.writeFile(file, JSON.stringify(scene));stage='native-compilation';
          const checked = await command(options.director,['html','check',file]);
          const metrics = JSON.parse(checked.stdout.trim().split('\n').at(-1));stage='native-render';
          const png = path.join(directory,`${item.name}.png`);
          await command(options.director,['html','still',file,'--time',String(scene.duration/2),'-o',png]);
          report.items.push({name:item.name,status:'native-rendered',width:scene.width,height:scene.height,duration:scene.duration,frames:scene.frames.length,compile_ms:metrics.compile_ms,nodes:metrics.nodes,elapsed_ms:performance.now()-started});
        } catch (error) { report.items.push({name:item.name,status:'incompatible',stage,diagnostic:String(error.message).slice(0,6000),elapsed_ms:performance.now()-started}); }
        await save();
        if (report.items.length%10===0) console.log(JSON.stringify({completed:report.items.length,remaining:queue.length,native_rendered:report.items.filter(i=>i.status==='native-rendered').length}));
      }
    }));
    report.items.sort((a,b)=>a.name.localeCompare(b.name));
    report.total=report.items.length;report.native_rendered=report.items.filter(i=>i.status==='native-rendered').length;report.incompatible=report.total-report.native_rendered;
    await save();console.log(JSON.stringify({output:directory,total:report.total,native_rendered:report.native_rendered,incompatible:report.incompatible}));
  } catch (error) { console.error(error.stack || error);process.exitCode=2; }
}
