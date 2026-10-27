// Browser environment for page scripts at compile time. Nodes live in the
// Rust document; this file wraps node indexes in DOM-shaped objects, keeps
// inline styles, and records what scripts write while the timeline plays.
(() => {
"use strict";
const G = globalThis;
const N = {
  kind: __dKind, tag: __dTag, ns: __dNs, parent: __dParent, children: __dChildren,
  connected: __dConnected, attr: __dAttr, attrs: __dAttrs, setAttr: __dSetAttr,
  text: __dText, setText: __dSetText, create: __dCreate, createText: __dCreateText,
  insert: __dInsert, remove: __dRemove, clone: __dClone, setInner: __dSetInner,
  markup: __dMarkup, select: __dSelect, matches: __dMatches, layout: __dLayout,
  viewport: __dViewport, computed: __dComputed, matrix: __dMatrix,
  generation: __dGeneration, media: __dMedia, path: __dPath,
};
for (const name of ["__dKind", "__dTag", "__dNs", "__dParent", "__dChildren", "__dConnected", "__dAttr",
  "__dAttrs", "__dSetAttr", "__dText", "__dSetText", "__dCreate", "__dCreateText", "__dInsert", "__dRemove",
  "__dClone", "__dSetInner", "__dMarkup", "__dSelect", "__dMatches", "__dLayout", "__dViewport",
  "__dComputed", "__dMatrix", "__dGeneration", "__dMedia", "__dPath"]) delete G[name];

const HTML_NS = "http://www.w3.org/1999/xhtml";
const SVG_NS = "http://www.w3.org/2000/svg";
const [VW, VH] = N.viewport();
const fail = (message) => { throw new Error(message); };

// Setup runs the page scripts; play seeks the timeline frame by frame.
const S = {
  phase: "setup",
  wrappers: [],
  styles: new Map(),
  dirtyStyles: new Set(),
  layoutDirty: false,
  overlay: new Map(),
  touched: new Set(),
  last: new Map(),
  generation: 0,
  timeouts: [],
  timerSeq: 1,
  frames: [],
};

// ---- CSS text helpers ----
const kebab = (name) => {
  if (name.startsWith("--")) return name;
  if (name === "cssFloat") return "float";
  return name.replace(/^(webkit|moz|ms|o)(?=[A-Z])/, "-$1").replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
};
const camel = (name) => name.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
function splitTop(text, sep) {
  const out = [];
  let depth = 0, quote = "", start = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) { if (c === quote && text[i - 1] !== "\\") quote = ""; continue; }
    if (c === '"' || c === "'") quote = c;
    else if (c === "(") depth++;
    else if (c === ")") depth--;
    else if (c === sep && depth === 0) { out.push(text.slice(start, i)); start = i + 1; }
  }
  out.push(text.slice(start));
  return out;
}
function parseDecls(text) {
  const map = new Map();
  for (const part of splitTop(text || "", ";")) {
    const at = part.indexOf(":");
    if (at < 0) continue;
    let name = part.slice(0, at).trim();
    let value = part.slice(at + 1).trim().replace(/\s*!important$/i, "");
    if (!name) continue;
    if (!name.startsWith("--")) name = name.toLowerCase();
    if (value !== "") map.set(name, value);
  }
  return map;
}
const serialize = (map) => [...map].map(([k, v]) => `${k}: ${v}`).join("; ");
const LAYOUT = /^(width|height|min-|max-|margin|padding|border(?!-radius|-color|-style)|display|position|top|left|right|bottom|font|line-height|letter-spacing|word-spacing|white-space|flex|grid|gap|row-gap|column-gap|align|justify|order|box-sizing|aspect-ratio|inset|overflow|text-indent|writing-mode)/;

// ---- inline style ----
function styleMap(i) {
  let map = S.styles.get(i);
  if (!map) {
    map = parseDecls(N.attr(i, "style"));
    S.styles.set(i, map);
  }
  return map;
}
function styleWrite(i, name, value) {
  const map = styleMap(i);
  const v = value == null ? "" : String(value).trim().replace(/\s*!important$/i, "");
  if (v === "") map.delete(name); else map.set(name, v);
  noteStyle(i, name);
}
function noteStyle(i, name) {
  if (S.phase === "play") { S.touched.add(i); return; }
  S.dirtyStyles.add(i);
  if (name === null || LAYOUT.test(name)) S.layoutDirty = true;
}
function flushStyles() {
  for (const i of S.dirtyStyles) {
    const text = serialize(styleMap(i));
    const old = N.attr(i, "style");
    if ((old || "") !== text) N.setAttr(i, "style", text === "" ? null : text);
  }
  S.dirtyStyles.clear();
  S.layoutDirty = false;
}
function layoutReady() {
  if (S.phase === "setup" && S.layoutDirty) flushStyles();
}
const DECL_METHODS = {
  setProperty(i, name, value) { styleWrite(i, kebab(String(name)), value); },
  getPropertyValue(i, name) { return styleMap(i).get(kebab(String(name))) || ""; },
  getPropertyPriority() { return ""; },
  removeProperty(i, name) {
    const key = kebab(String(name));
    const old = styleMap(i).get(key) || "";
    styleWrite(i, key, "");
    return old;
  },
  item(i, n) { return [...styleMap(i).keys()][n] || ""; },
};
function makeStyle(i) {
  return new Proxy({}, {
    get(_, key) {
      if (typeof key !== "string") return undefined;
      if (key in DECL_METHODS) return (...args) => DECL_METHODS[key](i, ...args);
      if (key === "cssText") return serialize(styleMap(i));
      if (key === "length") return styleMap(i).size;
      if (/^\d+$/.test(key)) return [...styleMap(i).keys()][+key] || "";
      return styleMap(i).get(kebab(key)) || "";
    },
    set(_, key, value) {
      if (typeof key !== "string") return true;
      if (key === "cssText") {
        S.styles.set(i, parseDecls(String(value)));
        noteStyle(i, null);
        return true;
      }
      styleWrite(i, kebab(key), value);
      return true;
    },
    // GSAP tweens a key the style lacks as a property of the element itself.
    // Every property has its unprefixed name, so GSAP's prefix probes miss.
    has(_, key) {
      return typeof key === "string" && !DOM_ONLY.has(key) && !/^(Webkit|Moz|ms|Ms|O)[A-Z]/.test(key)
        && !Object.prototype.hasOwnProperty.call(wrap(i), key);
    },
    deleteProperty(_, key) { if (typeof key === "string") styleWrite(i, kebab(key), ""); return true; },
  });
}
const DOM_ONLY = new Set([
  "textContent", "innerHTML", "innerText", "outerHTML", "nodeValue", "data", "value",
  "scrollTop", "scrollLeft", "className", "id", "src", "href", "title", "currentTime", "volume",
]);

