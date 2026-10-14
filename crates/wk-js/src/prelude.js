// JavaScript half of the DOM bindings (wk-js). Natives live in `__wk` and on
// the interface prototypes; this file adds what is cheaper to express in JS:
// constructors for instanceof, EventTarget/Event, CSSStyleDeclaration over the
// style attribute, DOMTokenList, dataset, reflected attributes and form state,
// the canvas 2D context (state machine over the native recorder), window,
// navigator, location, timers, storage, XMLHttpRequest and small shims.
// Values mirror QtWebKit 2.2 as embedded in wkhtmltopdf 0.12.6 where they are
// observable (navigator, screen 1366x768, a 0x0 viewport during load).
(function (g) {
  'use strict';
  var wk = g.__wk, P = wk.protos;
  var hasOwn = Object.prototype.hasOwnProperty;
  function def(o, name, get, set) {
    Object.defineProperty(o, name, { get: get, set: set, configurable: true, enumerable: true });
  }
  function val(o, name, v) {
    Object.defineProperty(o, name, { value: v, writable: true, configurable: true, enumerable: false });
  }

  // ---- constructors (instanceof support; not constructible) ----
  function illegal() { throw new TypeError('Illegal constructor'); }
  Object.keys(P).forEach(function (name) {
    var C = function () { illegal(); };
    Object.defineProperty(C, 'name', { value: name });
    C.prototype = P[name];
    val(P[name], 'constructor', C);
    try { Object.defineProperty(P[name], Symbol.toStringTag, { value: name, configurable: true }); } catch (e) {}
    g[name] = C;
  });
  ['ELEMENT_NODE', 1, 'ATTRIBUTE_NODE', 2, 'TEXT_NODE', 3, 'CDATA_SECTION_NODE', 4, 'PROCESSING_INSTRUCTION_NODE', 7,
   'COMMENT_NODE', 8, 'DOCUMENT_NODE', 9, 'DOCUMENT_TYPE_NODE', 10, 'DOCUMENT_FRAGMENT_NODE', 11,
   'DOCUMENT_POSITION_DISCONNECTED', 1, 'DOCUMENT_POSITION_PRECEDING', 2, 'DOCUMENT_POSITION_FOLLOWING', 4,
   'DOCUMENT_POSITION_CONTAINS', 8, 'DOCUMENT_POSITION_CONTAINED_BY', 16]
    .forEach(function (v, i, a) { if (i % 2 === 0) { g.Node[v] = a[i + 1]; P.Node[v] = a[i + 1]; } });
  var doc = g.document;

  // ---- events ----
  var listeners = new WeakMap(); // target -> {type: [{fn, capture, once}]}
  function Event(type, init) {
    if (!(this instanceof Event)) throw new TypeError("Failed to construct 'Event'");
    init = init || {};
    this.type = String(type);
    this.bubbles = !!init.bubbles;
    this.cancelable = !!init.cancelable;
    this.defaultPrevented = false;
    this.target = null; this.currentTarget = null; this.srcElement = null; this.eventPhase = 0;
    this.timeStamp = wk.now();
    this.isTrusted = false;
    this._stop = false; this._stopNow = false;
  }
  Event.NONE = 0; Event.CAPTURING_PHASE = 1; Event.AT_TARGET = 2; Event.BUBBLING_PHASE = 3;
  Event.prototype.preventDefault = function () { if (this.cancelable) { this.defaultPrevented = true; this.returnValue = false; } };
  Event.prototype.stopPropagation = function () { this._stop = true; this.cancelBubble = true; };
  Event.prototype.stopImmediatePropagation = function () { this._stop = this._stopNow = true; };
  Event.prototype.initEvent = function (type, bubbles, cancelable) {
    this.type = String(type); this.bubbles = !!bubbles; this.cancelable = !!cancelable;
  };
  Event.prototype.composedPath = function () { return this._path ? this._path.slice() : []; };
  g.Event = Event;
  function CustomEvent(type, init) { Event.call(this, type, init); this.detail = init && init.detail !== undefined ? init.detail : null; }
  CustomEvent.prototype = Object.create(Event.prototype);
  CustomEvent.prototype.constructor = CustomEvent;
  CustomEvent.prototype.initCustomEvent = function (type, b, c, detail) { this.initEvent(type, b, c); this.detail = detail; };
  g.CustomEvent = CustomEvent;
  ['UIEvent', 'MouseEvent', 'KeyboardEvent', 'FocusEvent', 'HTMLEvents', 'MessageEvent', 'ErrorEvent', 'ProgressEvent'].forEach(function (n) {
    var C = function (type, init) { Event.call(this, type, init); if (init) for (var k in init) if (!(k in this)) this[k] = init[k]; };
    C.prototype = Object.create(Event.prototype);
    C.prototype.constructor = C;
    g[n] = C;
  });

  function capture(opts) { return typeof opts === 'boolean' ? opts : !!(opts && opts.capture); }
  function addEventListener(type, fn, opts) {
    if (!fn) return;
    var cap = capture(opts);
    var map = listeners.get(this);
    if (!map) { map = {}; listeners.set(this, map); }
    var list = hasOwn.call(map, type) ? map[type] : (map[type] = []);
    for (var i = 0; i < list.length; i++) if (list[i].fn === fn && list[i].capture === cap) return;
    list.push({ fn: fn, capture: cap, once: !!(opts && opts.once) });
  }
  function removeEventListener(type, fn, opts) {
    var cap = capture(opts);
    var map = listeners.get(this), list = map && hasOwn.call(map, type) && map[type];
    if (!list) return;
    for (var i = 0; i < list.length; i++) if (list[i].fn === fn && list[i].capture === cap) { list.splice(i, 1); return; }
  }
  // Inline handler attributes (on<type>="…"), compiled on first use.
  var compiled = new WeakMap();
  function attrHandler(target, type) {
    if (!target || typeof target.getAttribute !== 'function' || target.nodeType !== 1) return null;
    var code = target.getAttribute('on' + type);
    if (code === null) return null;
    var cache = compiled.get(target);
    if (!cache) { cache = {}; compiled.set(target, cache); }
    if (!cache[type] || cache[type].code !== code) cache[type] = { code: code, fn: wk.compileHandler(code, target) };
    return cache[type].fn;
  }
  // Exceptions in listeners are reported, not propagated.
  function report(e) { wk.reportException(e); }
  var throwLater = report;
  // Event handler properties (`onload = …`) take the place of a listener
  // registered when the handler is first set, like WebCore's
  // JSLazyEventListener; attribute handlers count from parse time (first).
  var HANDLER_EVENTS = ['load', 'error', 'abort', 'click', 'change', 'input', 'submit', 'reset', 'focus', 'blur',
    'resize', 'scroll', 'unload', 'beforeunload', 'beforeprint', 'afterprint', 'message', 'readystatechange',
    'mousedown', 'mouseup', 'mouseover', 'mouseout', 'mousemove', 'keydown', 'keyup', 'keypress', 'dblclick',
    'contextmenu', 'select', 'hashchange', 'popstate', 'pageshow', 'pagehide', 'storage', 'online', 'offline',
    'animationend', 'transitionend', 'wheel', 'toggle', 'loadend', 'progress', 'timeout', 'loadstart'];
  var handlers = new WeakMap(); // target -> {type: fn}
  function handlerSlot(target, type) { return { slot: true, type: type, capture: false }; }
  function setHandler(target, type, fn) {
    var hs = handlers.get(target);
    if (!hs) { hs = {}; handlers.set(target, hs); }
    hs[type] = typeof fn === 'function' ? fn : null;
    var map = listeners.get(target);
    if (!map) { map = {}; listeners.set(target, map); }
    var list = hasOwn.call(map, type) ? map[type] : (map[type] = []);
    for (var i = 0; i < list.length; i++) if (list[i].slot) return;
    if (hs[type]) list.push({ slot: true, fn: null, capture: false });
  }
  function getHandler(target, type) { var hs = handlers.get(target); return hs && hs[type] || null; }
  HANDLER_EVENTS.forEach(function (type) {
    [P.Node, g].forEach(function (o) {
      Object.defineProperty(o, 'on' + type, {
        configurable: true, enumerable: true,
        get: function () { return getHandler(this, type); },
        set: function (fn) { setHandler(this, type, fn); }
      });
    });
  });
  function callHandler(target, ev, h) {
    try { if (h.call(target, ev) === false) ev.preventDefault(); } catch (e) { report(e); }
  }
  function invoke(target, ev, phase) {
    var map = listeners.get(target), list = map && hasOwn.call(map, ev.type) && map[ev.type];
    ev.currentTarget = target;
    var slotSeen = false;
    // Attribute handlers (parsed) run before listeners added by script.
    if (phase !== 1 && !getHandler(target, ev.type)) {
      var ah = attrHandler(target, ev.type);
      if (typeof ah === 'function') { slotSeen = true; callHandler(target, ev, ah); }
    }
    if (list) {
      list = list.slice();
      for (var i = 0; i < list.length && !ev._stopNow; i++) {
        var l = list[i];
        if (l.slot) {
          slotSeen = true;
          var h = getHandler(target, ev.type);
          if (phase !== 1 && typeof h === 'function') callHandler(target, ev, h);
          continue;
        }
        if (phase === 1 && !l.capture) continue;
        if (phase === 3 && l.capture) continue;
        if (l.once) removeEventListener.call(target, ev.type, l.fn, l.capture);
        try {
          var r = typeof l.fn === 'function' ? l.fn.call(target, ev) : l.fn.handleEvent(ev);
          if (r === false) ev.preventDefault();
        } catch (e) { report(e); }
      }
    }
    // Plain `on<type>` properties of events without an accessor.
    if (phase !== 1 && !slotSeen && !ev._stopNow && HANDLER_EVENTS.indexOf(ev.type) < 0) {
      var p = target['on' + ev.type];
      if (typeof p === 'function') callHandler(target, ev, p);
    }
  }
  function dispatchEvent(ev) {
    if (!(ev instanceof Event)) throw new TypeError('dispatchEvent: parameter 1 is not of type Event');
    ev.target = ev.srcElement = this;
    var path = [], n = this;
    if (n !== g) {
      while (n) { path.push(n); n = n.parentNode; }
      if (path[path.length - 1] === doc && ev.type !== 'load') path.push(g);
    } else path.push(g);
    ev._path = path;
    ev.eventPhase = 1;
    for (var i = path.length - 1; i > 0 && !ev._stop; i--) invoke(path[i], ev, 1);
    ev.eventPhase = 2;
    if (!ev._stop) invoke(path[0], ev, 2);
    if (ev.bubbles) { ev.eventPhase = 3; for (var j = 1; j < path.length && !ev._stop; j++) invoke(path[j], ev, 3); }
    ev.currentTarget = null; ev.eventPhase = 0;
    return !ev.defaultPrevented;
  }
  [P.EventTarget, g].forEach(function (o) {
    val(o, 'addEventListener', addEventListener);
    val(o, 'removeEventListener', removeEventListener);
    val(o, 'dispatchEvent', dispatchEvent);
  });
  wk.fire = function (target, type, bubbles) {
    var ev = new Event(type, { bubbles: bubbles });
    ev.isTrusted = true;
    if (target === g && type === 'load') {
      // The window's load event targets the document.
      ev.target = ev.srcElement = doc; ev.eventPhase = 2;
      invoke(g, ev, 2); ev.currentTarget = null; ev.eventPhase = 0;
      return !ev.defaultPrevented;
    }
    return target.dispatchEvent(ev);
  };
  doc.createEvent = function (kind) {
    var k = String(kind);
    var C = /custom/i.test(k) ? CustomEvent : (g[k.replace(/s$/, '')] || Event);
    var ev = Object.create(C.prototype);
    Event.call(ev, '');
    return ev;
  };

  // ---- CSSStyleDeclaration over the style attribute ----
  function kebab(p) {
    if (p === 'cssFloat' || p === 'styleFloat') return 'float';
    if (/^(webkit|moz|ms|o)[A-Z]/.test(p)) p = '-' + p;
    return p.charAt(0) === '-' && p.indexOf('-', 1) < 0 && /[A-Z]/.test(p)
      ? p.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); })
      : p.replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); });
  }
  var styles = new WeakMap();
  function styleFor(el) {
    var st = styles.get(el);
    if (st) return st;
    var base = {
      getPropertyValue: function (p) { return wk.styleGet(el, String(p).toLowerCase()); },
      getPropertyPriority: function () { return ''; },
      setProperty: function (p, v, prio) { wk.styleSet(el, String(p).toLowerCase(), v == null ? '' : String(v), prio === 'important'); },
      removeProperty: function (p) { var o = wk.styleGet(el, String(p)); wk.styleSet(el, String(p), '', false); return o; },
      item: function (i) { return names()[i] || ''; }
    };
    function names() {
      return (el.getAttribute('style') || '').split(';').map(function (d) { return d.split(':')[0].trim(); }).filter(Boolean);
    }
    st = new Proxy(base, {
      get: function (t, k) {
        if (typeof k !== 'string') return undefined;
        if (k in t) return t[k];
        if (k === 'cssText') return el.getAttribute('style') || '';
        if (k === 'length') return names().length;
        if (/^\d+$/.test(k)) return names()[+k];
        if (k === 'parentRule') return null;
        return wk.styleKnown(kebab(k)) ? wk.styleGet(el, kebab(k)) : undefined;
      },
      set: function (t, k, v) {
        if (typeof k !== 'string') return true;
        if (k === 'cssText') el.setAttribute('style', v == null ? '' : String(v));
        else wk.styleSet(el, kebab(k), v == null ? '' : String(v), false);
        return true;
      },
      has: function (t, k) { return k in t || k === 'cssText' || (typeof k === 'string' && wk.styleKnown(kebab(k))); }
    });
    styles.set(el, st);
    return st;
  }
  def(P.Element, 'style', function () { return styleFor(this); }, function (v) { this.setAttribute('style', String(v)); });
  g.getComputedStyle = function (el, pseudo) {
    if (!el || el.nodeType !== 1) throw new TypeError("getComputedStyle: parameter 1 is not of type 'Element'");
    return new Proxy({
      getPropertyValue: function (p) { return wk.computed(el, String(p).toLowerCase()); },
      getPropertyPriority: function () { return ''; }
    }, {
      get: function (t, k) {
        if (typeof k !== 'string') return undefined;
        if (k in t) return t[k];
        if (k === 'length') return 0;
        if (k === 'cssText') return '';
        return wk.styleKnown(kebab(k)) ? wk.computed(el, kebab(k)) : undefined;
      }
    });
  };
  P.Document.defaultView = g;

  // ---- DOMTokenList over the class attribute ----
  function tokenList(el, attr) {
    function toks() { return (el.getAttribute(attr) || '').split(/[\t\n\f\r ]+/).filter(Boolean); }
    function write(t) { el.setAttribute(attr, t.join(' ')); }
    var list = {
      item: function (i) { var t = toks(); return i < t.length ? t[i] : null; },
      contains: function (c) { return toks().indexOf(String(c)) >= 0; },
      add: function () { var t = toks(); for (var i = 0; i < arguments.length; i++) { var c = String(arguments[i]); if (t.indexOf(c) < 0) t.push(c); } write(t); },
      remove: function () { var a = [].map.call(arguments, String); write(toks().filter(function (c) { return a.indexOf(c) < 0; })); },
      toggle: function (c, f) { var has = this.contains(c); if (f === undefined) f = !has; if (f) this.add(c); else this.remove(c); return !!f; },
      replace: function (a, b) { var t = toks(), i = t.indexOf(a); if (i < 0) return false; t[i] = b; write(t); return true; },
      forEach: function (cb, th) { toks().forEach(cb, th); },
      toString: function () { return el.getAttribute(attr) || ''; }
    };
    def(list, 'length', function () { return toks().length; });
    def(list, 'value', function () { return el.getAttribute(attr) || ''; }, function (v) { el.setAttribute(attr, v); });
    return list;
  }
  def(P.Element, 'classList', function () { return tokenList(this, 'class'); });
  def(P.Element, 'relList', function () { return tokenList(this, 'rel'); });

  // ---- dataset ----
  def(P.Element, 'dataset', function () {
    var el = this;
    function attr(k) { return 'data-' + String(k).replace(/[A-Z]/g, function (c) { return '-' + c.toLowerCase(); }); }
    return new Proxy({}, {
      get: function (t, k) { if (typeof k !== 'string') return undefined; var v = el.getAttribute(attr(k)); return v === null ? undefined : v; },
      set: function (t, k, v) { el.setAttribute(attr(k), String(v)); return true; },
      has: function (t, k) { return el.hasAttribute(attr(k)); },
      deleteProperty: function (t, k) { el.removeAttribute(attr(k)); return true; },
      ownKeys: function () {
        return el.getAttributeNames().filter(function (n) { return n.indexOf('data-') === 0; })
          .map(function (n) { return n.slice(5).replace(/-([a-z])/g, function (m, c) { return c.toUpperCase(); }); });
      },
      getOwnPropertyDescriptor: function (t, k) {
        var v = el.getAttribute(attr(k));
        return v === null ? undefined : { value: v, writable: true, enumerable: true, configurable: true };
      }
    });
  });

  // ---- attributes (static NamedNodeMap) ----
  function Attr(el, name) { this.ownerElement = el; this.name = this.nodeName = this.localName = name; }
  def(Attr.prototype, 'value', function () { return this.ownerElement.getAttribute(this.name); }, function (v) { this.ownerElement.setAttribute(this.name, v); });
  def(Attr.prototype, 'nodeValue', function () { return this.value; }, function (v) { this.value = v; });
  Attr.prototype.specified = true; Attr.prototype.nodeType = 2; Attr.prototype.namespaceURI = null; Attr.prototype.prefix = null;
  g.Attr = Attr;
  def(P.Element, 'attributes', function () {
    var el = this, names = el.getAttributeNames();
    var map = names.map(function (n) { return new Attr(el, n); });
    map.getNamedItem = function (n) { n = String(n).toLowerCase(); for (var i = 0; i < map.length; i++) if (map[i].name === n) return map[i]; return null; };
    map.item = function (i) { return map[i] || null; };
    map.removeNamedItem = function (n) { el.removeAttribute(n); };
    map.setNamedItem = function (a) { el.setAttribute(a.name, a.value); };
    // Named access (`attributes.onsubmit`, jQuery 1.x support tests).
    map.forEach(function (a) { if (!(a.name in map)) map[a.name] = a; });
    return map;
  });
  P.Element.hasAttributes = function () { return this.getAttributeNames().length > 0; };
  P.Element.toggleAttribute = function (n, f) {
    var has = this.hasAttribute(n);
    if (f === undefined) f = !has;
    if (f && !has) this.setAttribute(n, '');
    if (!f && has) this.removeAttribute(n);
    return !!f;
  };
  P.Element.getAttributeNS = function (ns, n) { return this.getAttribute(n); };
  P.Element.setAttributeNS = function (ns, n, v) { this.setAttribute(String(n).replace(/^[^:]*:/, ns && /xlink/.test(ns) ? 'xlink:' : ''), v); };
  P.Element.removeAttributeNS = function (ns, n) { this.removeAttribute(n); };
  P.Element.hasAttributeNS = function (ns, n) { return this.hasAttribute(n); };
  P.Element.getAttributeNode = function (n) { return this.hasAttribute(n) ? new Attr(this, String(n).toLowerCase()) : null; };
  P.Element.getClientRects = function () { var r = this.getBoundingClientRect(); return r.width || r.height ? [r] : []; };
  P.Element.insertAdjacentElement = function (pos, el) {
    switch (String(pos).toLowerCase()) {
      case 'beforebegin': return this.parentNode ? this.parentNode.insertBefore(el, this) : null;
      case 'afterbegin': return this.insertBefore(el, this.firstChild);
      case 'beforeend': return this.appendChild(el);
      case 'afterend': return this.parentNode ? this.parentNode.insertBefore(el, this.nextSibling) : null;
    }
    throw new TypeError('SyntaxError: invalid position');
  };
  P.Element.insertAdjacentText = function (pos, t) { this.insertAdjacentElement(pos, doc.createTextNode(t)); };
  ['focus', 'blur', 'click', 'scrollIntoView', 'scrollTo', 'scrollBy', 'scroll'].forEach(function (m) { P.Element[m] = function () {}; });
  P.Node.isSameNode = function (o) { return this === o; };
  P.Node.isEqualNode = function (o) {
    return !!o && this.nodeType === o.nodeType && (this.nodeType === 1 ? this.outerHTML === o.outerHTML : this.nodeValue === o.nodeValue);
  };
  P.Node.lookupNamespaceURI = function () { return null; };
  P.Node.isDefaultNamespace = function (n) { return n === 'http://www.w3.org/1999/xhtml'; };
  def(P.Node, 'baseURI', function () { return doc.URL; });
  def(P.Node, 'prefix', function () { return null; });
  def(P.Element, 'nodeValue', function () { return null; }, function () {});
  def(P.Document, 'nodeValue', function () { return null; }, function () {});
  def(P.DocumentFragment, 'nodeValue', function () { return null; }, function () {});
  def(P.CharacterData, 'length', function () { return this.data.length; });
  P.CharacterData.appendData = function (s) { this.data += s; };
  P.CharacterData.substringData = function (o, n) { return this.data.substr(o, n); };
  P.CharacterData.insertData = function (o, s) { var d = this.data; this.data = d.slice(0, o) + s + d.slice(o); };
  P.CharacterData.deleteData = function (o, n) { var d = this.data; this.data = d.slice(0, o) + d.slice(o + n); };
  P.CharacterData.replaceData = function (o, n, s) { var d = this.data; this.data = d.slice(0, o) + s + d.slice(o + n); };
  def(P.Text, 'wholeText', function () { return this.data; });
  P.Text.splitText = function (o) {
    var d = this.data, t = doc.createTextNode(d.slice(o));
    this.data = d.slice(0, o);
    if (this.parentNode) this.parentNode.insertBefore(t, this.nextSibling);
    return t;
  };

  // ---- innerText, text ----
  def(P.HTMLElement, 'innerText', function () { return this.textContent; }, function (v) { this.textContent = v; });
  def(P.HTMLElement, 'outerText', function () { return this.textContent; });
  def(P.HTMLScriptElement, 'text', function () { return this.textContent; }, function (v) { this.textContent = v; });
  def(P.HTMLTitleElement, 'text', function () { return this.textContent; }, function (v) { this.textContent = v; });
  def(P.HTMLAnchorElement, 'text', function () { return this.textContent; }, function (v) { this.textContent = v; });
  def(P.HTMLOptionElement, 'text', function () { return this.textContent; }, function (v) { this.textContent = v; });

  // ---- reflected attributes ----
  function reflect(proto, prop, attr, kind, dflt) {
    attr = attr || prop.toLowerCase();
    def(proto, prop,
      kind === 'bool' ? function () { return this.hasAttribute(attr); } :
      kind === 'url' ? function () { var v = this.getAttribute(attr); return v === null ? '' : wk.resolve(v); } :
      kind === 'int' ? function () { var v = parseInt(this.getAttribute(attr), 10); return isNaN(v) ? (dflt || 0) : v; } :
      function () { var v = this.getAttribute(attr); return v === null ? (dflt || '') : v; },
      kind === 'bool' ? function (v) { if (v) this.setAttribute(attr, ''); else this.removeAttribute(attr); } :
      function (v) { this.setAttribute(attr, String(v)); });
  }
  ['title', 'lang', 'dir', 'accessKey'].forEach(function (p) { reflect(P.HTMLElement, p); });
  reflect(P.HTMLElement, 'hidden', 'hidden', 'bool');
  reflect(P.HTMLElement, 'tabIndex', 'tabindex', 'int');
  def(P.HTMLElement, 'isContentEditable', function () { return false; });
  def(P.HTMLElement, 'contentEditable', function () { return 'inherit'; });
  reflect(P.HTMLAnchorElement, 'href', 'href', 'url');
  ['target', 'rel', 'name', 'hreflang', 'type', 'download'].forEach(function (p) { reflect(P.HTMLAnchorElement, p); });
  ['protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash'].forEach(function (p) {
    def(P.HTMLAnchorElement, p, function () { return parseUrl(this.href)[p]; });
  });
  reflect(P.HTMLLinkElement, 'href', 'href', 'url');
  ['rel', 'type', 'media', 'hreflang'].forEach(function (p) { reflect(P.HTMLLinkElement, p); });
  reflect(P.HTMLImageElement, 'src', 'src', 'url');
  ['alt', 'useMap', 'crossOrigin'].forEach(function (p) { reflect(P.HTMLImageElement, p); });
  reflect(P.HTMLImageElement, 'isMap', 'ismap', 'bool');
  def(P.HTMLImageElement, 'complete', function () { return true; });
  def(P.HTMLImageElement, 'naturalWidth', function () { return wk.imageSize(this)[0]; });
  def(P.HTMLImageElement, 'naturalHeight', function () { return wk.imageSize(this)[1]; });
  ['width', 'height'].forEach(function (p, i) {
    def(P.HTMLImageElement, p, function () {
      var v = parseInt(this.getAttribute(p), 10);
      if (!isNaN(v)) return v;
      return this.isConnected ? this['offset' + (i ? 'Height' : 'Width')] || wk.imageSize(this)[i] : wk.imageSize(this)[i];
    }, function (v) { this.setAttribute(p, String(v >>> 0)); });
  });
  reflect(P.HTMLScriptElement, 'src', 'src', 'url');
  ['type', 'charset', 'crossOrigin'].forEach(function (p) { reflect(P.HTMLScriptElement, p); });
  reflect(P.HTMLScriptElement, 'async', 'async', 'bool');
  reflect(P.HTMLScriptElement, 'defer', 'defer', 'bool');
  reflect(P.HTMLIFrameElement, 'src', 'src', 'url');
  ['name', 'width', 'height'].forEach(function (p) { reflect(P.HTMLIFrameElement, p); });
  def(P.HTMLIFrameElement, 'contentWindow', function () { return null; });
  def(P.HTMLIFrameElement, 'contentDocument', function () { return null; });
  ['media', 'type'].forEach(function (p) { reflect(P.HTMLStyleElement, p); });
  def(P.HTMLStyleElement, 'sheet', function () { return { cssRules: [], rules: [], insertRule: function () { return 0; }, deleteRule: function () {}, addRule: function () {} }; });
  def(P.HTMLLinkElement, 'sheet', function () { return null; });
  ['name', 'content', 'httpEquiv'].forEach(function (p) { reflect(P.HTMLMetaElement, p, p === 'httpEquiv' ? 'http-equiv' : null); });
  ['colSpan', 'rowSpan'].forEach(function (p) { reflect(P.HTMLTableCellElement, p, p.toLowerCase(), 'int', 1); });
  def(P.HTMLTableCellElement, 'cellIndex', function () { var p = this.parentNode; return p ? [].indexOf.call(p.children, this) : -1; });
  def(P.HTMLTableElement, 'rows', function () { return [].slice.call(this.querySelectorAll('tr')); });
  def(P.HTMLTableElement, 'tBodies', function () { return [].filter.call(this.children, function (c) { return c.localName === 'tbody'; }); });
  def(P.HTMLTableElement, 'tHead', function () { return this.querySelector('thead'); });
  def(P.HTMLTableElement, 'tFoot', function () { return this.querySelector('tfoot'); });
  def(P.HTMLTableElement, 'caption', function () { return this.querySelector('caption'); });
  P.HTMLTableElement.insertRow = function (i) {
    var body = this.tBodies[0] || this;
    return P.HTMLTableSectionElement.insertRow.call(body, i);
  };
  P.HTMLTableElement.createTHead = function () { var t = this.tHead; if (!t) { t = doc.createElement('thead'); this.insertBefore(t, this.firstChild); } return t; };
  P.HTMLTableElement.createTBody = function () { var t = doc.createElement('tbody'); this.appendChild(t); return t; };
  P.HTMLTableElement.deleteRow = function (i) { var r = this.rows[i < 0 ? this.rows.length - 1 : i]; if (r) r.remove(); };
  def(P.HTMLTableSectionElement, 'rows', function () { return [].filter.call(this.children, function (c) { return c.localName === 'tr'; }); });
  P.HTMLTableSectionElement.insertRow = function (i) {
    var rows = this.rows || [], tr = doc.createElement('tr');
    if (i === undefined || i < 0 || i >= rows.length) this.appendChild(tr); else this.insertBefore(tr, rows[i]);
    return tr;
  };
  P.HTMLTableSectionElement.deleteRow = function (i) { var r = this.rows[i < 0 ? this.rows.length - 1 : i]; if (r) r.remove(); };
  def(P.HTMLTableRowElement, 'cells', function () { return [].filter.call(this.children, function (c) { return c.localName === 'td' || c.localName === 'th'; }); });
  def(P.HTMLTableRowElement, 'rowIndex', function () { var t = this.closest('table'); return t ? t.rows.indexOf(this) : -1; });
  P.HTMLTableRowElement.insertCell = function (i) {
    var cells = this.cells, td = doc.createElement('td');
    if (i === undefined || i < 0 || i >= cells.length) this.appendChild(td); else this.insertBefore(td, cells[i]);
    return td;
  };
  P.HTMLTableRowElement.deleteCell = function (i) { var c = this.cells[i]; if (c) c.remove(); };
  reflect(P.HTMLOListElement, 'start', 'start', 'int', 1);

  // ---- forms: state reflected to attributes, so it prints ----
  [P.HTMLInputElement, P.HTMLSelectElement, P.HTMLTextAreaElement, P.HTMLButtonElement].forEach(function (pr) {
    reflect(pr, 'name');
    reflect(pr, 'disabled', 'disabled', 'bool');
    reflect(pr, 'required', 'required', 'bool');
    reflect(pr, 'autofocus', 'autofocus', 'bool');
    def(pr, 'form', function () { return this.closest('form'); });
    pr.checkValidity = function () { return true; };
    pr.setCustomValidity = function () {};
  });
  ['placeholder', 'min', 'max', 'step', 'pattern', 'accept', 'alt', 'autocomplete'].forEach(function (p) { reflect(P.HTMLInputElement, p); });
  reflect(P.HTMLInputElement, 'readOnly', 'readonly', 'bool');
  reflect(P.HTMLInputElement, 'multiple', 'multiple', 'bool');
  reflect(P.HTMLInputElement, 'size', 'size', 'int', 20);
  reflect(P.HTMLInputElement, 'maxLength', 'maxlength', 'int', -1);
  reflect(P.HTMLInputElement, 'src', 'src', 'url');
  def(P.HTMLInputElement, 'type', function () {
    var t = (this.getAttribute('type') || '').toLowerCase();
    return /^(hidden|text|search|tel|url|email|password|date|month|week|time|datetime-local|number|range|color|checkbox|radio|file|submit|image|reset|button)$/.test(t) ? t : 'text';
  }, function (v) { this.setAttribute('type', v); });
  def(P.HTMLInputElement, 'value', function () {
    var v = this.getAttribute('value');
    if (v === null) return /^(checkbox|radio)$/.test(this.type) ? 'on' : '';
    return v;
  }, function (v) { this.setAttribute('value', v == null ? '' : String(v)); });
  reflect(P.HTMLInputElement, 'defaultValue', 'value');
  def(P.HTMLInputElement, 'checked', function () { return this.hasAttribute('checked'); }, function (v) {
    if (v && this.type === 'radio' && this.name) {
      var self = this, scope = this.form || doc;
      [].forEach.call(scope.getElementsByName(this.name), function (o) { if (o !== self && o.localName === 'input') o.removeAttribute('checked'); });
    }
    if (v) this.setAttribute('checked', ''); else this.removeAttribute('checked');
  });
  reflect(P.HTMLInputElement, 'defaultChecked', 'checked', 'bool');
  P.HTMLInputElement.select = function () {};
  P.HTMLInputElement.setSelectionRange = function () {};
  def(P.HTMLTextAreaElement, 'value', function () { return this.textContent; }, function (v) { this.textContent = v == null ? '' : String(v); });
  def(P.HTMLTextAreaElement, 'defaultValue', function () { return this.textContent; }, function (v) { this.textContent = v; });
  def(P.HTMLTextAreaElement, 'type', function () { return 'textarea'; });
  ['placeholder', 'wrap'].forEach(function (p) { reflect(P.HTMLTextAreaElement, p); });
  reflect(P.HTMLTextAreaElement, 'rows', 'rows', 'int', 2);
  reflect(P.HTMLTextAreaElement, 'cols', 'cols', 'int', 20);
  reflect(P.HTMLTextAreaElement, 'readOnly', 'readonly', 'bool');
  def(P.HTMLButtonElement, 'type', function () { var t = (this.getAttribute('type') || '').toLowerCase(); return t === 'reset' || t === 'button' ? t : 'submit'; }, function (v) { this.setAttribute('type', v); });
  reflect(P.HTMLButtonElement, 'value');
  def(P.HTMLSelectElement, 'options', function () {
    var o = [].slice.call(this.querySelectorAll('option')), sel = this;
    o.add = function (opt, before) { sel.add(opt, before); };
    o.remove = function (i) { sel.remove(i); };
    def(o, 'selectedIndex', function () { return sel.selectedIndex; }, function (v) { sel.selectedIndex = v; });
    return o;
  });
  def(P.HTMLSelectElement, 'length', function () { return this.options.length; });
  def(P.HTMLSelectElement, 'type', function () { return this.multiple ? 'select-multiple' : 'select-one'; });
  reflect(P.HTMLSelectElement, 'multiple', 'multiple', 'bool');
  reflect(P.HTMLSelectElement, 'size', 'size', 'int', 0);
  def(P.HTMLSelectElement, 'selectedIndex', function () {
    var o = this.options;
    for (var i = 0; i < o.length; i++) if (o[i].hasAttribute('selected')) return i;
    return o.length && !this.multiple ? 0 : -1;
  }, function (v) {
    var o = this.options;
    for (var i = 0; i < o.length; i++) { if (i === +v) o[i].setAttribute('selected', ''); else o[i].removeAttribute('selected'); }
  });
  def(P.HTMLSelectElement, 'selectedOptions', function () { return this.options.filter(function (o) { return o.selected; }); });
  def(P.HTMLSelectElement, 'value', function () { var o = this.options[this.selectedIndex]; return o ? o.value : ''; }, function (v) {
    var o = this.options, found = -1;
    for (var i = 0; i < o.length; i++) if (o[i].value === String(v)) { found = i; break; }
    this.selectedIndex = found;
  });
  P.HTMLSelectElement.add = function (opt, before) {
    if (typeof before === 'number') before = this.options[before];
    if (before) before.parentNode.insertBefore(opt, before); else this.appendChild(opt);
  };
  P.HTMLSelectElement.remove = function (i) {
    if (i === undefined) { P.Element.remove.call(this); return; }
    var o = this.options[i]; if (o) o.remove();
  };
  P.HTMLSelectElement.item = function (i) { return this.options[i] || null; };
  def(P.HTMLOptionElement, 'value', function () { var v = this.getAttribute('value'); return v === null ? this.textContent.replace(/\s+/g, ' ').trim() : v; }, function (v) { this.setAttribute('value', v); });
  def(P.HTMLOptionElement, 'label', function () { var v = this.getAttribute('label'); return v === null ? this.text : v; }, function (v) { this.setAttribute('label', v); });
  def(P.HTMLOptionElement, 'selected', function () {
    if (this.hasAttribute('selected')) return true;
    var s = this.closest('select');
    return !!s && !s.multiple && s.selectedIndex === s.options.indexOf(this);
  }, function (v) {
    var s = this.closest('select');
    if (v && s && !s.multiple) s.options.forEach(function (o) { o.removeAttribute('selected'); });
    if (v) this.setAttribute('selected', ''); else this.removeAttribute('selected');
  });
  reflect(P.HTMLOptionElement, 'defaultSelected', 'selected', 'bool');
  reflect(P.HTMLOptionElement, 'disabled', 'disabled', 'bool');
  def(P.HTMLOptionElement, 'index', function () { var s = this.closest('select'); return s ? s.options.indexOf(this) : 0; });
  g.Option = function (text, value, defaultSelected, selected) {
    var o = doc.createElement('option');
    if (text !== undefined) o.textContent = String(text);
    if (value !== undefined) o.setAttribute('value', String(value));
    if (defaultSelected || selected) o.setAttribute('selected', '');
    return o;
  };
  def(P.HTMLFormElement, 'elements', function () { return [].slice.call(this.querySelectorAll('input,select,textarea,button,fieldset,output,object')); });
  def(P.HTMLFormElement, 'length', function () { return this.elements.length; });
  ['action', 'method', 'name', 'target', 'enctype'].forEach(function (p) { reflect(P.HTMLFormElement, p, null, p === 'action' ? 'url' : null); });
  P.HTMLFormElement.submit = function () {};
  P.HTMLFormElement.reset = function () {};
  P.HTMLFormElement.checkValidity = function () { return true; };

  // ---- canvas 2D ----
  function canvasDim(i) {
    return function () { return wk.canvasSize(this)[i]; };
  }
  [['width', 0], ['height', 1]].forEach(function (p) {
    def(P.HTMLCanvasElement, p[0], canvasDim(p[1]), function (v) {
      this.setAttribute(p[0], String(v >>> 0));
      var c = contexts.get(this);
      if (c) c._reset();
      wk.canvasReset(this);
    });
  });
  var contexts = new WeakMap();
  P.HTMLCanvasElement.getContext = function (kind) {
    if (String(kind) !== '2d') return null;
    var c = contexts.get(this);
    if (!c) { c = new CanvasRenderingContext2D(this); contexts.set(this, c); }
    return c;
  };
  P.HTMLCanvasElement.toDataURL = function () {
    return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==';
  };
  P.HTMLCanvasElement.toBlob = function (cb) { if (cb) wk.addTimer(function () { cb(null); }, 0, false, []); };

  function CanvasGradient(kind, a) {
    this.kind = kind; this.stops = [];
    this.x0 = a[0]; this.y0 = a[1];
    if (kind === 'linear') { this.x1 = a[2]; this.y1 = a[3]; this.r0 = this.r1 = 0; }
    else { this.r0 = a[2]; this.x1 = a[3]; this.y1 = a[4]; this.r1 = a[5]; }
  }
  CanvasGradient.prototype.addColorStop = function (o, c) {
    o = +o;
    if (!(o >= 0 && o <= 1)) throw new RangeError('IndexSizeError: offset out of range');
    this.stops.push([o, String(c)]);
    this.stops.sort(function (a, b) { return a[0] - b[0]; });
  };
  g.CanvasGradient = CanvasGradient;
  function CanvasPattern() { this.kind = 'pattern'; this.fallback = 'rgba(128,128,128,0.5)'; }
  CanvasPattern.prototype.setTransform = function () {};
  g.CanvasPattern = CanvasPattern;

  var ARC_STEP = Math.PI / 2;
  function mul(m, n) {
    return [m[0] * n[0] + m[2] * n[1], m[1] * n[0] + m[3] * n[1], m[0] * n[2] + m[2] * n[3],
            m[1] * n[2] + m[3] * n[3], m[0] * n[4] + m[2] * n[5] + m[4], m[1] * n[4] + m[3] * n[5] + m[5]];
  }
  function fin() { for (var i = 0; i < arguments.length; i++) if (!isFinite(arguments[i])) return false; return true; }
  function defaultState() {
    return {
      t: [1, 0, 0, 1, 0, 0], fillStyle: '#000000', strokeStyle: '#000000', lineWidth: 1, lineCap: 'butt',
      lineJoin: 'miter', miterLimit: 10, dash: [], dashOffset: 0, font: '10px sans-serif', textAlign: 'start',
      textBaseline: 'alphabetic', globalAlpha: 1, gco: 'source-over', clips: 0, shadowBlur: 0,
      shadowColor: 'rgba(0, 0, 0, 0)', shadowOffsetX: 0, shadowOffsetY: 0, direction: 'ltr',
      imageSmoothingEnabled: true
    };
  }
  function Path() { this.d = []; this.cx = 0; this.cy = 0; this.sx = 0; this.sy = 0; this.empty = true; }
  function CanvasRenderingContext2D(canvas) {
    this.canvas = canvas;
    this._reset();
  }
  var C2 = CanvasRenderingContext2D.prototype;
  g.CanvasRenderingContext2D = CanvasRenderingContext2D;
  C2._reset = function () { this._s = defaultState(); this._stack = []; this._p = new Path(); };
  // Device-space point of user-space (x, y).
  C2._pt = function (x, y) { var t = this._s.t; return [t[0] * x + t[2] * y + t[4], t[1] * x + t[3] * y + t[5]]; };
  ['fillStyle', 'strokeStyle'].forEach(function (k) {
    def(C2, k, function () { return this._s[k]; }, function (v) {
      if (v instanceof CanvasGradient || v instanceof CanvasPattern) { this._s[k] = v; return; }
      if (typeof v === 'string' || typeof v === 'number') this._s[k] = String(v);
    });
  });
  [['lineWidth', 'lineWidth', 'pos'], ['miterLimit', 'miterLimit', 'pos'], ['globalAlpha', 'globalAlpha', 'alpha'],
   ['lineDashOffset', 'dashOffset', 'num'], ['shadowBlur', 'shadowBlur', 'num'], ['shadowOffsetX', 'shadowOffsetX', 'num'],
   ['shadowOffsetY', 'shadowOffsetY', 'num']].forEach(function (k) {
    def(C2, k[0], function () { return this._s[k[1]]; }, function (v) {
      v = +v;
      if (!isFinite(v)) return;
      if (k[2] === 'pos' && v <= 0) return;
      if (k[2] === 'alpha' && (v < 0 || v > 1)) return;
      this._s[k[1]] = v;
    });
  });
  [['lineCap', /^(butt|round|square)$/], ['lineJoin', /^(miter|round|bevel)$/], ['textAlign', /^(start|end|left|right|center)$/],
   ['textBaseline', /^(top|hanging|middle|alphabetic|ideographic|bottom)$/], ['font', /./], ['shadowColor', /./],
   ['direction', /^(ltr|rtl|inherit)$/]].forEach(function (k) {
    def(C2, k[0], function () { return this._s[k[0]]; }, function (v) { v = String(v); if (k[1].test(v)) this._s[k[0]] = v; });
  });
  def(C2, 'globalCompositeOperation', function () { return this._s.gco; }, function (v) { this._s.gco = String(v); });
  def(C2, 'imageSmoothingEnabled', function () { return this._s.imageSmoothingEnabled; }, function (v) { this._s.imageSmoothingEnabled = !!v; });
  C2.save = function () {
    var s = this._s, c = {};
    for (var k in s) c[k] = s[k];
    c.t = s.t.slice(); c.dash = s.dash.slice(); c.clips = 0;
    this._stack.push(s);
    this._s = c;
  };
  C2.restore = function () {
    if (!this._stack.length) return;
    if (this._s.clips) wk.canvasPopClips(this.canvas, this._s.clips);
    this._s = this._stack.pop();
  };
  C2.reset = function () { while (this._stack.length) this.restore(); this._reset(); wk.canvasClear(this.canvas); };
  C2.setTransform = function (a, b, c, d, e, f) {
    if (typeof a === 'object' && a) { var m = a; a = m.a; b = m.b; c = m.c; d = m.d; e = m.e; f = m.f; }
    if (!fin(a, b, c, d, e, f)) return;
    this._s.t = [+a, +b, +c, +d, +e, +f];
  };
  C2.resetTransform = function () { this._s.t = [1, 0, 0, 1, 0, 0]; };
  C2.transform = function (a, b, c, d, e, f) { if (fin(a, b, c, d, e, f)) this._s.t = mul(this._s.t, [+a, +b, +c, +d, +e, +f]); };
  C2.translate = function (x, y) { this.transform(1, 0, 0, 1, x, y); };
  C2.scale = function (x, y) { this.transform(x, 0, 0, y, 0, 0); };
  C2.rotate = function (r) { var c = Math.cos(r), s = Math.sin(r); this.transform(c, s, -s, c, 0, 0); };
  C2.getTransform = function () { var t = this._s.t; return { a: t[0], b: t[1], c: t[2], d: t[3], e: t[4], f: t[5], is2D: true }; };
  C2.setLineDash = function (a) {
    if (!a || typeof a.length !== 'number') return;
    var d = [];
    for (var i = 0; i < a.length; i++) { var v = +a[i]; if (!isFinite(v) || v < 0) return; d.push(v); }
    if (d.length % 2) d = d.concat(d);
    this._s.dash = d;
  };
  C2.getLineDash = function () { return this._s.dash.slice(); };
  // Paths (stored in device space, like the spec's current default path).
  C2.beginPath = function () { this._p = new Path(); };
  C2._move = function (x, y) {
    var p = this._p, q = this._pt(x, y);
    p.d.push(0, q[0], q[1]); p.cx = p.sx = q[0]; p.cy = p.sy = q[1]; p.empty = false;
  };
  C2._ensure = function (x, y) { if (this._p.empty) this._move(x, y); };
  C2.moveTo = function (x, y) { if (fin(x, y)) this._move(x, y); };
  C2.lineTo = function (x, y) {
    if (!fin(x, y)) return;
    if (this._p.empty) { this._move(x, y); return; }
    var q = this._pt(x, y), p = this._p;
    p.d.push(1, q[0], q[1]); p.cx = q[0]; p.cy = q[1];
  };
  C2.quadraticCurveTo = function (cx, cy, x, y) {
    if (!fin(cx, cy, x, y)) return;
    this._ensure(cx, cy);
    var c = this._pt(cx, cy), q = this._pt(x, y), p = this._p;
    p.d.push(2, c[0], c[1], q[0], q[1]); p.cx = q[0]; p.cy = q[1];
  };
  C2.bezierCurveTo = function (c1x, c1y, c2x, c2y, x, y) {
    if (!fin(c1x, c1y, c2x, c2y, x, y)) return;
    this._ensure(c1x, c1y);
    var a = this._pt(c1x, c1y), b = this._pt(c2x, c2y), q = this._pt(x, y), p = this._p;
    p.d.push(3, a[0], a[1], b[0], b[1], q[0], q[1]); p.cx = q[0]; p.cy = q[1];
  };
  C2.closePath = function () {
    var p = this._p;
    if (p.empty) return;
    p.d.push(4); p.cx = p.sx; p.cy = p.sy;
  };
  C2.rect = function (x, y, w, h) {
    if (!fin(x, y, w, h)) return;
    this._move(x, y); this.lineTo(x + w, y); this.lineTo(x + w, y + h); this.lineTo(x, y + h); this.closePath();
  };
  C2.roundRect = function (x, y, w, h, r) {
    if (!fin(x, y, w, h)) return;
    r = typeof r === 'number' ? r : (r && r.length ? +r[0] : 0);
    r = Math.max(0, Math.min(r || 0, Math.abs(w) / 2, Math.abs(h) / 2));
    this.moveTo(x + r, y); this.arcTo(x + w, y, x + w, y + h, r); this.arcTo(x + w, y + h, x, y + h, r);
    this.arcTo(x, y + h, x, y, r); this.arcTo(x, y, x + w, y, r); this.closePath();
  };
  // Elliptical arc as cubic Béziers in user space (each ≤ 90°).
  C2._ellipse = function (x, y, rx, ry, rot, a0, a1, ccw) {
    var tau = 2 * Math.PI, sweep;
    if (!ccw && a1 - a0 >= tau) sweep = tau;
    else if (ccw && a0 - a1 >= tau) sweep = -tau;
    else {
      sweep = a1 - a0;
      if (!ccw) { sweep = sweep % tau; if (sweep < 0) sweep += tau; }
      else { sweep = sweep % tau; if (sweep > 0) sweep -= tau; }
    }
    var cr = Math.cos(rot), sr = Math.sin(rot), self = this;
    function pt(a) { var px = rx * Math.cos(a), py = ry * Math.sin(a); return [x + px * cr - py * sr, y + px * sr + py * cr]; }
    function d(a) { var px = -rx * Math.sin(a), py = ry * Math.cos(a); return [px * cr - py * sr, px * sr + py * cr]; }
    var start = pt(a0);
    if (this._p.empty) this._move(start[0], start[1]); else this.lineTo(start[0], start[1]);
    var n = Math.max(1, Math.ceil(Math.abs(sweep) / ARC_STEP - 1e-9)), step = sweep / n, a = a0;
    for (var i = 0; i < n; i++) {
      var b = a + step, k = 4 / 3 * Math.tan(step / 4);
      var p0 = pt(a), p3 = pt(b), d0 = d(a), d3 = d(b);
      self.bezierCurveTo(p0[0] + k * d0[0], p0[1] + k * d0[1], p3[0] - k * d3[0], p3[1] - k * d3[1], p3[0], p3[1]);
      a = b;
    }
  };
  C2.arc = function (x, y, r, a0, a1, ccw) {
    if (!fin(x, y, r, a0, a1)) return;
    if (r < 0) throw new RangeError('IndexSizeError: negative radius');
    this._ellipse(x, y, r, r, 0, a0, a1, !!ccw);
  };
  C2.ellipse = function (x, y, rx, ry, rot, a0, a1, ccw) {
    if (!fin(x, y, rx, ry, rot, a0, a1)) return;
    if (rx < 0 || ry < 0) throw new RangeError('IndexSizeError: negative radius');
    this._ellipse(x, y, rx, ry, rot, a0, a1, !!ccw);
  };
  C2.arcTo = function (x1, y1, x2, y2, r) {
    if (!fin(x1, y1, x2, y2, r)) return;
    this._ensure(x1, y1);
    // Current point back in user space.
    var t = this._s.t, det = t[0] * t[3] - t[1] * t[2];
    if (!det) return;
    var px = this._p.cx - t[4], py = this._p.cy - t[5];
    var x0 = (t[3] * px - t[2] * py) / det, y0 = (-t[1] * px + t[0] * py) / det;
    var v1x = x0 - x1, v1y = y0 - y1, v2x = x2 - x1, v2y = y2 - y1;
    var l1 = Math.sqrt(v1x * v1x + v1y * v1y), l2 = Math.sqrt(v2x * v2x + v2y * v2y);
    var cross = v1x * v2y - v1y * v2x;
    if (!r || !l1 || !l2 || Math.abs(cross) < 1e-12) { this.lineTo(x1, y1); return; }
    var ang = Math.acos(Math.max(-1, Math.min(1, (v1x * v2x + v1y * v2y) / (l1 * l2))));
    var dist = r / Math.tan(ang / 2);
    var t1x = x1 + v1x / l1 * dist, t1y = y1 + v1y / l1 * dist;
    var t2x = x1 + v2x / l2 * dist, t2y = y1 + v2y / l2 * dist;
    var ccw = cross > 0;
    var nx = ccw ? v1y / l1 : -v1y / l1, ny = ccw ? -v1x / l1 : v1x / l1;
    var cx = t1x - nx * r, cy = t1y - ny * r;
    this.lineTo(t1x, t1y);
    var a0 = Math.atan2(t1y - cy, t1x - cx), a1 = Math.atan2(t2y - cy, t2x - cx);
    this._ellipse(cx, cy, r, r, 0, a0, a1, ccw);
  };
  function pathArg(ctx, p) { return p instanceof Path2D ? p._data(ctx) : ctx._p.d; }
  function isEvenOdd(a) { return a === 'evenodd'; }
  C2._alpha = function () { return this._s.globalAlpha; };
  C2.fill = function (a, b) {
    var path = a instanceof Path2D ? a._data(this) : this._p.d, rule = a instanceof Path2D ? b : a;
    if (!path.length) return;
    wk.canvasFill.apply(null, [this.canvas, path, this._s.fillStyle, isEvenOdd(rule), this._alpha()].concat(this._s.t));
  };
  C2.stroke = function (a) {
    var path = a instanceof Path2D ? a._data(this) : this._p.d, s = this._s;
    if (!path.length) return;
    wk.canvasStroke.apply(null, [this.canvas, path, s.strokeStyle, this._alpha(), s.lineWidth, s.lineCap, s.lineJoin,
      s.miterLimit, s.dash, s.dashOffset].concat(s.t));
  };
  C2.clip = function (a, b) {
    var path = a instanceof Path2D ? a._data(this) : this._p.d, rule = a instanceof Path2D ? b : a;
    wk.canvasClip(this.canvas, path.length ? path : [0, 0, 0], isEvenOdd(rule));
    this._s.clips++;
  };
  C2._rectPath = function (x, y, w, h) {
    var a = this._pt(x, y), b = this._pt(x + w, y), c = this._pt(x + w, y + h), d = this._pt(x, y + h);
    return [0, a[0], a[1], 1, b[0], b[1], 1, c[0], c[1], 1, d[0], d[1], 4];
  };
  C2.fillRect = function (x, y, w, h) {
    if (!fin(x, y, w, h) || !w || !h) return;
    wk.canvasFill.apply(null, [this.canvas, this._rectPath(x, y, w, h), this._s.fillStyle, false, this._alpha()].concat(this._s.t));
  };
  C2.strokeRect = function (x, y, w, h) {
    if (!fin(x, y, w, h)) return;
    var s = this._s;
    wk.canvasStroke.apply(null, [this.canvas, this._rectPath(x, y, w, h), s.strokeStyle, this._alpha(), s.lineWidth, s.lineCap,
      s.lineJoin, s.miterLimit, s.dash, s.dashOffset].concat(s.t));
  };
  C2.clearRect = function (x, y, w, h) {
    if (!fin(x, y, w, h)) return;
    // Vector recording: only a clear covering the whole canvas erases.
    var sz = wk.canvasSize(this.canvas), p = this._rectPath(x, y, w, h);
    var xs = [p[1], p[4], p[7], p[10]], ys = [p[2], p[5], p[8], p[11]];
    if (Math.min.apply(null, xs) <= 0 && Math.min.apply(null, ys) <= 0 &&
        Math.max.apply(null, xs) >= sz[0] && Math.max.apply(null, ys) >= sz[1]) wk.canvasClear(this.canvas);
  };
  C2._text = function (t, x, y, mw, stroke) {
    if (!fin(x, y)) return;
    var s = this._s;
    wk.canvasText.apply(null, [this.canvas, String(t), +x, +y, s.font, s.textAlign, s.textBaseline,
      mw === undefined ? undefined : +mw, stroke ? s.strokeStyle : s.fillStyle, this._alpha()].concat(s.t, [!!stroke, s.lineWidth, s.direction === 'rtl']));
  };
  C2.fillText = function (t, x, y, mw) { this._text(t, x, y, mw, false); };
  C2.strokeText = function (t, x, y, mw) { this._text(t, x, y, mw, true); };
  C2.measureText = function (t) { return wk.measureText(this._s.font, String(t)); };
  C2.drawImage = function (img) {
    var a = [].slice.call(arguments, 1), sw, sh, sx = 0, sy = 0, dx, dy, dw, dh, nat;
    if (!img || (img.localName !== 'img' && img.localName !== 'canvas')) return;
    nat = img.localName === 'canvas' ? wk.canvasSize(img) : wk.imageSize(img);
    sw = nat[0]; sh = nat[1];
    if (a.length === 2) { dx = a[0]; dy = a[1]; dw = sw; dh = sh; }
    else if (a.length === 4) { dx = a[0]; dy = a[1]; dw = a[2]; dh = a[3]; }
    else if (a.length >= 8) { sx = a[0]; sy = a[1]; sw = a[2]; sh = a[3]; dx = a[4]; dy = a[5]; dw = a[6]; dh = a[7]; }
    else return;
    if (!fin(sx, sy, sw, sh, dx, dy, dw, dh)) return;
    wk.canvasImage.apply(null, [this.canvas, img, sx, sy, sw, sh, dx, dy, dw, dh, this._alpha()].concat(this._s.t));
  };
  C2.createLinearGradient = function (x0, y0, x1, y1) { return new CanvasGradient('linear', [x0, y0, x1, y1]); };
  C2.createRadialGradient = function (x0, y0, r0, x1, y1, r1) { return new CanvasGradient('radial', [x0, y0, r0, x1, y1, r1]); };
  C2.createConicGradient = function () { return new CanvasPattern(); };
  C2.createPattern = function () { return new CanvasPattern(); };
  C2.isPointInPath = function () { return false; };
  C2.isPointInStroke = function () { return false; };
  C2.getImageData = function (x, y, w, h) { return this.createImageData(w, h); };
  C2.createImageData = function (w, h) {
    if (typeof w === 'object') { h = w.height; w = w.width; }
    w = Math.abs(w | 0); h = Math.abs(h | 0);
    return { width: w, height: h, data: new Uint8ClampedArray(w * h * 4) };
  };
  C2.putImageData = function () {};
  C2.drawFocusIfNeeded = function () {};
  C2.getContextAttributes = function () { return { alpha: true }; };
  // Path2D: records calls, replayed into the context's path machinery.
  function Path2D(src) {
    this._ops = [];
    if (src instanceof Path2D) this._ops = src._ops.slice();
  }
  ['moveTo', 'lineTo', 'quadraticCurveTo', 'bezierCurveTo', 'arc', 'arcTo', 'ellipse', 'rect', 'roundRect', 'closePath'].forEach(function (m) {
    Path2D.prototype[m] = function () { this._ops.push([m, [].slice.call(arguments)]); };
  });
  Path2D.prototype.addPath = function (p) { if (p instanceof Path2D) this._ops = this._ops.concat(p._ops); };
  Path2D.prototype._data = function (ctx) {
    var saved = ctx._p;
    ctx._p = new Path();
    this._ops.forEach(function (o) { C2[o[0]].apply(ctx, o[1]); });
    var d = ctx._p.d;
    ctx._p = saved;
    return d;
  };
  g.Path2D = Path2D;

  // ---- images ----
  g.Image = function (w, h) {
    var i = doc.createElement('img');
    if (w !== undefined) i.setAttribute('width', String(w));
    if (h !== undefined) i.setAttribute('height', String(h));
    return i;
  };

  // ---- URL parsing (location, anchors) ----
  function parseUrl(href) {
    var m = /^([a-z][a-z0-9+.\-]*:)(?:\/\/([^\/?#:]*)(?::(\d+))?)?([^?#]*)(\?[^#]*)?(#.*)?$/i.exec(href) || [];
    var u = {
      href: href, protocol: m[1] || '', hostname: m[2] || '', port: m[3] || '',
      pathname: m[4] || '', search: m[5] || '', hash: m[6] || ''
    };
    u.host = u.hostname + (u.port ? ':' + u.port : '');
    u.origin = u.protocol === 'file:' ? 'null' : u.protocol + '//' + u.host;
    return u;
  }
  var loc = parseUrl(wk.url);
  var location = {
    toString: function () { return this.href; },
    reload: function () {}, assign: function () {}, replace: function () {}
  };
  for (var k in loc) location[k] = loc[k];
  g.location = location;
  def(P.Document, 'location', function () { return location; }, function () {});
  def(P.Document, 'URL', function () { return wk.url; });
  def(P.Document, 'documentURI', function () { return wk.url; });
  def(P.Document, 'baseURI', function () { return wk.url; });
  def(P.Document, 'domain', function () { return loc.hostname; });
  def(P.Document, 'referrer', function () { return ''; });
  def(P.Document, 'cookie', function () { return ''; }, function () {});
  def(P.Document, 'characterSet', function () { return wk.charset || 'UTF-8'; });
  def(P.Document, 'charset', function () { return this.characterSet; });
  def(P.Document, 'inputEncoding', function () { return this.characterSet; });
  def(P.Document, 'contentType', function () { return 'text/html'; });
  def(P.Document, 'defaultView', function () { return g; });
  def(P.Document, 'readyState', function () { return wk.readyState; });
  def(P.Document, 'compatMode', function () { return wk.quirks ? 'BackCompat' : 'CSS1Compat'; });
  def(P.Document, 'doctype', function () { return null; });
  def(P.Document, 'activeElement', function () { return this.body; });
  def(P.Document, 'scrollingElement', function () { return this.documentElement; });
  def(P.Document, 'hidden', function () { return false; });
  def(P.Document, 'visibilityState', function () { return 'visible'; });
  def(P.Document, 'currentScript', function () { return wk.currentScript(); });
  ['forms', 'images', 'links', 'scripts', 'anchors', 'embeds', 'plugins'].forEach(function (n) {
    var sel = { forms: 'form', images: 'img', links: 'a[href],area[href]', scripts: 'script', anchors: 'a[name]', embeds: 'embed', plugins: 'embed' }[n];
    def(P.Document, n, function () { return [].slice.call(this.querySelectorAll(sel)); });
  });
  def(P.Document, 'styleSheets', function () { return []; });
  def(P.Document, 'all', function () { return [].slice.call(this.getElementsByTagName('*')); });
  P.Document.hasFocus = function () { return true; };
  P.Document.execCommand = function () { return false; };
  P.Document.queryCommandSupported = function () { return false; };
  P.Document.getSelection = function () { return g.getSelection(); };
  P.Document.elementFromPoint = function () { return null; };
  P.Document.open = function () { return this; };
  P.Document.close = function () {};
  P.Document.createRange = function () {
    var r = {
      collapsed: true, commonAncestorContainer: doc, startContainer: doc, endContainer: doc, startOffset: 0, endOffset: 0,
      setStart: function (n, o) { this.startContainer = n; this.startOffset = o; },
      setEnd: function (n, o) { this.endContainer = n; this.endOffset = o; },
      setStartBefore: function () {}, setStartAfter: function () {}, setEndBefore: function () {}, setEndAfter: function () {},
      selectNode: function () {}, selectNodeContents: function () {}, collapse: function () {}, detach: function () {},
      cloneRange: function () { return doc.createRange(); },
      getBoundingClientRect: function () { return { x: 0, y: 0, left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 }; },
      getClientRects: function () { return []; },
      createContextualFragment: function (html) {
        var f = doc.createDocumentFragment(), d = doc.createElement('div');
        d.innerHTML = html;
        while (d.firstChild) f.appendChild(d.firstChild);
        return f;
      },
      deleteContents: function () {}, extractContents: function () { return doc.createDocumentFragment(); },
      toString: function () { return ''; }
    };
    return r;
  };
  P.Document.createTreeWalker = P.Document.createNodeIterator = function (root, what, filter) {
    var nodes = [], i = -1;
    (function walk(n) { for (var c = n.firstChild; c; c = c.nextSibling) { nodes.push(c); walk(c); } })(root);
    var show = what === undefined ? 0xFFFFFFFF : what;
    nodes = nodes.filter(function (n) {
      if (!(show & (1 << (n.nodeType - 1)))) return false;
      if (!filter) return true;
      var f = typeof filter === 'function' ? filter : filter.acceptNode;
      return f.call(filter, n) === 1;
    });
    return {
      root: root, currentNode: root,
      nextNode: function () { i++; return (this.currentNode = nodes[i] || null) || (i = nodes.length, null); },
      previousNode: function () { i--; return i >= 0 ? (this.currentNode = nodes[i]) : (i = -1, null); },
      detach: function () {}
    };
  };
  g.NodeFilter = { FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3, SHOW_ALL: 0xFFFFFFFF, SHOW_ELEMENT: 1, SHOW_TEXT: 4, SHOW_COMMENT: 128 };

  // document.implementation: blitz-dom has one arena per document, so an
  // inert second document is emulated with detached elements of this one.
  var implementation = {
    hasFeature: function () { return true; },
    createHTMLDocument: function (title) {
      var d = Object.create(doc);
      var html = doc.createElement('html'), head = doc.createElement('head'), body = doc.createElement('body');
      html.appendChild(head); html.appendChild(body);
      if (title !== undefined) { var t = doc.createElement('title'); t.textContent = String(title); head.appendChild(t); }
      val(d, 'documentElement', html);
      def(d, 'head', function () { return head; });
      def(d, 'body', function () { return body; });
      val(d, 'getElementById', function (id) { return html.querySelector('#' + String(id).replace(/([^\w-])/g, '\\$1')); });
      val(d, 'querySelector', function (s) { return html.querySelector(s); });
      val(d, 'querySelectorAll', function (s) { return html.querySelectorAll(s); });
      val(d, 'getElementsByTagName', function (s) { return html.getElementsByTagName(s); });
      val(d, 'getElementsByClassName', function (s) { return html.getElementsByClassName(s); });
      val(d, 'createElement', function (t) { return doc.createElement(t); });
      return d;
    },
    createDocument: function () { return implementation.createHTMLDocument(''); },
    createDocumentType: function (n) { return { name: n, nodeType: 10 }; }
  };
  def(P.Document, 'implementation', function () { return implementation; });
  g.DOMParser = function () {};
  g.DOMParser.prototype.parseFromString = function (s) {
    var d = implementation.createHTMLDocument('');
    var m = /<body[^>]*>([\s\S]*)<\/body>/i.exec(s);
    d.body.innerHTML = m ? m[1] : String(s);
    return d;
  };
  g.XMLSerializer = function () {};
  g.XMLSerializer.prototype.serializeToString = function (n) { return n.outerHTML || n.textContent || ''; };

  // ---- window ----
  g.window = g; g.self = g; g.top = g; g.parent = g; g.frames = g;
  g.length = 0; g.opener = null; g.closed = false; g.name = '';
  g.frameElement = null;
  g.status = ''; g.defaultStatus = '';
  g.devicePixelRatio = 1;
  // QtWebKit lays the page out in a 0x0 viewport during load (verified).
  g.innerWidth = 0; g.innerHeight = 0; g.outerWidth = 0; g.outerHeight = 0;
  g.pageXOffset = g.pageYOffset = g.scrollX = g.scrollY = 0;
  g.screenX = g.screenY = g.screenLeft = g.screenTop = 0;
  g.navigator = {
    userAgent: wk.userAgent,
    appCodeName: 'Mozilla', appName: 'Netscape', appVersion: wk.userAgent.replace(/^Mozilla\//, ''),
    platform: 'Linux x86_64', product: 'Gecko', productSub: '20030107', vendor: 'Apple Computer, Inc.', vendorSub: '',
    language: 'C', languages: ['C'], cookieEnabled: true, onLine: true, doNotTrack: null,
    javaEnabled: function () { return false; }, plugins: [], mimeTypes: [], maxTouchPoints: 0, hardwareConcurrency: 1
  };
  g.screen = {
    width: wk.viewportWidth, height: wk.viewportHeight, availWidth: wk.viewportWidth, availHeight: wk.viewportHeight,
    availLeft: 0, availTop: 0, colorDepth: 24, pixelDepth: 24
  };
  g.history = { length: 1, state: null, pushState: function () {}, replaceState: function () {}, back: function () {}, forward: function () {}, go: function () {} };
  g.setTimeout = function (fn, ms) { return wk.addTimer(fn, +ms || 0, false, [].slice.call(arguments, 2)); };
  g.setInterval = function (fn, ms) { return wk.addTimer(fn, +ms || 0, true, [].slice.call(arguments, 2)); };
  g.clearTimeout = g.clearInterval = function (id) { wk.clearTimer(+id || 0); };
  g.setImmediate = function (fn) { return wk.addTimer(fn, 0, false, [].slice.call(arguments, 1)); };
  g.clearImmediate = g.clearTimeout;
  g.queueMicrotask = function (fn) { Promise.resolve().then(fn); };
  g.print = function () { wk.print(); };
  g.alert = function (m) { wk.dialog('alert', arguments.length ? String(m) : '', ''); };
  g.confirm = function (m) { wk.dialog('confirm', arguments.length ? String(m) : '', ''); return true; };
  g.prompt = function (m, d) { wk.dialog('prompt', arguments.length ? String(m) : '', d === undefined ? '' : String(d)); return ''; };
  g.open = function () { return null; };
  g.close = g.stop = g.focus = g.blur = g.moveTo = g.moveBy = g.resizeTo = g.resizeBy = g.scroll = g.scrollTo = g.scrollBy = function () {};
  g.postMessage = function () {};
  g.getSelection = function () {
    return { rangeCount: 0, isCollapsed: true, addRange: function () {}, removeAllRanges: function () {}, getRangeAt: function () { return doc.createRange(); }, toString: function () { return ''; } };
  };
  g.matchMedia = function (q) {
    return { matches: wk.mediaMatches(String(q)), media: String(q), onchange: null, addListener: function () {}, removeListener: function () {}, addEventListener: function () {}, removeEventListener: function () {} };
  };
  g.performance = { now: function () { return wk.now(); }, timing: { navigationStart: Date.now() - wk.now() }, mark: function () {}, measure: function () {}, getEntriesByName: function () { return []; } };
  g.requestAnimationFrame = function (fn) { return wk.addTimer(function () { fn(wk.now()); }, 16, false, []); };
  g.cancelAnimationFrame = g.clearTimeout;
  function fmt(a) { return a.length ? String(a[0]) : ''; }
  var console = {};
  ['log', 'info', 'warn', 'error', 'debug', 'trace', 'dir', 'dirxml', 'table', 'assert', 'count', 'group', 'groupCollapsed', 'groupEnd', 'time', 'timeEnd', 'timeLog', 'clear', 'profile', 'profileEnd'].forEach(function (k) {
    console[k] = function () { if (k === 'assert') { if (arguments[0]) return; wk.console('Assertion failed: ' + fmt([].slice.call(arguments, 1))); return; } if (/^(log|info|warn|error|debug|trace|dir|dirxml|table)$/.test(k)) wk.console(fmt(arguments)); };
  });
  g.console = console;
  // Storage (in memory).
  function Storage() { var d = {}; var s = this;
    val(s, 'getItem', function (k) { k = String(k); return hasOwn.call(d, k) ? d[k] : null; });
    val(s, 'setItem', function (k, v) { d[String(k)] = String(v); });
    val(s, 'removeItem', function (k) { delete d[String(k)]; });
    val(s, 'clear', function () { d = {}; });
    val(s, 'key', function (i) { return Object.keys(d)[i] || null; });
    def(s, 'length', function () { return Object.keys(d).length; });
  }
  g.Storage = Storage;
  g.localStorage = new Storage(); g.sessionStorage = new Storage();
  // base64
  var B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  g.btoa = function (s) {
    s = String(s);
    var out = '', i = 0;
    for (var j = 0; j < s.length; j++) if (s.charCodeAt(j) > 255) throw new Error('InvalidCharacterError: btoa');
    while (i < s.length) {
      var a = s.charCodeAt(i++), b = s.charCodeAt(i++), c = s.charCodeAt(i++);
      var n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64.charAt(n >> 18 & 63) + B64.charAt(n >> 12 & 63) + (isNaN(b) ? '=' : B64.charAt(n >> 6 & 63)) + (isNaN(c) ? '=' : B64.charAt(n & 63));
    }
    return out;
  };
  g.atob = function (s) {
    s = String(s).replace(/[\t\n\f\r ]/g, '').replace(/=+$/, '');
    var out = '', bits = 0, n = 0;
    for (var i = 0; i < s.length; i++) {
      var v = B64.indexOf(s.charAt(i));
      if (v < 0) throw new Error('InvalidCharacterError: atob');
      n = (n << 6) | v; bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((n >> bits) & 255); }
    }
    return out;
  };

  // ---- XMLHttpRequest over the loader ----
  function XMLHttpRequest() {
    this.readyState = 0; this.status = 0; this.statusText = ''; this.responseText = ''; this.response = '';
    this.responseXML = null; this.responseType = ''; this.timeout = 0; this.withCredentials = false;
    this.onreadystatechange = null; this.onload = null; this.onerror = null; this.onloadend = null;
    this._headers = {};
  }
  XMLHttpRequest.UNSENT = 0; XMLHttpRequest.OPENED = 1; XMLHttpRequest.HEADERS_RECEIVED = 2; XMLHttpRequest.LOADING = 3; XMLHttpRequest.DONE = 4;
  var X = XMLHttpRequest.prototype;
  X.open = function (method, url, async) {
    this._method = String(method).toUpperCase(); this._url = String(url); this._async = async !== false;
    this.readyState = 1; this._fire('readystatechange');
  };
  X.setRequestHeader = function (k, v) { this._headers[k] = v; };
  X.overrideMimeType = function () {};
  X.getResponseHeader = function (k) { return /content-type/i.test(k) && this.readyState === 4 ? 'text/plain' : null; };
  X.getAllResponseHeaders = function () { return ''; };
  X.abort = function () { this._aborted = true; };
  X._fire = function (type) {
    var ev = new Event(type);
    ev.target = this;
    var h = this['on' + type];
    try { if (typeof h === 'function') h.call(this, ev); } catch (e) { throwLater(e); }
    var map = listeners.get(this), list = map && hasOwn.call(map, type) && map[type];
    if (list) list.slice().forEach(function (l) { try { l.fn.call(this, ev); } catch (e) { throwLater(e); } }, this);
  };
  X.addEventListener = addEventListener;
  X.removeEventListener = removeEventListener;
  X.send = function () {
    var self = this;
    function done() {
      if (self._aborted) return;
      var r = wk.fetch(self._url);
      var file = /^file:/i.test(wk.resolve(self._url));
      self.status = r[0] ? (file ? 0 : 200) : 0;
      self.statusText = r[0] && !file ? 'OK' : '';
      self.responseText = r[1];
      self.response = self.responseType === 'json' ? (function () { try { return JSON.parse(r[1]); } catch (e) { return null; } })() : r[1];
      self.readyState = 4;
      self._fire('readystatechange');
      self._fire(r[0] ? 'load' : 'error');
      self._fire('loadend');
      return r[0];
    }
    if (this._async) wk.networkTask(done);
    else if (!done()) throw new Error('NETWORK_ERR: XMLHttpRequest Exception 101');
  };
  g.XMLHttpRequest = XMLHttpRequest;
  g.FormData = function () { this._d = []; };
  g.FormData.prototype.append = function (k, v) { this._d.push([k, v]); };

  // ---- C-locale formatting, like QtWebKit under LC_ALL=C ----
  function pad(n) { return (n < 10 ? '0' : '') + n; }
  var DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  var MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  Date.prototype.toLocaleDateString = function () {
    if (isNaN(this.getTime())) return 'Invalid Date';
    return pad(this.getMonth() + 1) + '/' + pad(this.getDate()) + '/' + this.getFullYear();
  };
  Date.prototype.toLocaleTimeString = function () {
    if (isNaN(this.getTime())) return 'Invalid Date';
    return pad(this.getHours()) + ':' + pad(this.getMinutes()) + ':' + pad(this.getSeconds());
  };
  Date.prototype.toLocaleString = function () {
    if (isNaN(this.getTime())) return 'Invalid Date';
    return DAYS[this.getDay()] + ' ' + MONTHS[this.getMonth()] + ' ' + pad(this.getDate()) + ' ' + this.toLocaleTimeString() + ' ' + this.getFullYear();
  };
  Number.prototype.toLocaleString = function () { return String(+this); };
  var dateToString = Date.prototype.toString;
  Date.prototype.toString = function () {
    var r = dateToString.call(this);
    return / GMT\+0000$/.test(r) ? r + ' (UTC)' : r;
  };

  // Elements with an id are reachable as globals (named access on window).
  var namedRe = /^[A-Za-z_$][\w$]*$/;
  wk.syncIds = function () {
    var els = doc.querySelectorAll('[id]');
    for (var i = 0; i < els.length; i++) {
      var id = els[i].getAttribute('id');
      if (!namedRe.test(id) || id in g) continue;
      (function (id) {
        Object.defineProperty(g, id, {
          configurable: true, enumerable: false,
          get: function () { return doc.getElementById(id) || undefined; },
          set: function (v) { Object.defineProperty(g, id, { value: v, writable: true, configurable: true, enumerable: true }); }
        });
      })(id);
    }
  };

  ['HTMLCollection', 'NodeList', 'NamedNodeMap', 'DOMTokenList', 'CSSStyleDeclaration', 'StyleSheet', 'CSSStyleSheet', 'Selection', 'Range'].forEach(function (n) {
    var C = function () { illegal(); };
    Object.defineProperty(C, 'name', { value: n });
    g[n] = C;
  });
})(this);
