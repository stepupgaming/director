// Director's compile-time GSAP recorder. It runs the page's inline scripts
// against a read-only DOM snapshot and records every tween as flat
// per-target records; Rust turns those into director-timeline/1 tracks.
(function () {
  "use strict";
  const DOM = globalThis.__director_dom;
  const BIG = 1e8;
  const extraction = " requires extraction; run director-engine html capture";
  const refuse = (what) => {
    throw new Error(what + extraction);
  };

  // ---- Read-only DOM -------------------------------------------------------
  const wrappers = [];
  const wrap = (i) => wrappers[i] || (wrappers[i] = new El(i));
  const select = (selector, scope) => {
    const found = globalThis.__director_select(String(selector), scope);
    return found === "" ? [] : found.split(",").map((i) => wrap(+i));
  };
  const style = new Proxy(
    {},
    {
      get: () => refuse("reading or writing element style from script"),
      set: () => refuse("reading or writing element style from script"),
    }
  );
  class El {
    constructor(i) {
      this.__index = i;
    }
    get __node() {
      return DOM.elements[this.__index];
    }
    get nodeType() {
      return 1;
    }
    get id() {
      return this.__node.attrs.id || "";
    }
    get tagName() {
      return this.__node.tag.toUpperCase();
    }
    get nodeName() {
      return this.tagName;
    }
    get className() {
      return this.__node.attrs.class || "";
    }
    get classList() {
      const names = this.className.split(/\s+/).filter(Boolean);
      return {
        contains: (n) => names.includes(n),
        get length() {
          return names.length;
        },
        add: () => refuse("changing classes from script"),
        remove: () => refuse("changing classes from script"),
        toggle: () => refuse("changing classes from script"),
      };
    }
    get dataset() {
      const out = {};
      for (const [k, v] of Object.entries(this.__node.attrs)) {
        if (k.startsWith("data-")) {
          out[k.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())] = v;
        }
      }
      return out;
    }
    getAttribute(name) {
      const v = this.__node.attrs[String(name).toLowerCase()];
      return v === undefined ? null : v;
    }
    hasAttribute(name) {
      return String(name).toLowerCase() in this.__node.attrs;
    }
    get textContent() {
      return this.__node.text;
    }
    set textContent(_) {
      refuse("changing text from script");
    }
    get innerText() {
      return this.__node.text;
    }
    set innerHTML(_) {
      refuse("changing markup from script");
    }
    get children() {
      return this.__node.children.map(wrap);
    }
    get childElementCount() {
      return this.__node.children.length;
    }
    get firstElementChild() {
      const c = this.__node.children;
      return c.length ? wrap(c[0]) : null;
    }
    get lastElementChild() {
      const c = this.__node.children;
      return c.length ? wrap(c[c.length - 1]) : null;
    }
    get parentElement() {
      const p = this.__node.parent;
      return p < 0 ? null : wrap(p);
    }
    get parentNode() {
      return this.parentElement;
    }
    get style() {
      return style;
    }
    matches(selector) {
      return select(selector, -1).includes(this);
    }
    closest(selector) {
      const all = select(selector, -1);
      for (let el = this; el; el = el.parentElement) {
        if (all.includes(el)) return el;
      }
      return null;
    }
    querySelector(selector) {
      return select(selector, this.__index)[0] || null;
    }
    querySelectorAll(selector) {
      return select(selector, this.__index);
    }
    getBoundingClientRect() {
      refuse("layout measurement from script");
    }
    get offsetWidth() {
      return refuse("layout measurement from script");
    }
    get offsetHeight() {
      return refuse("layout measurement from script");
    }
    get clientWidth() {
      return refuse("layout measurement from script");
    }
    get clientHeight() {
      return refuse("layout measurement from script");
    }
    appendChild() {
      refuse("DOM mutation from script");
    }
    removeChild() {
      refuse("DOM mutation from script");
    }
    insertBefore() {
      refuse("DOM mutation from script");
    }
    remove() {
      refuse("DOM mutation from script");
    }
    setAttribute() {
      refuse("DOM mutation from script");
    }
    addEventListener() {}
    removeEventListener() {}
  }

  const queued = [];
  const listen = (type, fn) => {
    if (type === "DOMContentLoaded" || type === "load") queued.push(fn);
  };
  globalThis.window = globalThis;
  globalThis.self = globalThis;
  globalThis.innerWidth = DOM.width;
  globalThis.innerHeight = DOM.height;
  globalThis.devicePixelRatio = 1;
  globalThis.addEventListener = listen;
  globalThis.removeEventListener = () => {};
  globalThis.document = {
    readyState: "complete",
    fonts: { ready: Promise.resolve(), load: () => Promise.resolve([]) },
    get documentElement() {
      return select("html", -1)[0] || null;
    },
    get body() {
      return select("body", -1)[0] || null;
    },
    getElementById: (id) => {
      const i = DOM.elements.findIndex((e) => e.attrs.id === String(id));
      return i < 0 ? null : wrap(i);
    },
    querySelector: (s) => select(s, -1)[0] || null,
    querySelectorAll: (s) => select(s, -1),
    getElementsByClassName: (c) => select("." + CSS.escape(String(c)), -1),
    getElementsByTagName: (t) => select(String(t), -1),
    addEventListener: listen,
    removeEventListener: () => {},
    createElement: () => refuse("DOM mutation from script"),
    createElementNS: () => refuse("DOM mutation from script"),
    createTextNode: () => refuse("DOM mutation from script"),
  };
  globalThis.CSS = {
    escape: (s) => String(s).replace(/([^\w-])/g, "\\$1"),
  };
  globalThis.console = { log() {}, info() {}, warn() {}, error() {}, debug() {} };
  Math.random = () => refuse("Math.random (nondeterministic)");
  Date.now = () => refuse("Date.now (nondeterministic)");
  globalThis.performance = { now: () => refuse("performance.now (nondeterministic)") };
  for (const name of [
    "requestAnimationFrame",
    "setTimeout",
    "setInterval",
    "matchMedia",
    "fetch",
    "getComputedStyle",
    "ResizeObserver",
    "IntersectionObserver",
    "MutationObserver",
  ]) {
    globalThis[name] = () => refuse(name);
  }
  globalThis.__timelines = {};

  // ---- GSAP recording ------------------------------------------------------
  const special = new Set([
    "duration",
    "delay",
    "ease",
    "repeat",
    "yoyo",
    "repeatDelay",
    "stagger",
    "immediateRender",
    "overwrite",
    "id",
    "lazy",
    "data",
    "force3D",
    "inherit",
    "paused",
    "defaults",
  ]);
  const callbacks = /^on[A-Z]/;
  const refused = {
    keyframes: "GSAP keyframes",
    startAt: "GSAP startAt",
    yoyoEase: "GSAP yoyoEase",
    repeatRefresh: "GSAP repeatRefresh",
    runBackwards: "GSAP runBackwards",
    clearProps: "GSAP clearProps",
    transformOrigin: "GSAP transformOrigin",
    motionPath: "GSAP plugins",
    morphSVG: "GSAP plugins",
    drawSVG: "GSAP plugins",
    text: "GSAP plugins",
    scrambleText: "GSAP plugins",
    scrollTrigger: "GSAP ScrollTrigger",
    snap: "GSAP snap",
    modifiers: "GSAP modifiers",
    timeScale: "GSAP timeScale",
  };
  let order = 0;
  const timelines = [];
  const globalDefaults = { duration: 0.5, ease: "power1.out" };
  const rootSets = [];

  const targetsOf = (targets) => {
    if (targets == null) return [];
    if (typeof targets === "string") return select(targets, -1);
    if (targets instanceof El) return [targets];
    if (Array.isArray(targets)) {
      const out = [];
      for (const t of targets) for (const el of targetsOf(t)) out.includes(el) || out.push(el);
      return out;
    }
    if (typeof targets === "object" && typeof targets.length === "number") {
      return targetsOf(Array.from(targets));
    }
    refuse("GSAP targets other than elements and selectors");
  };

  const staggerFn = (stagger, count) => {
    if (stagger == null || stagger === 0) return () => 0;
    if (typeof stagger === "number") return (i) => i * stagger;
    if (typeof stagger === "function") return (i, el, all) => +stagger(i, el, all) || 0;
    if (typeof stagger !== "object") refuse("GSAP stagger '" + stagger + "'");
    for (const key of Object.keys(stagger)) {
      if (!["each", "amount", "from"].includes(key)) refuse("GSAP stagger option '" + key + "'");
    }
    const from = stagger.from === undefined ? 0 : stagger.from;
    const last = count - 1;
    const distance = (i) => {
      if (typeof from === "number") return Math.abs(i - from);
      switch (from) {
        case "start":
          return i;
        case "end":
          return last - i;
        case "center":
          return Math.abs(i - last / 2);
        case "edges":
          return Math.min(i, last - i);
        default:
          return refuse("GSAP stagger from '" + from + "'");
      }
    };
    let max = 0;
    for (let i = 0; i < count; i++) max = Math.max(max, distance(i));
    const each =
      stagger.amount !== undefined ? (max ? stagger.amount / max : 0) : +stagger.each || 0;
    return (i) => distance(i) * each;
  };

  const isRelative = (v) => typeof v === "string" && /^[+\-*]=/.test(v);
  const resolveValue = (v, i, el, all) => (typeof v === "function" ? v(i, el, all) : v);

  // A tween owns its targets and timing; it records one entry per target.
  class Tween {
    constructor(method, targets, fromVars, toVars, defaults) {
      const vars = Object.assign({}, globalDefaults, defaults || {}, toVars);
      for (const key of Object.keys(toVars).concat(fromVars ? Object.keys(fromVars) : [])) {
        if (callbacks.test(key)) refuse("GSAP callback '" + key + "'");
        if (key in refused) refuse(refused[key]);
      }
      if (typeof vars.ease === "function") refuse("GSAP custom ease functions");
      this.method = method;
      this.targets = targetsOf(targets);
      this.duration = method === "set" ? 0 : +vars.duration;
      this.delay = +vars.delay || 0;
      this.ease = vars.ease == null ? "none" : String(vars.ease);
      this.repeat = method === "set" ? 0 : Math.trunc(+vars.repeat || 0);
      this.yoyo = !!vars.yoyo;
      this.repeatDelay = +vars.repeatDelay || 0;
      this.immediate =
        method === "from" || method === "fromTo"
          ? vars.immediateRender !== false
          : !!vars.immediateRender;
      const offset = staggerFn(vars.stagger, this.targets.length);
      this.offsets = this.targets.map((el, i) => offset(i, el, this.targets));
      if (this.repeat && this.offsets.some((o) => o !== 0)) {
        refuse("GSAP repeat on a staggered tween (put repeat in the stagger object)");
      }
      this.span = Math.max(0, ...this.offsets);
      this.props = this.targets.map((el, i) => {
        const props = {};
        const take = (source, side) => {
          for (const [key, raw] of Object.entries(source || {})) {
            if (special.has(key)) continue;
            const v = resolveValue(raw, i, el, this.targets);
            if (v === undefined || v === null) continue;
            if (typeof v !== "number" && typeof v !== "string") {
              refuse("GSAP value for '" + key + "' that is not a number or string");
            }
            (props[key] = props[key] || {})[side] = v;
          }
        };
        if (method === "from") take(toVars, "from");
        else {
          take(fromVars, "from");
          take(toVars, "to");
        }
        return props;
      });
      this.start = 0;
    }
    iteration() {
      return this.duration + this.span;
    }
    totalDuration() {
      if (this.repeat < 0) return BIG;
      return this.iteration() * (this.repeat + 1) + this.repeatDelay * this.repeat;
    }
    endTime(withRepeats) {
      return this.start + (withRepeats ? this.totalDuration() : this.iteration());
    }
    flatten(offset, out) {
      this.targets.forEach((el, i) => {
        out.push({
          target: el.__index,
          order: this.order,
          start: offset + this.start + this.offsets[i],
          duration: this.duration,
          ease: this.ease,
          repeat: this.repeat,
          yoyo: this.yoyo,
          repeatDelay: this.repeatDelay,
          immediate: this.immediate,
          base: !!this.base,
          method: this.method,
          props: this.props[i],
        });
      });
    }
  }

  class Timeline {
    constructor(vars) {
      vars = vars || {};
      for (const key of Object.keys(vars)) {
        if (callbacks.test(key)) refuse("GSAP callback '" + key + "'");
        if (key === "repeat" && vars.repeat) refuse("GSAP timeline repeat");
        if (key === "yoyo" && vars.yoyo) refuse("GSAP timeline yoyo");
        if (key in refused) refuse(refused[key]);
      }
      this.vars = vars;
      this.defaults = vars.defaults || {};
      this.children = [];
      this.labels = {};
      this.recent = null;
      this.start = 0;
      this.delay = +vars.delay || 0;
      this.parent = null;
      this.order = order++;
      timelines.push(this);
    }
    get _dur() {
      let end = 0;
      for (const c of this.children) end = Math.max(end, c.endTime(true));
      return end;
    }
    duration() {
      return this._dur;
    }
    totalDuration() {
      return this._dur;
    }
    endTime(withRepeats) {
      return this.start + this._dur;
    }
    position(position, inserting) {
      const recent = this.recent || { start: 0, endTime: () => 0, totalDuration: () => 0 };
      const clipped = this._dur >= BIG ? recent.endTime(false) : this._dur;
      if (typeof position === "string" && (isNaN(position) || position in this.labels)) {
        const first = position.charAt(0);
        const percent = position.slice(-1) === "%";
        let i = position.indexOf("=");
        if (first === "<" || first === ">") {
          if (i >= 0) position = position.replace(/=/, "");
          const base = first === "<" ? recent.start : recent.endTime(recent.repeat >= 0);
          const scale = percent ? (i < 0 ? recent : inserting).totalDuration() / 100 : 1;
          return base + (parseFloat(position.slice(1)) || 0) * scale;
        }
        if (i < 0) {
          if (!(position in this.labels)) this.labels[position] = clipped;
          return this.labels[position];
        }
        let offset = parseFloat(position.charAt(i - 1) + position.slice(i + 1));
        if (percent && inserting) offset = (offset / 100) * inserting.totalDuration();
        return i > 1 ? this.position(position.slice(0, i - 1), inserting) + offset : clipped + offset;
      }
      return position == null ? clipped : +position;
    }
    insert(child, position) {
      if (child.parent) refuse("adding one GSAP timeline to two parents");
      child.start = this.position(position, child) + (child.delay || 0);
      child.parent = this;
      this.children.push(child);
      this.recent = child;
      return this;
    }
    tween(method, targets, fromVars, toVars, position) {
      if (typeof toVars !== "object" || toVars === null) refuse("GSAP " + method + " without vars");
      const tween = new Tween(method, targets, fromVars, toVars, this.defaults);
      tween.order = order++;
      return this.insert(tween, position);
    }
    to(targets, vars, position) {
      return this.tween("to", targets, null, vars, position);
    }
    from(targets, vars, position) {
      return this.tween("from", targets, null, vars, position);
    }
    fromTo(targets, fromVars, toVars, position) {
      return this.tween("fromTo", targets, fromVars, toVars, position);
    }
    set(targets, vars, position) {
      return this.tween("set", targets, null, Object.assign({}, vars, { duration: 0 }), position);
    }
    add(child, position) {
      if (child instanceof Timeline) return this.insert(child, position);
      if (child instanceof Tween) refuse("adding a standalone GSAP tween");
      if (typeof child === "string") return this.addLabel(child, position);
      if (typeof child === "function") refuse("GSAP callback via add()");
      if (Array.isArray(child)) {
        child.forEach((c) => this.add(c, position));
        return this;
      }
      return refuse("GSAP add() of " + typeof child);
    }
    addLabel(label, position) {
      this.labels[label] = this.position(position);
      return this;
    }
    call() {
      refuse("GSAP callback via call()");
    }
    addPause() {
      refuse("GSAP addPause");
    }
    pause() {
      return this;
    }
    play() {
      return this;
    }
    paused() {
      return this;
    }
    seek() {
      return this;
    }
    progress() {
      return this;
    }
    time() {
      return refuse("reading GSAP time from script");
    }
    eventCallback() {
      refuse("GSAP eventCallback");
    }
    kill() {
      refuse("GSAP kill");
    }
    flatten(offset, out) {
      for (const child of this.children) child.flatten(offset + this.start, out);
    }
  }

  const unsupported = (name) => () => refuse("gsap." + name);
  globalThis.gsap = {
    version: "3.14.2",
    timeline: (vars) => new Timeline(vars),
    set: (targets, vars) => {
      const tween = new Tween("set", targets, null, Object.assign({}, vars, { duration: 0 }));
      tween.order = order++;
      tween.immediate = true;
      tween.base = true;
      rootSets.push(tween);
      return tween;
    },
    to: unsupported("to outside a timeline"),
    from: unsupported("from outside a timeline"),
    fromTo: unsupported("fromTo outside a timeline"),
    delayedCall: unsupported("delayedCall"),
    quickTo: unsupported("quickTo"),
    quickSetter: unsupported("quickSetter"),
    getProperty: unsupported("getProperty"),
    killTweensOf: unsupported("killTweensOf"),
    ticker: { add: unsupported("ticker"), remove: () => {}, lagSmoothing: () => {} },
    defaults: (vars) => {
      for (const key of Object.keys(vars || {})) {
        if (key !== "duration" && key !== "ease") refuse("gsap.defaults '" + key + "'");
      }
      Object.assign(globalDefaults, vars);
      return globalDefaults;
    },
    config: () => {},
    registerPlugin: () => {},
    registerEase: unsupported("registerEase"),
    context: (fn) => {
      if (typeof fn === "function") fn();
      return { revert: () => {}, kill: () => {}, add: (f) => f() };
    },
    matchMedia: unsupported("matchMedia"),
    utils: {
      toArray: (t) => targetsOf(t),
      clamp: (a, b, v) => Math.min(Math.max(v, a), b),
      interpolate: (a, b, p) => a + (b - a) * p,
      mapRange: (a, b, c, d, v) => c + ((v - a) / (b - a)) * (d - c),
      wrap: (a, b, v) => {
        const r = b - a;
        return a + ((((v - a) % r) + r) % r);
      },
      random: unsupported("utils.random"),
      selector: () => (s) => select(s, -1),
    },
  };

  const guard = (fn) => {
    try {
      return JSON.stringify(fn());
    } catch (e) {
      return JSON.stringify({ error: String((e && e.message) || e) });
    }
  };
  globalThis.__director_ready = () =>
    guard(() => {
      for (const fn of queued.splice(0)) fn({ type: "DOMContentLoaded" });
      return {};
    });
  globalThis.__director_export = () =>
    guard(() => {
      const registered = Object.entries(globalThis.__timelines).filter(
        ([, tl]) => tl instanceof Timeline
      );
      let root = registered.find(([key]) => key === DOM.composition);
      if (!root && registered.length === 1) root = registered[0];
      if (!root && registered.length > 1) {
        refuse(
          "several window.__timelines entries without one named '" + DOM.composition + "'"
        );
      }
      const tl = root && root[1];
      if (tl && tl.parent) refuse("registering a nested GSAP timeline");
      for (const other of timelines) {
        if (other !== tl && !other.parent && other.children.length) {
          refuse("a GSAP timeline that is neither registered in window.__timelines nor nested");
        }
      }
      const records = [];
      for (const tween of rootSets) tween.flatten(0, records);
      if (tl) tl.flatten(0, records);
      return { records: records };
    });
})();