// ---- computed style ----
const PLAIN = /^-?[\d.]+(e-?\d+)?(px|deg|rad|turn|s|ms)?$|^[a-z-]+$|^#[0-9a-f]+$|^(rgb|rgba|hsl|hsla)\(/i;
function computedValue(el, name) {
  const i = el.__i;
  if (el.nodeType !== 1) return "";
  const inline = S.styles.get(i)?.get(name);
  if (inline !== undefined && inline !== "") {
    if (name === "transform") return N.matrix(i, inline);
    if (PLAIN.test(inline) && !/^(inherit|initial|unset|revert|auto|currentcolor)$/i.test(inline)) return inline;
  }
  // SVG content has no layout box: its inline style, then its presentation attribute.
  if (el.namespaceURI === SVG_NS && el.ownerSVGElement) {
    if (inline !== undefined && inline !== "") return inline;
    const attr = el.getAttribute(name);
    if (attr !== null) return attr;
  }
  layoutReady();
  return N.computed(i, name);
}
function computedStyle(el) {
  if (!el || el.__i === undefined) fail("getComputedStyle: argument is not an Element");
  return new Proxy({}, {
    get(_, key) {
      if (typeof key !== "string") return undefined;
      if (key === "getPropertyValue") return (name) => computedValue(el, kebab(String(name)));
      if (key === "getPropertyPriority") return () => "";
      if (key === "length") return 0;
      if (key === "cssText") return "";
      return computedValue(el, kebab(key));
    },
    has(_, key) { return typeof key === "string"; },
  });
}

// ---- layout ----
function box(el) {
  layoutReady();
  return N.layout(el.__i);
}
function unitPx(text, el, axis) {
  const m = /^(-?[\d.]+)(px|em|rem|%|vw|vh)?$/.exec(String(text).trim());
  if (!m) return null;
  const n = parseFloat(m[1]);
  switch (m[2] || "px") {
    case "px": return n;
    case "em": return n * fontSize(el);
    case "rem": return n * fontSize(document.documentElement);
    case "vw": return n * VW / 100;
    case "vh": return n * VH / 100;
    case "%": {
      const parent = el.parentElement;
      const b = parent && box(parent);
      return b ? n * b[axis + 2] / 100 : null;
    }
  }
  return null;
}
function fontSize(el) {
  const inline = el && S.styles.get(el.__i)?.get("font-size");
  if (inline) {
    const v = unitPx(inline, el.parentElement || el, 0);
    if (v != null && !/em$/.test(inline)) return v;
  }
  const b = el && box(el);
  if (b) return b[6];
  return el && el.parentElement ? fontSize(el.parentElement) : 16;
}
// An absolute box with inline size, no padding and no border measures
// itself; GSAP's unit conversion probes this way.
function selfSize(el, axis) {
  const map = S.styles.get(el.__i);
  if (!map || map.get("position") !== "absolute") return null;
  const size = map.get(axis ? "height" : "width");
  if (!size) return null;
  const pad = map.get("padding"), border = map.get("border-width");
  if ((pad && parseFloat(pad)) || (border && parseFloat(border))) return null;
  return unitPx(size, el, axis);
}
function offsetSize(el, axis) {
  const own = selfSize(el, axis);
  if (own != null) return own;
  if (el === document.body || el === document.documentElement) return axis ? VH : VW;
  const b = box(el);
  return b ? b[axis + 2] : 0;
}
function rectOf(el) {
  if (el === document.body || el === document.documentElement) return domRect(0, 0, VW, VH);
  const b = box(el);
  if (b) return domRect(b[4], b[5], b[2], b[3]);
  if (el.namespaceURI === SVG_NS && el.ownerSVGElement) {
    const svg = rectOf(el.ownerSVGElement);
    const bb = el.getBBox();
    return domRect(svg.x + bb.x, svg.y + bb.y, bb.width, bb.height);
  }
  return domRect(0, 0, 0, 0);
}
function domRect(x, y, width, height) {
  return { x, y, width, height, left: x, top: y, right: x + width, bottom: y + height, toJSON() { return this; } };
}

