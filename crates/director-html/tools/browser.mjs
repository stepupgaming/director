import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';

export function argumentsOf(argv = process.argv.slice(2)) {
  const result = {};
  for (let i = 0; i < argv.length; i++) {
    const key = argv[i];
    if (!key.startsWith('--')) throw new Error(`Unexpected argument ${key}`);
    result[key.slice(2)] = argv[i + 1]?.startsWith('--') || !argv[i + 1] ? true : argv[++i];
  }
  return result;
}
export function dependencies(modules) {
  return createRequire(modules ? pathToFileURL(path.resolve(modules, '../package.json')) : import.meta.url);
}

const GL_HOOK = `(() => {
  if (window.__directorGl) return;
  const canvases = new WeakMap();
  const locations = new WeakMap();
  const textures = new WeakMap();
  const contexts = new WeakMap();
  const hooked = new Set();
  function state(gl) {
    let current = contexts.get(gl);
    if (!current) { current = { unit: gl.TEXTURE0, units: [], flipY: false }; contexts.set(gl, current); }
    return current;
  }
  function entry(gl) {
    let current = canvases.get(gl.canvas);
    if (!current) { current = { fragment: '', uniforms: {}, images: {} }; canvases.set(gl.canvas, current); }
    return current;
  }
  function remember(gl, loc, values) {
    const meta = locations.get(loc);
    const record = meta && canvases.get(meta.canvas);
    if (record && values.every(Number.isFinite)) record.uniforms[meta.name] = values;
  }
  function bytesOf(pixels) {
    const bytes = pixels instanceof Uint8Array ? pixels : new Uint8Array(pixels.buffer, pixels.byteOffset || 0, pixels.byteLength || pixels.length);
    let binary = '';
    const chunk = 0x4000;
    for (let i = 0; i < bytes.length; i += chunk) binary += String.fromCharCode.apply(null, bytes.subarray(i, Math.min(i + chunk, bytes.length)));
    return btoa(binary);
  }
  function hook(proto) {
    if (!proto || hooked.has(proto)) return;
    hooked.add(proto);
    const sources = new WeakMap();
    const shaderSource = proto.shaderSource;
    const compileShader = proto.compileShader;
    const getUniformLocation = proto.getUniformLocation;
    const activeTexture = proto.activeTexture;
    const bindTexture = proto.bindTexture;
    const pixelStorei = proto.pixelStorei;
    const texImage2D = proto.texImage2D;
    const uniform1i = proto.uniform1i;
    proto.shaderSource = function (shader, source) { sources.set(shader, String(source)); return shaderSource.call(this, shader, source); };
    proto.compileShader = function (shader) {
      const result = compileShader.call(this, shader);
      try {
        if (this.getShaderParameter(shader, this.SHADER_TYPE) === this.FRAGMENT_SHADER) entry(this).fragment = sources.get(shader) || '';
      } catch {}
      return result;
    };
    proto.getUniformLocation = function (program, name) {
      const loc = getUniformLocation.call(this, program, name);
      if (loc) locations.set(loc, { canvas: this.canvas, name: String(name) });
      return loc;
    };
    proto.activeTexture = function (unit) { state(this).unit = unit; return activeTexture.call(this, unit); };
    proto.bindTexture = function (target, tex) {
      const current = state(this);
      if (tex) current.units[(current.unit ?? this.TEXTURE0) - this.TEXTURE0] = tex;
      return bindTexture.call(this, target, tex);
    };
    proto.pixelStorei = function (pname, value) {
      if (pname === this.UNPACK_FLIP_Y_WEBGL) state(this).flipY = !!value;
      return pixelStorei.call(this, pname, value);
    };
    proto.texImage2D = function (...args) {
      const result = texImage2D.apply(this, args);
      try {
        if (args[1] !== 0) return result;
        const current = state(this);
        const tex = current.units[(current.unit ?? this.TEXTURE0) - this.TEXTURE0];
        if (!tex) return result;
        let width = 0, height = 0, pixels = null;
        if (args.length === 6 && args[5] && args[5].width) {
          const src = args[5];
          width = src.naturalWidth || src.width; height = src.naturalHeight || src.height;
          if (width > 0 && height > 0 && width <= 4096 && height <= 4096) {
            const copy = document.createElement('canvas'); copy.width = width; copy.height = height;
            const ctx = copy.getContext('2d', { willReadFrequently: true });
            ctx.drawImage(src, 0, 0, width, height);
            pixels = ctx.getImageData(0, 0, width, height).data;
          }
        } else if (args.length >= 9 && args[8] && args[3] > 0 && args[4] > 0 && args[3] <= 4096 && args[4] <= 4096) {
          width = args[3]; height = args[4]; pixels = args[8];
        }
        if (pixels && pixels.length >= width * height * 4) textures.set(tex, { width, height, rgba: bytesOf(pixels), flip_y: !!current.flipY });
      } catch {}
      return result;
    };
    function numeric(args, start) {
      const data = args[start];
      if (data && typeof data.length === 'number' && typeof data !== 'number') return Array.from(data, Number);
      return args.slice(start).map(Number);
    }
    for (const [method, count] of [['uniform1f', 1], ['uniform2f', 2], ['uniform3f', 3], ['uniform4f', 4]]) {
      const original = proto[method];
      proto[method] = function (loc, ...rest) {
        const result = original.call(this, loc, ...rest);
        try { remember(this, loc, rest.slice(0, count).map(Number)); } catch {}
        return result;
      };
    }
    for (const method of ['uniform1fv', 'uniform2fv', 'uniform3fv', 'uniform4fv', 'uniformMatrix2fv', 'uniformMatrix3fv', 'uniformMatrix4fv']) {
      const original = proto[method];
      proto[method] = function (...args) {
        const result = original.apply(this, args);
        try { remember(this, args[0], method.startsWith('uniformMatrix') ? numeric(args, 2) : numeric(args, 1)); } catch {}
        return result;
      };
    }
    proto.uniform1i = function (loc, value) {
      const result = uniform1i.call(this, loc, value);
      try {
        const meta = locations.get(loc);
        const record = meta && canvases.get(meta.canvas);
        const image = textures.get(state(this).units[value]);
        if (record && meta && image) { record.images[meta.name] = image; delete record.uniforms[meta.name]; }
        else if (record && meta) record.uniforms[meta.name] = [Number(value)];
      } catch {}
      return result;
    };
  }
  hook(window.WebGL2RenderingContext && WebGL2RenderingContext.prototype);
  hook(window.WebGLRenderingContext && WebGLRenderingContext.prototype);
  window.__directorGl = { read(canvas) { const record = canvases.get(canvas); return record && record.fragment ? { fragment: record.fragment, uniforms: record.uniforms, images: record.images } : null; } };
})();`;

