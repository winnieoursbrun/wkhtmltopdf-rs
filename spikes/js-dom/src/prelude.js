// JS half of the spike's DOM bindings. Natives live in `__wk` (installed from Rust) and on the
// Node/Element/Document prototypes; this file adds what is cheaper to express in JS: EventTarget,
// Event, CSSStyleDeclaration proxy, DOMTokenList, Location, constructors for instanceof checks,
// timers/console aliases and the optional missing-API audit.
(function (g) {
  'use strict';
  var wk = g.__wk, P = wk.protos;

  // ---- constructors (instanceof support; not constructible, like in browsers) ----
  function illegal() { throw new TypeError('Illegal constructor'); }
  function defCtor(name, proto) {
    var C = function () { illegal(); };
    Object.defineProperty(C, 'name', { value: name });
    C.prototype = proto;
    Object.defineProperty(proto, 'constructor', { value: C, writable: true, configurable: true });
    g[name] = C;
    return C;
  }
  defCtor('EventTarget', P.eventTarget);
  defCtor('Node', P.node);
  defCtor('Element', P.element);
  g.HTMLElement = g.Element;
  defCtor('CharacterData', P.characterData);
  defCtor('Text', P.text);
  defCtor('Comment', P.comment);
  defCtor('Document', P.document);
  g.HTMLDocument = g.Document;
  defCtor('DocumentFragment', P.fragment);
  ['ELEMENT_NODE', 1, 'TEXT_NODE', 3, 'COMMENT_NODE', 8, 'DOCUMENT_NODE', 9, 'DOCUMENT_FRAGMENT_NODE', 11]
    .forEach(function (v, i, a) { if (i % 2 === 0) g.Node[v] = P.node[v] = a[i + 1]; });

  // ---- events ----
  var listeners = new WeakMap(); // target -> {type: [{fn, capture, once}]}
  function Event(type, init) {
    this.type = String(type);
    init = init || {};
    this.bubbles = !!init.bubbles;
    this.cancelable = !!init.cancelable;
    this.defaultPrevented = false;
    this.target = null; this.currentTarget = null; this.eventPhase = 0;
    this.timeStamp = wk.now();
    this._stop = false; this._stopNow = false;
  }
  Event.prototype.preventDefault = function () { if (this.cancelable) this.defaultPrevented = true; };
  Event.prototype.stopPropagation = function () { this._stop = true; };
  Event.prototype.stopImmediatePropagation = function () { this._stop = this._stopNow = true; };
  g.Event = Event;
  g.CustomEvent = function (type, init) { Event.call(this, type, init); this.detail = init && init.detail; };
  g.CustomEvent.prototype = Object.create(Event.prototype);

  function addEventListener(type, fn, opts) {
    if (!fn) return;
    var capture = typeof opts === 'boolean' ? opts : !!(opts && opts.capture);
    var map = listeners.get(this);
    if (!map) { map = {}; listeners.set(this, map); }
    var list = map[type] || (map[type] = []);
    for (var i = 0; i < list.length; i++) if (list[i].fn === fn && list[i].capture === capture) return;
    list.push({ fn: fn, capture: capture, once: !!(opts && opts.once) });
  }
  function removeEventListener(type, fn, opts) {
    var capture = typeof opts === 'boolean' ? opts : !!(opts && opts.capture);
    var map = listeners.get(this), list = map && map[type];
    if (!list) return;
    for (var i = 0; i < list.length; i++) if (list[i].fn === fn && list[i].capture === capture) { list.splice(i, 1); return; }
  }
  function invoke(target, ev, capturePhase) {
    var map = listeners.get(target), list = map && map[ev.type];
    ev.currentTarget = target;
    if (list) {
      list = list.slice();
      for (var i = 0; i < list.length && !ev._stopNow; i++) {
        var l = list[i];
        if (capturePhase !== null && l.capture !== capturePhase) continue;
        if (l.once) removeEventListener.call(target, ev.type, l.fn, l.capture);
        try {
          var r = typeof l.fn === 'function' ? l.fn.call(target, ev) : l.fn.handleEvent(ev);
          if (r === false) ev.preventDefault();
        } catch (e) { wk.reportError(e); }
      }
    }
    // on<type> handler properties (window.onload = ..., body onload attribute compiled by host)
    var h = target['on' + ev.type];
    if (typeof h === 'function' && capturePhase !== true && !ev._stopNow) {
      try { if (h.call(target, ev) === false) ev.preventDefault(); } catch (e) { wk.reportError(e); }
    }
  }
  function dispatchEvent(ev) {
    ev.target = this;
    var path = [], n = this;
    if (n !== g) {
      while (n) { path.push(n); n = n.parentNode; }
      if (path[path.length - 1] === g.document) path.push(g);
    } else path.push(g);
    ev.eventPhase = 1;
    for (var i = path.length - 1; i > 0 && !ev._stop; i--) invoke(path[i], ev, true);
    ev.eventPhase = 2;
    if (!ev._stop) invoke(path[0], ev, null);
    ev.eventPhase = 3;
    if (ev.bubbles) for (var j = 1; j < path.length && !ev._stop; j++) invoke(path[j], ev, false);
    ev.currentTarget = null; ev.eventPhase = 0;
    return !ev.defaultPrevented;
  }
  [P.eventTarget, g].forEach(function (o) {
    Object.defineProperty(o, 'addEventListener', { value: addEventListener, writable: true, configurable: true });
    Object.defineProperty(o, 'removeEventListener', { value: removeEventListener, writable: true, configurable: true });
    Object.defineProperty(o, 'dispatchEvent', { value: dispatchEvent, writable: true, configurable: true });
  });
  wk.fire = function (target, type, bubbles) { return target.dispatchEvent(new Event(type, { bubbles: bubbles })); };

  // ---- element.style (CSSStyleDeclaration backed by the style attribute) ----
  function kebab(p) { return p.charAt(0) === '-' ? p : p.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); }); }
  var styles = new WeakMap();
  Object.defineProperty(P.element, 'style', {
    configurable: true,
    get: function () {
      var el = this, s = styles.get(el);
      if (s) return s;
      var base = {
        getPropertyValue: function (p) { return wk.styleGet(el, kebab(p)); },
        setProperty: function (p, v, prio) { wk.styleSet(el, kebab(p), v == null ? '' : String(v), prio === 'important'); },
        removeProperty: function (p) { var o = wk.styleGet(el, kebab(p)); wk.styleSet(el, kebab(p), '', false); return o; }
      };
      s = new Proxy(base, {
        get: function (t, k) {
          if (typeof k !== 'string' || k in t) return t[k];
          if (k === 'cssText') return el.getAttribute('style') || '';
          return wk.styleGet(el, kebab(k));
        },
        set: function (t, k, v) {
          if (k === 'cssText') el.setAttribute('style', String(v));
          else wk.styleSet(el, kebab(String(k)), v == null ? '' : String(v), false);
          return true;
        },
        has: function (t, k) { return k in t || wk.styleKnown(kebab(String(k))); }
      });
      styles.set(el, s);
      return s;
    },
    set: function (v) { this.setAttribute('style', String(v)); }
  });
  g.getComputedStyle = function (el) {
    return new Proxy({
      getPropertyValue: function (p) { return wk.computed(el, kebab(p)); }
    }, {
      get: function (t, k) { return typeof k !== 'string' || k in t ? t[k] : wk.computed(el, kebab(k)); }
    });
  };

  // ---- classList (DOMTokenList over the class attribute) ----
  Object.defineProperty(P.element, 'classList', {
    configurable: true,
    get: function () {
      var el = this;
      function toks() { return (el.getAttribute('class') || '').split(/\s+/).filter(Boolean); }
      function write(t) { el.setAttribute('class', t.join(' ')); }
      return {
        get length() { return toks().length; },
        item: function (i) { return toks()[i] || null; },
        contains: function (c) { return toks().indexOf(c) >= 0; },
        add: function () { var t = toks(); for (var i = 0; i < arguments.length; i++) if (t.indexOf(arguments[i]) < 0) t.push(arguments[i]); write(t); },
        remove: function () { var a = [].slice.call(arguments); write(toks().filter(function (c) { return a.indexOf(c) < 0; })); },
        toggle: function (c, f) { var has = this.contains(c); if (f === undefined) f = !has; if (f) this.add(c); else this.remove(c); return f; },
        toString: function () { return el.getAttribute('class') || ''; }
      };
    }
  });

  // script.text, canvas.getContext
  Object.defineProperty(P.element, 'text', {
    configurable: true,
    get: function () { return this.textContent; },
    set: function (v) { this.textContent = v; }
  });
  Object.defineProperty(P.element, 'getContext', {
    configurable: true, writable: true,
    value: function (kind) {
      if (this.localName !== 'canvas') return undefined;
      wk.missing('HTMLCanvasElement.getContext(' + kind + ')');
      return wk.canvasRecorder ? recorder('ctx2d', this) : null;
    }
  });
  // HTMLCanvasElement.width/height reflection (unsigned long, defaults 300x150).
  [['width', 300], ['height', 150]].forEach(function (p) {
    Object.defineProperty(P.element, p[0], {
      configurable: true,
      get: function () {
        var v = this.getAttribute(p[0]);
        if (this.localName === 'canvas' || this.localName === 'img') return v === null ? (this.localName === 'canvas' ? p[1] : 0) : (parseInt(v, 10) || 0);
        return undefined;
      },
      set: function (v) { this.setAttribute(p[0], String(v >>> 0)); }
    });
  });
  // Canvas audit: a context that records every member touched (the API surface P4 must implement).
  function recorder(name, canvas) {
    var used = wk.canvasUsed || (wk.canvasUsed = {});
    var target = function () {};
    return new Proxy(target, {
      get: function (t, k) {
        if (typeof k !== 'string') return undefined;
        if (k === 'then') return undefined;
        used[name + '.' + k] = (used[name + '.' + k] || 0) + 1;
        if (k === 'measureText') return function (s) { return { width: String(s).length * 6, actualBoundingBoxAscent: 8, actualBoundingBoxDescent: 2 }; };
        if (k === 'canvas') return canvas;
        if (/^(create|get)/.test(k)) return function () { return recorder(k); };
        return function () {};
      },
      set: function (t, k) { used[name + '.' + String(k) + '='] = (used[name + '.' + String(k) + '='] || 0) + 1; return true; }
    });
  }

  // ---- location (parsed from the host URL) ----
  var href = wk.url;
  var m = /^([a-z][a-z0-9+.-]*:)(?:\/\/([^\/?#:]*)(?::(\d+))?)?([^?#]*)(\?[^#]*)?(#.*)?$/i.exec(href) || [];
  var location = {
    href: href, protocol: m[1] || '', hostname: m[2] || '', port: m[3] || '',
    pathname: m[4] || '', search: m[5] || '', hash: m[6] || '',
    toString: function () { return this.href; },
    reload: function () {}, assign: function () {}, replace: function () {}
  };
  location.host = location.hostname + (location.port ? ':' + location.port : '');
  location.origin = location.protocol === 'file:' ? 'null' : location.protocol + '//' + location.host;
  g.location = location;
  Object.defineProperty(P.document, 'location', { get: function () { return location; }, configurable: true });
  Object.defineProperty(P.document, 'URL', { get: function () { return href; }, configurable: true });
  Object.defineProperty(P.document, 'defaultView', { get: function () { return g; }, configurable: true });
  Object.defineProperty(P.document, 'readyState', { get: function () { return wk.readyState; }, configurable: true });
  Object.defineProperty(P.document, 'implementation', {
    configurable: true,
    get: function () {
      return {
        hasFeature: function () { return true; },
        // Shim: blitz-dom has one arena per document, so an inert second document is emulated with
        // detached elements of the main one (ownerDocument stays the main document).
        createHTMLDocument: function (title) {
          wk.missing('document.implementation.createHTMLDocument (shimmed)');
          var d = Object.create(g.document);
          var html = g.document.createElement('html'), head = g.document.createElement('head'), body = g.document.createElement('body');
          html.appendChild(head); html.appendChild(body);
          Object.defineProperty(d, 'documentElement', { value: html });
          Object.defineProperty(d, 'head', { value: head });
          Object.defineProperty(d, 'body', { value: body });
          return d;
        }
      };
    }
  });
  Object.defineProperty(P.document, 'compatMode', { value: 'CSS1Compat', configurable: true });

  // ---- window ----
  g.window = g; g.self = g; g.top = g; g.parent = g; g.frames = g;
  g.status = '';
  g.devicePixelRatio = 1;
  g.innerWidth = wk.viewportWidth; g.innerHeight = wk.viewportHeight;
  g.navigator = {
    userAgent: 'Mozilla/5.0 (Unknown; Linux x86_64) AppleWebKit/534.34 (KHTML, like Gecko) wkhtmltopdf Safari/534.34',
    platform: 'Linux x86_64', language: 'en-US', cookieEnabled: false, javaEnabled: function () { return false; }
  };
  g.screen = { width: wk.viewportWidth, height: wk.viewportHeight, availWidth: wk.viewportWidth, availHeight: wk.viewportHeight, colorDepth: 24 };
  g.history = { length: 1, pushState: function () {}, replaceState: function () {}, back: function () {} };
  g.setTimeout = function (fn, ms) { return wk.addTimer(fn, +ms || 0, false, [].slice.call(arguments, 2)); };
  g.setInterval = function (fn, ms) { return wk.addTimer(fn, +ms || 0, true, [].slice.call(arguments, 2)); };
  g.clearTimeout = g.clearInterval = function (id) { wk.clearTimer(+id || 0); };
  g.requestAnimationFrame = function (fn) { return wk.addTimer(function () { fn(wk.now()); }, 16, false, []); };
  g.cancelAnimationFrame = g.clearTimeout;
  g.print = function () { wk.print(); };
  g.alert = function (m) { wk.dialog('alert', String(m), ''); };
  g.confirm = function (m) { wk.dialog('confirm', String(m), ''); return true; };
  g.prompt = function (m, d) { wk.dialog('prompt', String(m), d === undefined ? '' : String(d)); return ''; };
  g.matchMedia = function (q) { return { matches: wk.mediaMatches(String(q)), media: q, addListener: function () {}, removeListener: function () {}, addEventListener: function () {}, removeEventListener: function () {} }; };
  function fmt(a) { return [].map.call(a, function (x) { return typeof x === 'string' ? x : String(x); }).join(' '); }
  g.console = {};
  ['log', 'info', 'warn', 'error', 'debug', 'trace'].forEach(function (k) { g.console[k] = function () { wk.console(fmt(arguments)); }; });

  // ---- optional audit: record every property looked up on DOM objects or globals that is not there ----
  if (wk.audit) {
    var seen = wk.missingSet = {};
    function trap(label, base) {
      return new Proxy(base, {
        get: function (t, k, r) {
          if (typeof k === 'string' && !(k in t)) seen[label + '.' + k] = (seen[label + '.' + k] || 0) + 1;
          return Reflect.get(t, k, r);
        },
        has: function (t, k) {
          if (typeof k === 'string' && !(k in t)) seen[label + '.' + k + ' (in)'] = (seen[label + '.' + k + ' (in)'] || 0) + 1;
          return k in t;
        }
      });
    }
    // Lookups that fall through every DOM prototype land on the proxy.
    Object.setPrototypeOf(P.eventTarget, trap('Node', Object.prototype));
    try { Object.setPrototypeOf(g, trap('window', Object.getPrototypeOf(g))); } catch (e) { wk.console('audit: cannot proxy global: ' + e); }
  }
})(this);