// ---- SVG geometry ----
function svgMatrix(text) {
  let m = [1, 0, 0, 1, 0, 0];
  const mul = (a, b) => [a[0] * b[0] + a[2] * b[1], a[1] * b[0] + a[3] * b[1], a[0] * b[2] + a[2] * b[3],
    a[1] * b[2] + a[3] * b[3], a[0] * b[4] + a[2] * b[5] + a[4], a[1] * b[4] + a[3] * b[5] + a[5]];
  const re = /(matrix|translate|scale|rotate|skewX|skewY)\s*\(([^)]*)\)/g;
  let f;
  while ((f = re.exec(text || ""))) {
    const v = f[2].split(/[\s,]+/).filter(Boolean).map(Number);
    const rad = (d) => d * Math.PI / 180;
    let t;
    switch (f[1]) {
      case "matrix": t = v.slice(0, 6); break;
      case "translate": t = [1, 0, 0, 1, v[0] || 0, v[1] || 0]; break;
      case "scale": t = [v[0], 0, 0, v[1] ?? v[0], 0, 0]; break;
      case "rotate": {
        const c = Math.cos(rad(v[0])), s = Math.sin(rad(v[0]));
        t = [c, s, -s, c, 0, 0];
        if (v.length > 2) t = mul(mul([1, 0, 0, 1, v[1], v[2]], t), [1, 0, 0, 1, -v[1], -v[2]]);
        break;
      }
      case "skewX": t = [1, 0, Math.tan(rad(v[0])), 1, 0, 0]; break;
      case "skewY": t = [1, Math.tan(rad(v[0])), 0, 1, 0, 0]; break;
    }
    m = mul(m, t);
  }
  return { a: m[0], b: m[1], c: m[2], d: m[3], e: m[4], f: m[5] };
}
function pathPoints(d) {
  const tokens = String(d || "").match(/[a-zA-Z]|-?(?:\d+\.?\d*|\.\d+)(?:e-?\d+)?/g) || [];
  const pts = [];
  let x = 0, y = 0, sx = 0, sy = 0, cmd = "", i = 0;
  const num = () => parseFloat(tokens[i++]);
  const counts = { m: 2, l: 2, h: 1, v: 1, c: 6, s: 4, q: 4, t: 2, a: 7, z: 0 };
  while (i < tokens.length) {
    if (/[a-zA-Z]/.test(tokens[i])) cmd = tokens[i++];
    const lower = cmd.toLowerCase(), rel = cmd !== cmd.toUpperCase();
    if (lower === "z") { x = sx; y = sy; pts.push([x, y]); continue; }
    const n = counts[lower];
    if (n === undefined || i + n > tokens.length) break;
    const v = Array.from({ length: n }, num);
    const ox = rel ? x : 0, oy = rel ? y : 0;
    switch (lower) {
      case "h": x = ox + v[0]; break;
      case "v": y = oy + v[0]; break;
      case "a": x = ox + v[5]; y = oy + v[6]; break;
      default:
        for (let k = 0; k + 1 < n; k += 2) pts.push([ox + v[k], oy + v[k + 1]]);
        x = ox + v[n - 2]; y = oy + v[n - 1];
    }
    pts.push([x, y]);
    if (lower === "m") { sx = x; sy = y; cmd = rel ? "l" : "L"; }
  }
  return pts;
}
// A basic shape as path data, starting and turning where browsers do.
function shapePath(el) {
  const a = (n) => parseFloat(el.getAttribute(n)) || 0;
  switch (el.localName) {
    case "path": return el.getAttribute("d") || "";
    case "line": return `M${a("x1")},${a("y1")}L${a("x2")},${a("y2")}`;
    case "polyline": case "polygon": {
      const v = (el.getAttribute("points") || "").split(/[\s,]+/).filter(Boolean).map(Number);
      let d = "";
      for (let k = 0; k + 1 < v.length; k += 2) d += (k ? "L" : "M") + v[k] + "," + v[k + 1];
      return el.localName === "polygon" && d ? d + "Z" : d;
    }
    case "circle": case "ellipse": {
      const cx = a("cx"), cy = a("cy");
      const rx = el.localName === "circle" ? a("r") : a("rx"), ry = el.localName === "circle" ? a("r") : a("ry");
      return `M${cx + rx},${cy}A${rx},${ry} 0 0 1 ${cx},${cy + ry}A${rx},${ry} 0 0 1 ${cx - rx},${cy}` +
        `A${rx},${ry} 0 0 1 ${cx},${cy - ry}A${rx},${ry} 0 0 1 ${cx + rx},${cy}Z`;
    }
    case "rect": {
      const x = a("x"), y = a("y"), w = a("width"), h = a("height");
      let rx = el.hasAttribute("rx") ? a("rx") : a("ry"), ry = el.hasAttribute("ry") ? a("ry") : rx;
      rx = Math.min(rx, w / 2); ry = Math.min(ry, h / 2);
      if (!rx || !ry) return `M${x},${y}H${x + w}V${y + h}H${x}Z`;
      return `M${x + rx},${y}H${x + w - rx}A${rx},${ry} 0 0 1 ${x + w},${y + ry}V${y + h - ry}` +
        `A${rx},${ry} 0 0 1 ${x + w - rx},${y + h}H${x + rx}A${rx},${ry} 0 0 1 ${x},${y + h - ry}` +
        `V${y + ry}A${rx},${ry} 0 0 1 ${x + rx},${y}Z`;
    }
  }
  fail(`SVG <${el.localName}> has no path length`);
}
function bboxOf(el) {
  const a = (n) => parseFloat(el.getAttribute(n)) || 0;
  let pts = [];
  switch (el.localName) {
    case "rect": case "image": case "use": case "foreignObject": case "svg":
      pts = [[a("x"), a("y")], [a("x") + a("width"), a("y") + a("height")]]; break;
    case "circle": pts = [[a("cx") - a("r"), a("cy") - a("r")], [a("cx") + a("r"), a("cy") + a("r")]]; break;
    case "ellipse": pts = [[a("cx") - a("rx"), a("cy") - a("ry")], [a("cx") + a("rx"), a("cy") + a("ry")]]; break;
    case "line": pts = [[a("x1"), a("y1")], [a("x2"), a("y2")]]; break;
    case "polyline": case "polygon": {
      const v = (el.getAttribute("points") || "").split(/[\s,]+/).filter(Boolean).map(Number);
      for (let k = 0; k + 1 < v.length; k += 2) pts.push([v[k], v[k + 1]]);
      break;
    }
    case "path": pts = pathPoints(el.getAttribute("d")); break;
    case "g": case "a": case "switch":
      for (const child of el.children) {
        if (child.namespaceURI !== SVG_NS) continue;
        const b = bboxOf(child);
        if (b.width || b.height || b.x || b.y) {
          const m = svgMatrix(child.getAttribute("transform"));
          for (const [px, py] of [[b.x, b.y], [b.x + b.width, b.y], [b.x, b.y + b.height], [b.x + b.width, b.y + b.height]])
            pts.push([m.a * px + m.c * py + m.e, m.b * px + m.d * py + m.f]);
        }
      }
      break;
  }
  if (!pts.length) return { x: 0, y: 0, width: 0, height: 0 };
  const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
  const x = Math.min(...xs), y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}