export async function openComposition(options) {
  const require = dependencies(options.modules);
  const { chromium } = require('playwright');
  const filename = path.resolve(options.input);
  const directory = path.resolve(options.root || path.dirname(filename));
  if (!filename.startsWith(directory + path.sep)) throw new Error('Input must be inside --root');
  const errors = [];
  const server = http.createServer(async (req, res) => {
    try {
      const pathname = decodeURIComponent(new URL(req.url, 'http://local').pathname);
      const file = path.resolve(directory, `.${pathname}`);
      if (file !== directory && !file.startsWith(directory + path.sep)) { res.writeHead(403).end(); return; }
      const content = await fs.readFile(file);
      const mime = { '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript', '.svg': 'image/svg+xml', '.json': 'application/json', '.ttf': 'font/ttf', '.woff2': 'font/woff2' }[path.extname(file)] || 'application/octet-stream';
      res.writeHead(200, { 'content-type': mime }); res.end(content);
    } catch { res.writeHead(404).end(); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  let browserServer;
  try {
    browserServer = await chromium.launchServer({ headless: true, args:['--enable-logging=stderr'], ...(options.chrome ? { executablePath: path.resolve(options.chrome) } : {}) });
    browser = await chromium.connect(browserServer.wsEndpoint());
    const page = await browser.newPage({ viewport: { width: Number(options.width || 1920), height: Number(options.height || 1080) }, deviceScaleFactor: 1 });
    const resources=new Map();
    page.on('response',response=>{if(response.ok()&&['image','font'].includes(response.request().resourceType()))resources.set(response.url(),response.body().catch(()=>null));});
    // Freeze the declared GSAP version for extraction and reference rendering.
    await page.route('https://cdn.jsdelivr.net/npm/gsap@*/dist/gsap.min.js', async route => {
      const version = /gsap@([^/]+)/.exec(route.request().url())?.[1];
      const installed = require('gsap/package.json').version;
      if (version !== installed && version !== installed.split('.')[0]) throw new Error(`GSAP ${version} requested, local ${installed} installed`);
      await route.fulfill({ contentType: 'text/javascript', body: await fs.readFile(require.resolve('gsap/dist/gsap.min.js')) });
    });
    page.on('pageerror', error => errors.push(String(error)));
    await page.addInitScript({ content: GL_HOOK });
    await page.goto(`http://127.0.0.1:${server.address().port}/${path.relative(directory, filename).split(path.sep).map(encodeURIComponent).join('/')}`, { waitUntil: 'networkidle' });
    // Use the browser's HTML parser to assemble template transport before scripts execute.
    const assembled = await page.evaluate(async () => {
      let changed = false;
      const doc = document.cloneNode(true);
      for (const template of [...doc.querySelectorAll('template')]) { template.replaceWith(template.content); changed = true; }
      for (const host of [...doc.querySelectorAll('[data-composition-src]')]) {
        const source = new URL(host.getAttribute('data-composition-src'), document.baseURI);
        const response = await fetch(source); if (!response.ok) throw new Error(`Subcomposition ${source}: HTTP ${response.status}`);
        const sub = new DOMParser().parseFromString(await response.text(), 'text/html');
        if (sub.querySelector('[data-composition-src]')) throw new Error('Nested external subcompositions must be assembled by HyperFrames before capture');
        const template = sub.querySelector('template');
        const payload = template ? template.content : sub.body;
        host.append(...[...payload.childNodes].map(n => doc.importNode(n, true)));
        if (!template) for (const style of sub.head.querySelectorAll('style,link,script')) doc.head.append(doc.importNode(style, true));
        changed = true;
      }
      const ids=new Set();for(const el of doc.querySelectorAll('[id]')){if(ids.has(el.id))throw new Error(`Duplicate id ${el.id}; assemble repeated subcompositions with upstream HyperFrames ID scoping`);ids.add(el.id);}
      return changed ? `<!doctype html>${doc.documentElement.outerHTML}` : null;
    });
    if (assembled) { errors.length = 0; await page.setContent(assembled, { waitUntil: 'networkidle' }); }
    const meta = await page.evaluate(({ composition, duration, fps }) => {
      const roots = [...document.querySelectorAll('[data-composition-id]')];
      const root = composition ? roots.find(el => el.dataset.compositionId === composition) : roots[0] || document.querySelector('[data-hypercn-item]') || document.body;
      if (!root) throw new Error(`Composition ${composition} not found`);
      root.setAttribute('data-director-root', '');
      const width = Number(root.dataset.width || root.__hypercn?.cfg.width || root.offsetWidth);
      const height = Number(root.dataset.height || root.__hypercn?.cfg.height || root.offsetHeight);
      const length = Number(duration || root.dataset.duration || (root.__hypercn?.cfg.durationInFrames / root.__hypercn?.cfg.fps) || Math.max(0, ...Object.values(window.__timelines || {}).map(tl => tl.duration())));
      if (!Number.isFinite(length) || length <= 0) throw new Error('Finite duration is required for capture');
      if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0 || width > 16384 || height > 16384) throw new Error('Invalid composition dimensions');
      root.style.width = `${width}px`; root.style.height = `${height}px`;
      document.documentElement.style.width = document.body.style.width = `${width}px`;
      document.documentElement.style.height = document.body.style.height = `${height}px`;
      return { width, height, duration: length, fps: Number(fps || root.dataset.fps || root.__hypercn?.cfg.fps || 30) };
    }, { composition: options.composition, duration: options.duration, fps: options.fps });
    if (!Number.isFinite(meta.fps) || meta.fps <= 0 || meta.fps > 1000) throw new Error('Invalid capture FPS');
    await page.setViewportSize({ width: meta.width, height: meta.height });
    await page.evaluate(() => document.fonts.ready);
    if (errors.length) throw new Error(errors.join('\n'));
    return { page, meta, errors, resources, browserPid:browserServer.process().pid, async close() { await browser.close(); await browserServer.close(); server.closeAllConnections();await new Promise(resolve => server.close(resolve)); } };
  } catch (error) { await browser?.close();await browserServer?.close();server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); throw error; }
}

export async function seek(page, time, duration) {
  await page.evaluate(({ time, duration }) => {
    window.__directorTime = time;
    window.__directorDuration = duration;
    const root = document.querySelector('[data-director-root]');
    const offsetFor=el=>{let offset=0;for(let parent=el;parent&&parent!==root;parent=parent.parentElement){if(parent.hasAttribute('data-composition-src'))offset+=Number(parent.dataset.start||0);}return offset;};
    const timelines = new Map(Object.entries(window.__timelines || {}).map(([id,tl])=>[tl,[...document.querySelectorAll('[data-composition-id]')].find(el=>el.dataset.compositionId===id)]));
    for (const el of document.querySelectorAll('[data-hypercn-item]')) if (el.__hypercn?.timeline&&!timelines.has(el.__hypercn.timeline)) timelines.set(el.__hypercn.timeline,el);
    for (const [tl,el] of timelines) {if(tl.parent&&tl.parent!==window.gsap?.globalTimeline)continue;const local=Math.max(0,time-offsetFor(el));tl.pause();tl.totalTime(local + 0.000001, true); tl.totalTime(local, false); }
    for (const anim of document.getAnimations()) { anim.pause(); anim.currentTime = Math.max(0,time-offsetFor(anim.effect?.target)) * 1000; }
    for (const el of root.querySelectorAll('[data-start], [data-hidden]')) {
      const start = Number(el.dataset.start || 0)+offsetFor(el.parentElement);
      const end = el.dataset.duration ? start + Number(el.dataset.duration) : Number(el.dataset.end || duration);
      if (!Number.isFinite(start) || !Number.isFinite(end)) throw new Error('Relative HyperFrames timing requires upstream assembly');
      const hidden = el.hasAttribute('data-hidden') && el.dataset.hidden !== 'false';
      el.style.visibility = !hidden && time >= start && time < end ? '' : 'hidden';
    }
    for (const el of document.querySelectorAll('[data-hypercn-item]')) if (el.__hypercn?.state.__hfmLastErr) throw new Error(el.__hypercn.state.__hfmLastErr);
  }, { time, duration });
}

export function capturedTransformStyle(value) {
  return value === 'preserve-3d' ? 'preserve-3d' : undefined;
}

export function capturedTextDecoration(value) {
  const trimmed = (value == null ? '' : String(value)).trim();
  if (!trimmed || trimmed === 'none') return undefined;
  if (trimmed.split(/\s+/).every(token => token === 'underline' || token === 'overline' || token === 'line-through')) return trimmed;
  return { unsupported: trimmed };
}

export function capturedMaskMode(mode, image) {
  if (mode === 'luminance') return 'luminance';
  if (image != null && image !== 'none' && mode !== 'alpha' && mode !== 'match-source' && mode !== 'luminance') return { unsupported: true };
  return undefined;
}

export async function extract(page) {
  return page.evaluate(() => {
    function firstCssListItem(value) {
      let depth = 0;
      let out = '';
      for (const ch of value) {
        if (ch === '(') depth++;
        else if (ch === ')' && depth > 0) depth--;
        else if (ch === ',' && depth === 0) return out.trim();
        out += ch;
      }
      return out.trim();
    }
    const root = document.querySelector('[data-director-root]');
    const time = Number(window.__directorTime || 0);
    for (const host of document.querySelectorAll('[data-paper-shader]')) {
      const mount = host.paperShaderMount;
      if (!mount || mount.hasBeenDisposed || typeof mount.setFrame !== 'function') continue;
      if (mount.rafId != null) { cancelAnimationFrame(mount.rafId); mount.rafId = null; }
      mount.currentSpeed = 0;
      const speed = Number(mount.speed || 0);
      if (speed !== 0) mount.setFrame(time * 1000 * speed);
      else if (typeof mount.render === 'function') mount.render(0);
    }
    const unsupported = [];
    const props = ['background-color', 'background-image', 'background-size', 'background-position-x', 'background-position-y', 'color', 'font-family', 'font-size', 'font-weight', 'font-style', 'font-variant-numeric', 'line-height', 'letter-spacing', 'text-align', 'text-indent', 'white-space', 'text-transform', 'overflow-wrap', 'word-break', 'text-overflow', '-webkit-text-stroke', '-webkit-text-stroke-width', '-webkit-text-stroke-color', 'background-clip', 'direction', 'opacity', 'transform', 'transform-origin', 'filter', 'backdrop-filter', 'mask-image', 'mix-blend-mode', 'overflow-x', 'overflow-y', 'box-shadow', 'text-shadow', 'clip-path', 'z-index', 'object-fit', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left'];
    for (const side of ['top','right','bottom','left']) for (const suffix of ['width','style','color']) props.push(`border-${side}-${suffix}`);
    for (const corner of ['top-left','top-right','bottom-right','bottom-left']) props.push(`border-${corner}-radius`);
    const nodes = [root, ...root.querySelectorAll('*')].filter(el => !el.closest('svg') || el === el.closest('svg'));
    const saved = new Map(); const styles = new Map();
    for (const el of nodes) {
      const css = getComputedStyle(el); const values = Object.fromEntries(props.map(p => [p, css.getPropertyValue(p)]));
      for (const name of ['background-size', 'background-position-x', 'background-position-y', 'text-shadow']) {
        if (values[name]) values[name] = firstCssListItem(values[name]);
      }
      if (css.display === 'none') values.visibility = 'hidden';
      if (css.visibility !== 'visible') values.visibility = 'hidden';
      const perspective = css.getPropertyValue('perspective');
      if (perspective && perspective !== 'none') {
        values.perspective = perspective;
        values['perspective-origin'] = css.getPropertyValue('perspective-origin');
      }
      if (css.getPropertyValue('backface-visibility') === 'hidden') values['backface-visibility'] = 'hidden';
      if (css.getPropertyValue('transform-style') === 'preserve-3d') values['transform-style'] = 'preserve-3d';
      const decoration = css.getPropertyValue('text-decoration-line').trim();
      if (decoration && decoration !== 'none') {
        if (decoration.split(/\s+/).every(token => token === 'underline' || token === 'overline' || token === 'line-through')) values['text-decoration-line'] = decoration;
        else unsupported.push(`${el.id || el.tagName}: text-decoration-line: ${decoration}`);
      }
      for (const property of ['background-blend-mode']) {
        const value = css.getPropertyValue(property);
        if (value && !value.split(',').every(v=>['none','normal'].includes(v.trim()))) unsupported.push(`${el.id || el.tagName}: ${property}: ${value}`);
      }
      const maskImage = css.maskImage;
      const maskMode = css.maskMode;
      if (maskMode === 'luminance') values['mask-mode'] = 'luminance';
      else if (maskImage != null && maskImage !== 'none' && maskMode !== 'alpha' && maskMode !== 'match-source' && maskMode !== 'luminance') unsupported.push(`${el.id || el.tagName}: non-alpha mask mode`);
      for (const pseudo of ['::before', '::after']) { const content = getComputedStyle(el, pseudo).content; if (content && !['none','normal','""'].includes(content)) unsupported.push(`${el.id || el.tagName}${pseudo}: generated content`); }
      if (css.backgroundImage.startsWith('url(')) unsupported.push(`${el.id || el.tagName}: image backgrounds need native image translation`);
      if (css.position === 'fixed' || css.position === 'sticky') values.position = css.position;
      if (el.tagName === 'CANVAS' && !window.__directorGl?.read(el)?.fragment) unsupported.push(`${el.id || 'canvas'}: canvas/WebGL requires a native translator`);
      if (el.tagName === 'IFRAME') unsupported.push(`${el.id || 'iframe'}: captured iframe requires native media translation; use the static compiler for media`);
      if (el.tagName === 'VIDEO') {
        if (!el.muted) unsupported.push(`${el.id || 'video'}: video audio needs an explicit audio element; author muted video`);
        if (!(el.currentSrc || el.src)) unsupported.push(`${el.id || 'video'}: video without src`);
        el.__directorMedia = mediaOf(el);
      }
      if (el.tagName === 'AUDIO') {
        const authored = el.dataset.playbackRate;
        const rate = authored == null || authored === '' ? 1 : Number(authored);
        if (!Number.isFinite(rate) || rate !== 1 || el.playbackRate !== 1) unsupported.push(`${el.id || 'audio'}: audio playback-rate translation unsupported`);
        if (!(el.currentSrc || el.src)) unsupported.push(`${el.id || 'audio'}: audio without src`);
        el.__directorMedia = mediaOf(el);
      }
      styles.set(el, values);
      saved.set(el, [el.style.getPropertyValue('transform'), el.style.getPropertyPriority('transform')]);
      el.style.setProperty('transform', 'none', 'important');
    }
    const mediaIds = new Set();
    for (const el of root.querySelectorAll('video, audio')) {
      if (!el.id) continue;
      if (mediaIds.has(el.id)) unsupported.push(`duplicate captured media id '${el.id}'`);
      mediaIds.add(el.id);
    }
    const rects = new Map(nodes.map(el => [el, el.getBoundingClientRect()]));
    function svgMarkup(el) {
      const colorCanvas=document.createElement('canvas');colorCanvas.width=colorCanvas.height=1;const colorContext=colorCanvas.getContext('2d',{willReadFrequently:true});
      const resolvedColor=(value,current)=>{if(value==='none'||value.startsWith('url('))return value;if(value.toLowerCase()==='currentcolor')value=current;colorContext.clearRect(0,0,1,1);colorContext.fillStyle=value;colorContext.fillRect(0,0,1,1);const [r,g,b,a]=colorContext.getImageData(0,0,1,1).data;return `rgba(${r},${g},${b},${a/255})`;};
      const clone = el.cloneNode(true);
      const original = [el, ...el.querySelectorAll('*')]; const copies = [clone, ...clone.querySelectorAll('*')];
      for (let i = 0; i < original.length; i++) {
        const css = getComputedStyle(original[i]);
        for (const p of ['fill','fill-opacity','stroke','stroke-width','stroke-opacity','stroke-linecap','stroke-linejoin','stroke-dasharray','stroke-dashoffset','opacity','color']) {const value=css.getPropertyValue(p);copies[i].setAttribute(p,['fill','stroke','color'].includes(p)?resolvedColor(value,css.color):value);copies[i].style.removeProperty(p);}
        if (i > 0 && css.filter !== 'none') {
          copies[i].removeAttribute('filter');
          copies[i].style.filter = css.filter;
        }
        if (i > 0 && css.mixBlendMode !== 'normal') copies[i].style.mixBlendMode = css.mixBlendMode;
        if (i > 0 && css.transform !== 'none') {
          const matrix = new DOMMatrix(css.transform);
          if (!matrix.is2D) unsupported.push(`${original[i].id || 'svg child'}: 3D transform inside SVG`);
          else {
            const origin = css.transformOrigin.split(' ').map(parseFloat);
            let [x, y] = origin;
            if (css.transformBox === 'fill-box') { const bbox = original[i].getBBox(); x += bbox.x; y += bbox.y; }
            const transform = new DOMMatrix().translate(x, y).multiply(matrix).translate(-x, -y);
            copies[i].setAttribute('transform', `matrix(${transform.a} ${transform.b} ${transform.c} ${transform.d} ${transform.e} ${transform.f})`);
            copies[i].style.transform = ''; copies[i].style.transformOrigin = '';
          }
        }
      }
      clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
      clone.style.transform = ''; clone.removeAttribute('transform'); clone.style.opacity = '1'; clone.removeAttribute('opacity');clone.style.filter='';
      return new XMLSerializer().serializeToString(clone);
    }
    function offsetFor(el) {
      let offset = 0;
      for (let parent = el; parent && parent !== root; parent = parent.parentElement) {
        if (parent.hasAttribute('data-composition-src')) offset += Number(parent.dataset.start || 0);
      }
      return offset;
    }
    function mediaOf(el) {
      const duration = Number(window.__directorDuration || root.dataset.duration || 0);
      const parentOffset = offsetFor(el.parentElement);
      const start = Number(el.dataset.start || 0) + parentOffset;
      // data-end is composition-local, matching the static compiler.
      const end = el.dataset.duration ? start + Number(el.dataset.duration) : (el.dataset.end ? parentOffset + Number(el.dataset.end) : duration);
      if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start) unsupported.push(`${el.id || el.tagName.toLowerCase()}: invalid clip window [${start}, ${end})`);
      const mediaStart = Number(el.dataset.mediaStart || 0);
      const authoredRate = el.dataset.playbackRate;
      const playbackRate = authoredRate == null || authoredRate === '' ? 1 : Number(authoredRate);
      if (!Number.isFinite(mediaStart) || mediaStart < 0 || !Number.isFinite(playbackRate) || playbackRate <= 0) unsupported.push(`${el.id || el.tagName.toLowerCase()}: media start must be nonnegative and rate positive`);
      let hidden = false;
      for (let node = el; node; node = node.parentElement) if (node.hasAttribute('data-hidden') && node.dataset.hidden !== 'false') hidden = true;
      const authoredVolume = el.dataset.volume;
      const volume = authoredVolume == null || authoredVolume === '' ? 1 : Number(authoredVolume);
      if (el.tagName === 'AUDIO' && (!Number.isFinite(volume) || volume < 0)) unsupported.push(`${el.id || 'audio'}: audio volume must be finite and nonnegative`);
      return { muted: !!el.muted, looped: !!el.loop, hidden, volume: Number.isFinite(volume) ? volume : 0, media_start: mediaStart, playback_rate: playbackRate, start, end };
    }
    function walk(el, parentRect) {
      if (['STYLE','SCRIPT','LINK','META','TEMPLATE','SOURCE','TRACK'].includes(el.tagName)) return null;
      if (el.tagName === 'AUDIO') return null;
      const rect = rects.get(el); const values = styles.get(el);
      if (!rect || !values || values.visibility === 'hidden') return null;
      const item = { tag: el.tagName.toLowerCase(), id: el.id || el.tagName.toLowerCase(), rect: [rect.x - parentRect.x, rect.y - parentRect.y, rect.width, rect.height], styles: values, children: [] };
      if (el.tagName.toLowerCase() === 'svg') { item.svg = svgMarkup(el); return item; }
      if (el.tagName === 'CANVAS') { const recorded = window.__directorGl?.read(el); if (recorded?.fragment) item.shader = recorded; }
      if (el.tagName === 'IMG') { item.src = el.currentSrc || el.src; return item; }
      if (el.tagName === 'VIDEO') {
        if (!el.id) item.id = `video@${[...root.querySelectorAll('video')].indexOf(el)}`;
        item.src = el.currentSrc || el.src;
        item.media = el.__directorMedia;
        return item;
      }
      const elementChildren = [...el.children].filter(c => !['STYLE','SCRIPT','LINK'].includes(c.tagName));
      if (!elementChildren.length) { const text = el.textContent; if (text.trim()) item.text = values['white-space'].startsWith('pre') ? text : text.replace(/\s+/g, ' ').trim(); }
      else {
        for (const node of el.childNodes) {
          if (node.nodeType === Node.ELEMENT_NODE) { const child = walk(node, rect); if (child) item.children.push(child); }
          else if (node.nodeType === Node.TEXT_NODE && node.textContent.trim()) {
            const range = document.createRange(); range.selectNodeContents(node); const r = range.getBoundingClientRect();
            const textStyles = Object.fromEntries(['color','font-family','font-size','font-weight','font-style','font-variant-numeric','line-height','letter-spacing','text-align','text-indent','white-space','text-transform','overflow-wrap','word-break','text-overflow','-webkit-text-stroke','-webkit-text-stroke-width','-webkit-text-stroke-color','direction'].map(p => [p, values[p]]));
            item.children.push({ tag:'text',id:`${item.id}:text`,rect:[r.x-rect.x,r.y-rect.y,r.width,r.height],styles:textStyles,text:node.textContent.replace(/\s+/g,' '),children:[] });
          }
        }
      }
      return item;
    }
    try {
      const rect = rects.get(root); const result = walk(root, rect);
      if (result) {
        for (const [index, el] of [...root.querySelectorAll('audio')].entries()) {
          result.children.push({ tag: 'audio', id: el.id || `audio@${index}`, rect: [0, 0, 0, 0], styles: {}, children: [], src: el.currentSrc || el.src, media: el.__directorMedia });
        }
      }
      if (unsupported.length) throw new Error([...new Set(unsupported)].join('\n'));
      return result;
    } finally { for (const [el, [value, priority]] of saved) { if (value) el.style.setProperty('transform', value, priority); else el.style.removeProperty('transform'); } }
  });
}

export async function freezeResources(page, frames, resources=new Map()) {
  const fonts = await page.evaluate(() => {
    const fonts = [];
    for (const sheet of document.styleSheets) {
      let rules; try { rules = sheet.cssRules; } catch { throw new Error('Cross-origin stylesheet cannot be frozen; supply local styles'); }
      for (const rule of rules) if (rule.type === CSSRule.FONT_FACE_RULE) fonts.push([rule.style.getPropertyValue('font-family').replace(/^['"]|['"]$/g,''), rule.style.getPropertyValue('src')]);
    }
    return fonts;
  });
  const freeze = async source => {
    if(source.startsWith('data:'))return source;
    const url=await page.evaluate(source=>new URL(source,document.baseURI).href,source);
    let bytes=await resources.get(url);
    if(!bytes){const response=await page.request.get(url);if(!response.ok())throw new Error(`Asset ${url}: HTTP ${response.status()}`);bytes=await response.body();}
    return `data:application/octet-stream;base64,${bytes.toString('base64')}`;
  };
  const assets = {};
  async function collect(node) { if (!node) return; if (node.src && !assets[node.src]) assets[node.src] = await freeze(node.src); for (const child of node.children) await collect(child); }
  for (const frame of frames) await collect(frame.root);
  const frozenFonts = [];
  for (const [family, src] of fonts) {
    // CSSOM normalized URLs; use the browser parser to select the first source.
    const source = await page.evaluate(src => { const div = document.createElement('div'); div.style.backgroundImage = src.split(/\)\s*(?:format|tech)\(/)[0] + (src.includes('format(') ? ')' : ''); return div.style.backgroundImage; }, src);
    const match = /^url\(["']?(.*?)["']?\)$/.exec(source || src.split(' format(')[0]);
    if (!match) throw new Error(`Cannot freeze font source ${family}`);
    frozenFonts.push([family, `url("${await freeze(match[1])}")`]);
  }
  return { fonts: frozenFonts, assets };
}