// ---- events ----
class Event {
  constructor(type, init = {}) {
    this.type = type; this.bubbles = !!init.bubbles; this.cancelable = !!init.cancelable;
    this.detail = init.detail; this.defaultPrevented = false; this.target = null; this.currentTarget = null;
  }
  preventDefault() { this.defaultPrevented = true; }
  stopPropagation() {}
  stopImmediatePropagation() {}
}
class CustomEvent extends Event {}
class EventTarget {
  addEventListener(type, fn, options) {
    if (!fn) return;
    const map = this.__listeners || (Object.defineProperty(this, "__listeners", { value: new Map() }), this.__listeners);
    const list = map.get(type) || [];
    if (!list.some((l) => l.fn === fn)) list.push({ fn, once: !!(options && options.once) });
    map.set(type, list);
  }
  removeEventListener(type, fn) {
    const list = this.__listeners && this.__listeners.get(type);
    if (list) this.__listeners.set(type, list.filter((l) => l.fn !== fn));
  }
  dispatchEvent(event) {
    event.target = event.target || this;
    event.currentTarget = this;
    const list = (this.__listeners && this.__listeners.get(event.type)) || [];
    for (const l of [...list]) {
      if (l.once) this.removeEventListener(event.type, l.fn);
      typeof l.fn === "function" ? l.fn.call(this, event) : l.fn.handleEvent(event);
    }
    const handler = this["on" + event.type];
    if (typeof handler === "function") handler.call(this, event);
    return !event.defaultPrevented;
  }
}

// ---- nodes ----
function wrap(i) {
  if (i == null || i < 0) return null;
  let w = S.wrappers[i];
  if (w) return w;
  switch (N.kind(i)) {
    case 1: {
      const ns = N.ns(i);
      w = ns === SVG_NS ? new SVGElement(i) : new HTMLElement(i);
      break;
    }
    case 3: w = new Text(i); break;
    case 8: w = new Comment(i); break;
    case 9: w = document; break;
    case 11: w = new DocumentFragment(i); break;
    default: w = new Node(i);
  }
  S.wrappers[i] = w;
  return w;
}
const list = (indexes) => indexes.map(wrap);
function nodeArg(node, what) {
  if (typeof node === "string") return document.createTextNode(node);
  if (!node || node.__i === undefined) fail(`${what}: argument is not a Node`);
  return node;
}
function assertSetup(what) {
  if (S.phase === "play") fail(`scripts changed the document structure (${what}) while the timeline played; build elements during setup`);
}
function textOverlay(i) {
  return S.phase === "play" ? S.overlay.get(i)?.get("#text") : undefined;
}

class Node extends EventTarget {
  constructor(i) { super(); Object.defineProperty(this, "__i", { value: i }); }
  get nodeType() { return N.kind(this.__i); }
  get nodeName() { return "#node"; }
  get ownerDocument() { return document; }
  get parentNode() { return wrap(N.parent(this.__i)); }
  get parentElement() { const p = this.parentNode; return p && p.nodeType === 1 ? p : null; }
  get childNodes() { return list(N.children(this.__i)); }
  get firstChild() { return this.childNodes[0] || null; }
  get lastChild() { const c = this.childNodes; return c[c.length - 1] || null; }
  get nextSibling() { return sibling(this, 1, false); }
  get previousSibling() { return sibling(this, -1, false); }
  get isConnected() { return N.connected(this.__i); }
  get textContent() { const o = textOverlay(this.__i); return o !== undefined ? o : N.text(this.__i); }
  set textContent(value) {
    const v = value == null ? "" : String(value);
    if (S.phase === "play") { overlaySet(this.__i, "#text", v); return; }
    N.setText(this.__i, v);
    S.layoutDirty = true;
  }
  get nodeValue() { return this.nodeType === 1 ? null : this.textContent; }
  set nodeValue(v) { if (this.nodeType !== 1) this.textContent = v; }
  hasChildNodes() { return N.children(this.__i).length > 0; }
  contains(other) {
    for (let n = other; n; n = n.parentNode) if (n === this) return true;
    return false;
  }
  getRootNode() { let n = this; while (n.parentNode) n = n.parentNode; return n; }
  appendChild(child) { return this.insertBefore(child, null); }
  insertBefore(child, ref) {
    child = nodeArg(child, "insertBefore");
    if (child.nodeType === 11) {
      for (const c of child.childNodes) this.insertBefore(c, ref);
      return child;
    }
    if (S.phase === "play" && child.isConnected) assertSetup("insertBefore");
    N.insert(this.__i, child.__i, ref ? ref.__i : -1);
    S.layoutDirty = S.layoutDirty || S.phase === "setup";
    return child;
  }
  removeChild(child) {
    if (!child || child.parentNode !== this) fail("NotFoundError: the node to remove is not a child of this node");
    N.remove(child.__i);
    S.layoutDirty = S.layoutDirty || S.phase === "setup";
    return child;
  }
  replaceChild(next, old) { this.insertBefore(next, old); this.removeChild(old); return old; }
  remove() { const p = this.parentNode; if (p) p.removeChild(this); }
  cloneNode(deep) {
    flushOwn(this);
    return wrap(N.clone(this.__i, !!deep));
  }
  before(...nodes) { const p = this.parentNode; for (const n of nodes) p.insertBefore(nodeArg(n, "before"), this); }
  after(...nodes) {
    const p = this.parentNode, next = this.nextSibling;
    for (const n of nodes) p.insertBefore(nodeArg(n, "after"), next);
  }
  replaceWith(...nodes) { this.before(...nodes); this.remove(); }
  compareDocumentPosition(other) {
    if (other === this) return 0;
    if (this.contains(other)) return 20;
    if (other.contains(this)) return 10;
    const all = document.querySelectorAll("*");
    return all.indexOf(other) > all.indexOf(this) ? 4 : 2;
  }
}
function sibling(node, step, elements) {
  const p = node.parentNode;
  if (!p) return null;
  const kids = elements ? p.children : p.childNodes;
  const at = kids.indexOf(node);
  if (at < 0) return null;
  return kids[at + step] || null;
}
function flushOwn(node) {
  if (node.nodeType !== 1) return;
  for (const i of [node.__i, ...N.select(node.__i, "*", false)]) {
    if (S.dirtyStyles.has(i)) {
      const text = serialize(styleMap(i));
      N.setAttr(i, "style", text === "" ? null : text);
      S.dirtyStyles.delete(i);
    }
  }
}
function overlaySet(i, key, value) {
  let map = S.overlay.get(i);
  if (!map) S.overlay.set(i, (map = new Map()));
  map.set(key, value);
  S.touched.add(i);
}

class CharacterData extends Node {
  get data() { return this.textContent; }
  set data(v) { this.textContent = v; }
  get length() { return this.textContent.length; }
  get nextElementSibling() { return nextElement(this, 1); }
  get previousElementSibling() { return nextElement(this, -1); }
}
function nextElement(node, step) {
  for (let n = step > 0 ? node.nextSibling : node.previousSibling; n; n = step > 0 ? n.nextSibling : n.previousSibling)
    if (n.nodeType === 1) return n;
  return null;
}
class Text extends CharacterData {
  get nodeName() { return "#text"; }
  get wholeText() { return this.textContent; }
  splitText(offset) {
    const text = this.textContent;
    const rest = document.createTextNode(text.slice(offset));
    this.textContent = text.slice(0, offset);
    this.after(rest);
    return rest;
  }
}
class Comment extends CharacterData { get nodeName() { return "#comment"; } }

class ParentNode extends Node {
  get children() { return this.childNodes.filter((n) => n.nodeType === 1); }
  get childElementCount() { return this.children.length; }
  get firstElementChild() { return this.children[0] || null; }
  get lastElementChild() { const c = this.children; return c[c.length - 1] || null; }
  querySelector(selector) { return wrap(N.select(this.__i, String(selector), true)[0]); }
  querySelectorAll(selector) { return list(N.select(this.__i, String(selector), false)); }
  getElementsByTagName(tag) { return this.querySelectorAll(tag === "*" ? "*" : tag); }
  getElementsByClassName(names) {
    return this.querySelectorAll(String(names).trim().split(/\s+/).map((n) => "." + cssEscape(n)).join(""));
  }
  append(...nodes) { for (const n of nodes) this.appendChild(nodeArg(n, "append")); }
  prepend(...nodes) { const first = this.firstChild; for (const n of nodes) this.insertBefore(nodeArg(n, "prepend"), first); }
  replaceChildren(...nodes) { for (const c of this.childNodes) this.removeChild(c); this.append(...nodes); }
}
const cssEscape = (s) => String(s).replace(/([^\w-])/g, "\\$1");

class DocumentFragment extends ParentNode {
  get nodeName() { return "#document-fragment"; }
  getElementById(id) { return this.querySelector("#" + cssEscape(id)); }
}

class Element extends ParentNode {
  constructor(i) {
    super(i);
    Object.defineProperty(this, "style", { value: makeStyle(i), enumerable: true });
  }
  get localName() { return N.tag(this.__i); }
  get namespaceURI() { return N.ns(this.__i); }
  get tagName() { const t = this.localName; return this.namespaceURI === HTML_NS ? t.toUpperCase() : t; }
  get nodeName() { return this.tagName; }
  get id() { return this.getAttribute("id") || ""; }
  set id(v) { this.setAttribute("id", v); }
  get className() { return this.getAttribute("class") || ""; }
  set className(v) { this.setAttribute("class", v); }
  get classList() { return classList(this); }
  get dataset() { return dataset(this); }
  get attributes() {
    const flat = N.attrs(this.__i), out = [];
    for (let k = 0; k < flat.length; k += 2) out.push({ name: flat[k], value: this.getAttribute(flat[k]), localName: flat[k] });
    return out;
  }
  getAttributeNames() { return this.attributes.map((a) => a.name); }
  getAttribute(name) {
    name = attrKey(this, name);
    if (name === "style" && S.styles.has(this.__i)) {
      const text = serialize(S.styles.get(this.__i));
      return text === "" && N.attr(this.__i, "style") === null ? null : text;
    }
    const o = S.phase === "play" ? S.overlay.get(this.__i)?.get("@" + name) : undefined;
    return o !== undefined ? o : N.attr(this.__i, name);
  }
  getAttributeNS(_, name) { return this.getAttribute(name); }
  hasAttribute(name) { return this.getAttribute(name) !== null; }
  hasAttributes() { return this.attributes.length > 0; }
  setAttribute(name, value) {
    name = attrKey(this, name);
    value = String(value);
    if (name === "style") {
      S.styles.set(this.__i, parseDecls(value));
      noteStyle(this.__i, null);
      return;
    }
    if (S.phase === "play") { overlaySet(this.__i, "@" + name, value); return; }
    N.setAttr(this.__i, name, value);
    S.layoutDirty = true;
  }
  setAttributeNS(_, name, value) { this.setAttribute(name, value); }
  removeAttribute(name) {
    name = attrKey(this, name);
    if (name === "style") { S.styles.set(this.__i, new Map()); noteStyle(this.__i, null); return; }
    if (S.phase === "play") { overlaySet(this.__i, "@" + name, null); return; }
    N.setAttr(this.__i, name, null);
    S.layoutDirty = true;
  }
  removeAttributeNS(_, name) { this.removeAttribute(name); }
  toggleAttribute(name, force) {
    const on = force === undefined ? !this.hasAttribute(name) : !!force;
    on ? this.setAttribute(name, "") : this.removeAttribute(name);
    return on;
  }
  matches(selector) { return N.matches(this.__i, String(selector)); }
  webkitMatchesSelector(selector) { return this.matches(selector); }
  closest(selector) {
    for (let n = this; n && n.nodeType === 1; n = n.parentNode) if (n.matches(selector)) return n;
    return null;
  }
  get nextElementSibling() { return sibling(this, 1, true); }
  get previousElementSibling() { return sibling(this, -1, true); }
  get innerHTML() { flushOwn(this); return N.markup(this.__i, false); }
  set innerHTML(markup) {
    assertSetup("innerHTML");
    for (const i of N.select(this.__i, "*", false)) S.styles.delete(i);
    N.setInner(this.__i, String(markup));
    S.layoutDirty = true;
  }
  get outerHTML() { flushOwn(this); return N.markup(this.__i, true); }
  get innerText() { return this.textContent; }
  set innerText(v) { this.textContent = v; }
  insertAdjacentHTML(position, markup) {
    const holder = document.createElement("div");
    holder.innerHTML = markup;
    this.insertAdjacentElementList(position, holder.childNodes);
  }
  insertAdjacentElement(position, el) { this.insertAdjacentElementList(position, [el]); return el; }
  insertAdjacentText(position, text) { this.insertAdjacentElementList(position, [document.createTextNode(text)]); }
  insertAdjacentElementList(position, nodes) {
    switch (String(position).toLowerCase()) {
      case "beforebegin": this.before(...nodes); break;
      case "afterbegin": this.prepend(...nodes); break;
      case "beforeend": this.append(...nodes); break;
      case "afterend": this.after(...nodes); break;
      default: fail(`insertAdjacent: unknown position '${position}'`);
    }
  }
  getBoundingClientRect() { return rectOf(this); }
  getClientRects() { return [rectOf(this)]; }
  get offsetParent() {
    if (this === document.body || this === document.documentElement || !this.isConnected) return null;
    if (S.styles.get(this.__i)?.get("display") === "none") return null;
    return box(this) || selfSize(this, 0) != null ? document.body : null;
  }
  get offsetWidth() { return offsetSize(this, 0); }
  get offsetHeight() { return offsetSize(this, 1); }
  get offsetLeft() { return rectOf(this).x; }
  get offsetTop() { return rectOf(this).y; }
  get clientWidth() { return offsetSize(this, 0); }
  get clientHeight() { return offsetSize(this, 1); }
  get scrollWidth() { return offsetSize(this, 0); }
  get scrollHeight() { return offsetSize(this, 1); }
  get scrollLeft() { return 0; }
  set scrollLeft(_) {}
  get scrollTop() { return 0; }
  set scrollTop(_) {}
  scrollTo() {}
  scrollIntoView() {}
  focus() {}
  blur() {}
  click() { this.dispatchEvent(new Event("click", { bubbles: true })); }
  getAnimations() { return []; }
  animate() { fail("the Web Animations API runs in wall-clock time; animate with GSAP or CSS"); }
  attachShadow() { fail("shadow DOM is not supported at compile time"); }
}
function attrKey(el, name) {
  name = String(name);
  return el.namespaceURI === HTML_NS ? name.toLowerCase() : name;
}
function classList(el) {
  const read = () => (el.getAttribute("class") || "").split(/\s+/).filter(Boolean);
  const write = (names) => el.setAttribute("class", names.join(" "));
  return {
    get length() { return read().length; },
    get value() { return el.getAttribute("class") || ""; },
    item(n) { return read()[n] ?? null; },
    contains(name) { return read().includes(String(name)); },
    add(...names) { const cur = read(); for (const n of names) if (!cur.includes(n)) cur.push(n); write(cur); },
    remove(...names) { write(read().filter((n) => !names.includes(n))); },
    toggle(name, force) {
      const has = read().includes(name);
      const on = force === undefined ? !has : !!force;
      on ? this.add(name) : this.remove(name);
      return on;
    },
    replace(a, b) { const cur = read(); const at = cur.indexOf(a); if (at < 0) return false; cur[at] = b; write(cur); return true; },
    forEach(fn) { read().forEach(fn); },
    [Symbol.iterator]() { return read()[Symbol.iterator](); },
    toString() { return this.value; },
  };
}
function dataset(el) {
  const key = (k) => "data-" + k.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase());
  return new Proxy({}, {
    get(_, k) { return typeof k === "string" ? (el.getAttribute(key(k)) ?? undefined) : undefined; },
    set(_, k, v) { el.setAttribute(key(k), v); return true; },
    has(_, k) { return typeof k === "string" && el.hasAttribute(key(k)); },
    deleteProperty(_, k) { el.removeAttribute(key(k)); return true; },
    ownKeys() {
      return el.getAttributeNames().filter((n) => n.startsWith("data-")).map((n) => camel(n.slice(5)));
    },
    getOwnPropertyDescriptor(_, k) {
      return el.hasAttribute(key(k)) ? { enumerable: true, configurable: true, value: el.getAttribute(key(k)) } : undefined;
    },
  });
}
class HTMLElement extends Element {}
class SVGElement extends Element {
  get ownerSVGElement() {
    for (let p = this.parentNode; p && p.nodeType === 1; p = p.parentNode) if (p.localName === "svg") return p;
    return null;
  }
  get viewportElement() { return this.ownerSVGElement; }
  getBBox() { return bboxOf(this); }
  getCTM() { return { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }; }
  getScreenCTM() { return this.getCTM(); }
  get transform() {
    const el = this;
    return { baseVal: {
      get numberOfItems() { return el.getAttribute("transform") ? 1 : 0; },
      consolidate() { return el.getAttribute("transform") ? { matrix: svgMatrix(el.getAttribute("transform")) } : null; },
    } };
  }
  getTotalLength() { return N.path(shapePath(this)); }
  getPointAtLength(at) {
    const [x, y] = N.path(shapePath(this), Number(at) || 0);
    return { x, y, matrixTransform(m) { return { x: m.a * x + m.c * y + m.e, y: m.b * x + m.d * y + m.f }; } };
  }
}

class Document extends ParentNode {
  constructor() { super(0); }
  get nodeName() { return "#document"; }
  get documentElement() { return this.children[0] || null; }
  get head() { return this.querySelector("head"); }
  get body() { return this.querySelector("body"); }
  get defaultView() { return G; }
  get ownerDocument() { return null; }
  get compatMode() { return "CSS1Compat"; }
  get visibilityState() { return "visible"; }
  get hidden() { return false; }
  get activeElement() { return this.body; }
  createElement(tag) { return wrap(N.create(String(tag), "")); }
  createElementNS(ns, tag) { return wrap(N.create(String(tag).replace(/^\w+:/, ""), ns || "")); }
  createTextNode(text) { return wrap(N.createText(String(text), false)); }
  createComment(text) { return wrap(N.createText(String(text), true)); }
  createDocumentFragment() { return wrap(N.create("#fragment", "")); }
  createEvent() { return new Event(""); }
  getElementById(id) { return this.querySelector("#" + cssEscape(id)); }
  getElementsByName(name) { return this.querySelectorAll(`[name="${String(name).replace(/"/g, '\\"')}"]`); }
  elementFromPoint() { return null; }
  hasFocus() { return true; }
}
const document = new Document();
S.wrappers[0] = document;
let readyState = "loading";
Object.defineProperty(document, "readyState", { get: () => readyState });
document.fonts = {
  ready: Promise.resolve(),
  status: "loaded",
  load: () => Promise.resolve([]),
  check: () => true,
  add() {}, delete() {}, forEach() {},
  addEventListener() {}, removeEventListener() {},
};
document.fonts.ready = Promise.resolve(document.fonts);

// ---- window ----
let seed = 0x9e3779b9;
const random = () => {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
Math.random = random;
Date.now = () => 0;
const noop = () => {};
class Observer { constructor() {} observe() {} unobserve() {} disconnect() {} takeRecords() { return []; } }
const mediaList = (query) => ({
  media: String(query), matches: N.media(String(query)), onchange: null,
  addListener() {}, removeListener() {}, addEventListener() {}, removeEventListener() {}, dispatchEvent() { return true; },
});
function timer(fn, delay, args, repeat) {
  if (typeof fn !== "function") fail("timers with string code are not supported");
  const id = S.timerSeq++;
  if (!(delay > 0) && !repeat) S.timeouts.push({ id, fn, args });
  else S.timeouts.push({ id, delayed: true, delay: +delay || 0, repeat });
  return id;
}
const clearTimer = (id) => { S.timeouts = S.timeouts.filter((t) => t.id !== id); };
Object.assign(G, {
  window: G, self: G, top: G, parent: G, globalThis: G, document,
  Node, Element, HTMLElement, SVGElement, Text, Comment, CharacterData, Document, DocumentFragment,
  EventTarget, Event, CustomEvent,
  HTMLDivElement: HTMLElement, HTMLSpanElement: HTMLElement, HTMLImageElement: HTMLElement,
  HTMLVideoElement: HTMLElement, HTMLCanvasElement: HTMLElement, SVGSVGElement: SVGElement,
  SVGGraphicsElement: SVGElement, SVGPathElement: SVGElement,
  NodeList: Array, HTMLCollection: Array,
  innerWidth: VW, innerHeight: VH, outerWidth: VW, outerHeight: VH, devicePixelRatio: 1,
  scrollX: 0, scrollY: 0, pageXOffset: 0, pageYOffset: 0,
  screen: { width: VW, height: VH, availWidth: VW, availHeight: VH },
  location: { href: "about:blank", protocol: "about:", host: "", hostname: "", pathname: "blank", search: "", hash: "", origin: "null" },
  navigator: { userAgent: "Director", language: "en-US", languages: ["en-US"], platform: "", maxTouchPoints: 0, hardwareConcurrency: 1 },
  history: { pushState: noop, replaceState: noop, state: null },
  performance: { now: () => 0, mark: noop, measure: noop, timeOrigin: 0 },
  console: { log: noop, info: noop, warn: noop, error: noop, debug: noop, trace: noop, group: noop, groupEnd: noop, table: noop },
  getComputedStyle: computedStyle,
  matchMedia: mediaList,
  requestAnimationFrame: () => S.timerSeq++,
  cancelAnimationFrame: noop,
  requestIdleCallback: () => S.timerSeq++,
  cancelIdleCallback: noop,
  setTimeout: (fn, delay, ...args) => timer(fn, delay, args, false),
  setInterval: (fn, delay, ...args) => timer(fn, delay, args, true),
  clearTimeout: clearTimer,
  clearInterval: clearTimer,
  queueMicrotask: (fn) => Promise.resolve().then(fn),
  ResizeObserver: Observer, IntersectionObserver: Observer, MutationObserver: Observer,
  Image: function Image() { return document.createElement("img"); },
  scrollTo: noop, scrollBy: noop, focus: noop, blur: noop, alert: noop,
  addEventListener: EventTarget.prototype.addEventListener,
  removeEventListener: EventTarget.prototype.removeEventListener,
  dispatchEvent: EventTarget.prototype.dispatchEvent,
  fetch: () => fail("fetch is not available at compile time; inline the data"),
  XMLHttpRequest: function XMLHttpRequest() { fail("XMLHttpRequest is not available at compile time; inline the data"); },
  CSS: { supports: () => true, escape: cssEscape },
});

// ---- lifecycle, driven from Rust ----
function runTimeouts() {
  for (let guard = 0; guard < 10000; guard++) {
    const next = S.timeouts.find((t) => !t.delayed);
    if (!next) return;
    clearTimer(next.id);
    next.fn(...next.args);
  }
  fail("setTimeout callbacks keep scheduling more callbacks");
}
function composite(owner) {
  let offset = 0;
  const root = owner && owner.closest && owner.ownerDocument && document.querySelector("[data-composition-id]");
  for (let p = owner; p && p.nodeType === 1 && p !== root; p = p.parentElement) {
    if (p.hasAttribute("data-composition-src")) offset += Number(p.getAttribute("data-start") || 0);
  }
  return offset;
}
let drive = [];
G.__director = {
  loaded() {
    readyState = "interactive";
    document.dispatchEvent(new Event("readystatechange"));
    document.dispatchEvent(new Event("DOMContentLoaded"));
    readyState = "complete";
    document.dispatchEvent(new Event("readystatechange"));
    G.dispatchEvent(new Event("load"));
    runTimeouts();
  },
  settle() {
    runTimeouts();
    const delayed = S.timeouts.find((t) => t.delayed);
    if (delayed) fail(`a ${delayed.repeat ? "setInterval" : `setTimeout of ${delayed.delay} ms`} runs in wall-clock time; schedule it on the GSAP timeline`);
  },
  // Flush setup state into the document and start recording.
  play() {
    S.dirtyStyles = new Set(S.styles.keys());
    flushStyles();
    S.phase = "play";
    S.generation = N.generation();
    for (const [i, map] of S.styles) S.last.set(i, new Map(map));
    const gsap = G.gsap;
    const registry = G.__timelines || {};
    drive = [];
    for (const [id, tl] of Object.entries(registry)) {
      if (!tl || typeof tl.totalTime !== "function") continue;
      if (tl.parent && gsap && tl.parent !== gsap.globalTimeline) continue;
      const owner = [...document.querySelectorAll("[data-composition-id]")].find((el) => el.getAttribute("data-composition-id") === id);
      drive.push([tl, owner ? composite(owner) : 0]);
      tl.pause();
    }
    return drive.length;
  },
  // Seek every timeline to `t` and return what changed since the last frame
  // as index, key, value triples joined by \x01, with \x02 for a removal.
  frame(t) {
    const gsap = G.gsap;
    for (const [tl] of drive) tl.pause();
    if (gsap && typeof gsap.updateRoot === "function") gsap.updateRoot(t);
    for (const [tl, offset] of drive) {
      const local = Math.max(0, t - offset);
      tl.totalTime(local + 0.000001, true);
      tl.totalTime(local, false);
    }
    if (N.generation() !== S.generation) fail(`scripts changed the document structure while the timeline played (at ${t}s)`);
    const out = [];
    for (const i of S.touched) {
      const now = S.styles.get(i) || new Map();
      const before = S.last.get(i) || new Map();
      for (const [k, v] of now) if (before.get(k) !== v) out.push(i, k, v);
      for (const k of before.keys()) if (!now.has(k)) out.push(i, k, null);
      S.last.set(i, new Map(now));
      const overlay = S.overlay.get(i);
      if (overlay) {
        for (const [k, v] of overlay) out.push(i, k, v);
        S.overlay.delete(i);
        const lastOverlay = S.last.get("o" + i) || new Map();
        for (const [k, v] of overlay) lastOverlay.set(k, v);
        S.last.set("o" + i, lastOverlay);
      }
    }
    S.touched.clear();
    return out.map((v) => (v === null || v === undefined ? "\x02" : String(v))).join("\x01");
  },
};
// Overlay values persist across frames for reads.
const overlayRead = S.overlay.get.bind(S.overlay);
S.overlay.get = (i) => overlayRead(i) || S.last.get("o" + i);
})();
