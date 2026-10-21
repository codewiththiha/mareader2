// The page a book's scripts see: a small DOM, events and timers.
(function () {
  'use strict';
  const G = globalThis;
  const VOID = new Set(['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input',
    'link', 'meta', 'param', 'source', 'track', 'wbr']);
  const RAW = new Set(['script', 'style']);
  const CLICKS = ['click', 'mousedown', 'mouseup', 'pointerdown', 'pointerup',
    'touchstart', 'touchend', 'dblclick'];
  const BLOCKS = new Set(['address', 'article', 'aside', 'blockquote', 'body', 'dd',
    'details', 'div', 'dl', 'dt', 'fieldset', 'figcaption', 'figure', 'footer', 'form',
    'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'header', 'hr', 'html', 'li', 'main', 'nav',
    'ol', 'p', 'pre', 'section', 'summary', 'table', 'ul']);
  const MAX_TIMERS = 1000;
  const MAX_ALERTS = 20;

  let dirty = true;
  let nextId = 1;
  let alerts = [];
  let errors = [];
  let timers = [];
  let timerSeq = 1;
  let now = 0;
  let sheets = [];
  let rules = null;
  const nodes = new Map();
  const touch = () => { dirty = true; };
  const report = (e) => { if (errors.length < 20) errors.push(String(e && e.stack || e)); };

  // ---- Events --------------------------------------------------------

  class Event_ {
    constructor(type, init) {
      init = init || {};
      this.type = String(type);
      this.bubbles = !!init.bubbles;
      this.cancelable = !!init.cancelable;
      this.composed = !!init.composed;
      this.defaultPrevented = false;
      this.target = null;
      this.currentTarget = null;
      this.eventPhase = 0;
      this.isTrusted = false;
      this.timeStamp = now;
      this.detail = init.detail === undefined ? null : init.detail;
      this.__stop = false;
      this.__now = false;
    }
    preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
    stopPropagation() { this.__stop = true; }
    stopImmediatePropagation() { this.__stop = true; this.__now = true; }
    get srcElement() { return this.target; }
    get returnValue() { return !this.defaultPrevented; }
    set returnValue(v) { if (!v) this.preventDefault(); }
    composedPath() { return pathOf(this.target); }
  }
  class MouseEvent_ extends Event_ {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.button = init.button || 0;
      this.buttons = 1;
      this.clientX = this.clientY = this.pageX = this.pageY = 0;
      this.screenX = this.screenY = this.offsetX = this.offsetY = 0;
      this.altKey = this.ctrlKey = this.metaKey = this.shiftKey = false;
      this.detail = 1;
    }
  }
  class KeyboardEvent_ extends Event_ {
    constructor(type, init) {
      super(type, init);
      init = init || {};
      this.key = init.key || '';
      this.code = init.code || '';
      this.keyCode = init.keyCode || 0;
    }
  }
  class CustomEvent_ extends Event_ {}

  class Target_ {
    addEventListener(type, f, o) {
      if (!f) return;
      if (!this.__l) this.__l = new Map();
      let all = this.__l.get(type);
      if (!all) { all = []; this.__l.set(type, all); }
      const capture = o === true || !!(o && o.capture);
      if (all.some((l) => l.f === f && l.c === capture)) return;
      all.push({ f: f, c: capture, once: !!(o && o.once) });
      touch();
    }
    removeEventListener(type, f, o) {
      const all = this.__l && this.__l.get(type);
      if (!all) return;
      const capture = o === true || !!(o && o.capture);
      const i = all.findIndex((l) => l.f === f && l.c === capture);
      if (i >= 0) { all.splice(i, 1); touch(); }
    }
    dispatchEvent(e) { return dispatch(this, e); }
  }

  const win = new Target_();
  function pathOf(target) {
    const path = [];
    for (let x = target; x; x = x.parentNode) path.push(x);
    if (path.length && path[path.length - 1] === doc) path.push(win);
    return path;
  }
  function call(f, node, e) {
    try {
      if (typeof f === 'function') return f.call(node === win ? G : node, e);
      if (f && typeof f.handleEvent === 'function') return f.handleEvent(e);
    } catch (err) { report(err); }
    return undefined;
  }
  const compiled = new Map();
  // `onclick` set by a script: kept aside on elements, plain elsewhere.
  function ownHandler(node, type) {
    if (node === win) return G['on' + type];
    return node.nodeType === 1 ? node['__on' + type] : node['on' + type];
  }
  function handlerOf(node, type) {
    const own = ownHandler(node, type);
    if (typeof own === 'function') return own;
    if (node.nodeType !== 1) return null;
    const code = node.getAttribute('on' + type);
    if (code == null) return null;
    let f = compiled.get(code);
    if (!f) {
      try { f = new Function('event', code); } catch (err) { report(err); f = () => {}; }
      compiled.set(code, f);
    }
    return f;
  }
  function fire(node, e, capture) {
    e.currentTarget = node;
    const all = node.__l && node.__l.get(e.type);
    if (all) {
      for (const l of all.slice()) {
        if (l.c !== capture && e.eventPhase !== 2) continue;
        if (l.once) node.removeEventListener(e.type, l.f, l.c);
        call(l.f, node, e);
        if (e.__now) return;
      }
    }
    if (!capture || e.eventPhase === 2) {
      const h = handlerOf(node, e.type);
      if (h && call(h, node, e) === false) e.preventDefault();
    }
  }
  function dispatch(target, e) {
    if (!(e instanceof Event_)) e = new Event_(String(e && e.type || e));
    e.target = target;
    const path = pathOf(target);
    e.eventPhase = 1;
    for (let i = path.length - 1; i > 0 && !e.__stop; i--) fire(path[i], e, true);
    if (!e.__stop) { e.eventPhase = 2; fire(target, e, false); }
    if (e.bubbles) {
      e.eventPhase = 3;
      for (let i = 1; i < path.length && !e.__stop; i++) fire(path[i], e, false);
    }
    e.eventPhase = 0;
    e.currentTarget = null;
    return !e.defaultPrevented;
  }

  // ---- Nodes ---------------------------------------------------------

  const list = (a) => { a.item = (i) => a[i] || null; return a; };

  class Node_ extends Target_ {
    constructor(type) {
      super();
      this.nodeType = type;
      this.parentNode = null;
      this.childNodes = list([]);
    }
    get ownerDocument() { return this === doc ? null : doc; }
    get firstChild() { return this.childNodes[0] || null; }
    get lastChild() { return this.childNodes[this.childNodes.length - 1] || null; }
    sibling(step, elements) {
      const p = this.parentNode;
      if (!p) return null;
      const kids = p.childNodes;
      for (let i = kids.indexOf(this) + step; i >= 0 && i < kids.length; i += step) {
        if (!elements || kids[i].nodeType === 1) return kids[i];
      }
      return null;
    }
    get nextSibling() { return this.sibling(1, false); }
    get previousSibling() { return this.sibling(-1, false); }
    get nextElementSibling() { return this.sibling(1, true); }
    get previousElementSibling() { return this.sibling(-1, true); }
    get parentElement() { const p = this.parentNode; return p && p.nodeType === 1 ? p : null; }
    get children() { return list(this.childNodes.filter((n) => n.nodeType === 1)); }
    get childElementCount() { return this.children.length; }
    get firstElementChild() { return this.children[0] || null; }
    get lastElementChild() { const c = this.children; return c[c.length - 1] || null; }
    get isConnected() { let x = this; while (x.parentNode) x = x.parentNode; return x === doc; }
    get nodeValue() { return this.nodeType === 3 || this.nodeType === 8 ? this.data : null; }
    set nodeValue(v) { if (this.nodeType === 3 || this.nodeType === 8) this.data = v; }
    hasChildNodes() { return this.childNodes.length > 0; }
    getRootNode() { let x = this; while (x.parentNode) x = x.parentNode; return x; }
    contains(n) { for (let x = n; x; x = x.parentNode) if (x === this) return true; return false; }
    get textContent() {
      if (this.nodeType === 3) return this.data;
      if (this.nodeType === 8) return '';
      return this.childNodes.map((c) => c.textContent).join('');
    }
    set textContent(v) {
      if (this.nodeType === 3 || this.nodeType === 8) { this.data = v; return; }
      for (const c of this.childNodes) c.parentNode = null;
      this.childNodes = list([]);
      const s = v == null ? '' : String(v);
      if (s) this.appendChild(new Text_(s));
      touch();
    }
    appendChild(n) { return this.insertBefore(n, null); }
    insertBefore(n, ref) {
      if (!(n instanceof Node_)) throw new TypeError('not a node');
      if (n.nodeType === 11) {
        for (const c of n.childNodes.slice()) this.insertBefore(c, ref);
        return n;
      }
      if (n.contains(this)) throw new Error('HierarchyRequestError');
      if (n.parentNode) n.parentNode.removeChild(n);
      const i = ref ? this.childNodes.indexOf(ref) : -1;
      if (i < 0) this.childNodes.push(n); else this.childNodes.splice(i, 0, n);
      n.parentNode = this;
      touch();
      return n;
    }
    removeChild(n) {
      const i = this.childNodes.indexOf(n);
      if (i < 0) throw new Error('NotFoundError');
      this.childNodes.splice(i, 1);
      n.parentNode = null;
      touch();
      return n;
    }
    replaceChild(n, old) { this.insertBefore(n, old); return this.removeChild(old); }
    remove() { if (this.parentNode) this.parentNode.removeChild(this); }
    append(...ns) { for (const n of ns) this.appendChild(asNode(n)); }
    prepend(...ns) { const f = this.firstChild; for (const n of ns) this.insertBefore(asNode(n), f); }
    before(...ns) { const p = this.parentNode; if (p) for (const n of ns) p.insertBefore(asNode(n), this); }
    after(...ns) {
      const p = this.parentNode;
      if (!p) return;
      const next = this.nextSibling;
      for (const n of ns) p.insertBefore(asNode(n), next);
    }
    replaceWith(...ns) { this.before(...ns); this.remove(); }
    replaceChildren(...ns) { this.textContent = ''; this.append(...ns); }
    cloneNode(deep) {
      let copy;
      if (this.nodeType === 3) copy = new Text_(this.data);
      else if (this.nodeType === 8) copy = new Comment_(this.data);
      else if (this.nodeType === 11) copy = new Fragment_();
      else {
        copy = new Element_(this.localName);
        for (const [k, v] of this.__a) copy.__a.set(k, v);
      }
      if (deep) for (const c of this.childNodes) copy.appendChild(c.cloneNode(true));
      return copy;
    }
    querySelector(s) { return this.querySelectorAll(s)[0] || null; }
    querySelectorAll(s) {
      const sel = parseSelector(s);
      return list(descendants(this).filter((el) => matches(el, sel)));
    }
    getElementsByTagName(t) {
      const n = String(t).toLowerCase();
      return list(descendants(this).filter((el) => n === '*' || el.localName === n));
    }
    getElementsByClassName(c) {
      const want = String(c).split(/\s+/).filter(Boolean);
      return list(descendants(this).filter((el) => want.every((w) => el.classList.contains(w))));
    }
  }

  function asNode(n) { return n instanceof Node_ ? n : new Text_(String(n)); }
  function descendants(root) {
    const out = [];
    const walk = (n) => {
      for (const c of n.childNodes) {
        if (c.nodeType === 1) { out.push(c); walk(c); }
      }
    };
    walk(root);
    return out;
  }

  class Text_ extends Node_ {
    constructor(data) { super(3); this.__d = String(data); }
    get data() { return this.__d; }
    set data(v) { this.__d = v == null ? '' : String(v); touch(); }
    get length() { return this.__d.length; }
    get nodeName() { return '#text'; }
    get wholeText() { return this.__d; }
  }
  class Comment_ extends Node_ {
    constructor(data) { super(8); this.__d = String(data); }
    get data() { return this.__d; }
    set data(v) { this.__d = String(v); }
    get nodeName() { return '#comment'; }
  }
  class Fragment_ extends Node_ {
    constructor() { super(11); }
    get nodeName() { return '#document-fragment'; }
  }

  // ---- Styles, classes, data ---------------------------------------

  const kebab = (k) => (k === 'cssFloat' ? 'float' : k.replace(/[A-Z]/g, (c) => '-' + c.toLowerCase()));
  const camel = (k) => k.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
  function parseStyle(text) {
    const m = new Map();
    for (const part of String(text || '').split(';')) {
      const i = part.indexOf(':');
      if (i > 0) m.set(part.slice(0, i).trim().toLowerCase(), part.slice(i + 1).trim());
    }
    return m;
  }
  function setStyle(el, name, value) {
    const m = parseStyle(el.getAttribute('style'));
    const v = value == null ? '' : String(value).trim();
    if (v === '') m.delete(name); else m.set(name, v);
    const text = Array.from(m, ([k, x]) => k + ': ' + x).join('; ');
    if (text) el.setAttribute('style', text); else el.removeAttribute('style');
  }
  function styleOf(el) {
    if (el.__s) return el.__s;
    el.__s = new Proxy({}, {
      get(_, k) {
        if (typeof k !== 'string') return undefined;
        const m = parseStyle(el.getAttribute('style'));
        switch (k) {
          case 'cssText': return el.getAttribute('style') || '';
          case 'length': return m.size;
          case 'setProperty': return (n, v) => setStyle(el, String(n).toLowerCase(), v);
          case 'removeProperty': return (n) => {
            const was = m.get(String(n).toLowerCase()) || '';
            setStyle(el, String(n).toLowerCase(), '');
            return was;
          };
          case 'getPropertyValue': return (n) => m.get(String(n).toLowerCase()) || '';
          case 'item': return (i) => Array.from(m.keys())[i] || '';
          default: return m.get(kebab(k)) || '';
        }
      },
      set(_, k, v) {
        if (k === 'cssText') { el.setAttribute('style', String(v)); return true; }
        if (typeof k === 'string') setStyle(el, kebab(k), v);
        return true;
      },
    });
    return el.__s;
  }
  function classesOf(el) {
    const get = () => String(el.getAttribute('class') || '').split(/\s+/).filter(Boolean);
    const put = (a) => el.setAttribute('class', a.join(' '));
    return {
      get length() { return get().length; },
      get value() { return el.getAttribute('class') || ''; },
      item: (i) => get()[i] || null,
      contains: (c) => get().includes(String(c)),
      add: (...cs) => { const a = get(); for (const c of cs) if (!a.includes(String(c))) a.push(String(c)); put(a); },
      remove: (...cs) => put(get().filter((x) => !cs.map(String).includes(x))),
      toggle(c, force) {
        c = String(c);
        const has = get().includes(c);
        const want = force === undefined ? !has : !!force;
        if (want && !has) this.add(c);
        if (!want && has) this.remove(c);
        return want;
      },
      replace(a, b) {
        const all = get();
        const i = all.indexOf(String(a));
        if (i < 0) return false;
        all[i] = String(b);
        put(all);
        return true;
      },
      forEach: (f) => get().forEach(f),
      toString: () => el.getAttribute('class') || '',
      [Symbol.iterator]: () => get()[Symbol.iterator](),
    };
  }
  function dataOf(el) {
    return new Proxy({}, {
      get: (_, k) => (typeof k === 'string' ? (el.getAttribute('data-' + kebab(k)) ?? undefined) : undefined),
      set: (_, k, v) => { el.setAttribute('data-' + kebab(String(k)), v); return true; },
      deleteProperty: (_, k) => { el.removeAttribute('data-' + kebab(String(k))); return true; },
      has: (_, k) => el.hasAttribute('data-' + kebab(String(k))),
      ownKeys: () => el.getAttributeNames().filter((n) => n.startsWith('data-')).map((n) => camel(n.slice(5))),
      getOwnPropertyDescriptor: () => ({ enumerable: true, configurable: true }),
    });
  }

  // ---- Elements ------------------------------------------------------

  const reflected = ['id', 'title', 'lang', 'dir', 'name', 'alt', 'src', 'href',
    'placeholder', 'role', 'target', 'rel', 'width', 'height', 'min', 'max', 'step'];
  const flags = ['hidden', 'disabled', 'readonly', 'required', 'multiple', 'open'];

  class Element_ extends Node_ {
    constructor(name) {
      super(1);
      this.localName = String(name);
      this.__a = new Map();
      this.__id = nextId++;
      this.__value = null;
      this.__checked = null;
      this.__selected = null;
      nodes.set(this.__id, this);
    }
    get tagName() { return this.localName.toUpperCase(); }
    get nodeName() { return this.tagName; }
    get namespaceURI() { return 'http://www.w3.org/1999/xhtml'; }
    getAttribute(n) { const v = this.__a.get(String(n).toLowerCase()); return v === undefined ? null : v; }
    setAttribute(n, v) { this.__a.set(String(n).toLowerCase(), String(v)); touch(); }
    removeAttribute(n) { if (this.__a.delete(String(n).toLowerCase())) touch(); }
    hasAttribute(n) { return this.__a.has(String(n).toLowerCase()); }
    hasAttributes() { return this.__a.size > 0; }
    getAttributeNames() { return Array.from(this.__a.keys()); }
    getAttributeNS(_, n) { return this.getAttribute(n); }
    setAttributeNS(_, n, v) { this.setAttribute(n, v); }
    removeAttributeNS(_, n) { this.removeAttribute(n); }
    toggleAttribute(n, force) {
      const want = force === undefined ? !this.hasAttribute(n) : !!force;
      if (want) this.setAttribute(n, ''); else this.removeAttribute(n);
      return want;
    }
    get attributes() {
      return list(Array.from(this.__a, ([name, value]) => ({ name: name, value: value, nodeName: name })));
    }
    get className() { return this.getAttribute('class') || ''; }
    set className(v) { this.setAttribute('class', v); }
    get classList() { return classesOf(this); }
    get style() { return styleOf(this); }
    set style(v) { this.setAttribute('style', v); }
    get dataset() { return dataOf(this); }
    get htmlFor() { return this.getAttribute('for') || ''; }
    set htmlFor(v) { this.setAttribute('for', v); }
    get tabIndex() { return Number(this.getAttribute('tabindex') || -1); }
    set tabIndex(v) { this.setAttribute('tabindex', v); }
    get type() {
      const t = (this.getAttribute('type') || '').toLowerCase();
      if (this.localName === 'input') return t || 'text';
      if (this.localName === 'button') return t || 'submit';
      if (this.localName === 'select') return this.multiple ? 'select-multiple' : 'select-one';
      return t;
    }
    set type(v) { this.setAttribute('type', v); }
    get value() {
      switch (this.localName) {
        case 'input':
          if (this.__value !== null) return this.__value;
          return this.getAttribute('value') ?? (/^(checkbox|radio)$/.test(this.type) ? 'on' : '');
        case 'textarea': return this.__value !== null ? this.__value : this.textContent;
        case 'select': { const o = this.options[this.selectedIndex]; return o ? o.value : ''; }
        case 'option': return this.getAttribute('value') ?? this.textContent.trim();
        default: return this.getAttribute('value') || '';
      }
    }
    set value(v) {
      const s = v == null ? '' : String(v);
      if (this.localName === 'select') {
        this.selectedIndex = this.options.findIndex((o) => o.value === s);
      } else if (this.localName === 'input' || this.localName === 'textarea') {
        this.__value = s;
        touch();
      } else {
        this.setAttribute('value', s);
      }
    }
    get defaultValue() { return this.getAttribute('value') || ''; }
    get checked() { return this.__checked !== null ? this.__checked : this.hasAttribute('checked'); }
    set checked(v) {
      const on = !!v;
      this.__checked = on;
      if (on && this.type === 'radio') for (const r of radioGroup(this)) if (r !== this) r.__checked = false;
      touch();
    }
    get defaultChecked() { return this.hasAttribute('checked'); }
    get selected() { return this.__selected !== null ? this.__selected : this.hasAttribute('selected'); }
    set selected(v) {
      const s = this.closest('select');
      if (v && s && !s.multiple) for (const o of s.options) o.__selected = false;
      this.__selected = !!v;
      touch();
    }
    get options() { return list(this.getElementsByTagName('option')); }
    get selectedOptions() { return list(this.options.filter((o) => o.selected)); }
    get selectedIndex() {
      const o = this.options;
      const i = o.findIndex((x) => x.selected);
      return i >= 0 || this.multiple ? i : (o.length ? 0 : -1);
    }
    set selectedIndex(i) { this.options.forEach((o, n) => { o.__selected = n === Number(i); }); touch(); }
    get text() { return this.textContent; }
    set text(v) { this.textContent = v; }
    get form() { return this.closest('form'); }
    get elements() { return list(this.querySelectorAll('input,select,textarea,button')); }
    get length() { return this.localName === 'select' ? this.options.length : this.elements.length; }
    get labels() {
      const id = this.id;
      const own = id ? doc.querySelectorAll('label[for="' + id + '"]') : [];
      const around = this.closest('label');
      return list(around && !own.includes(around) ? own.concat([around]) : own);
    }
    get control() {
      const f = this.getAttribute('for');
      return f ? doc.getElementById(f) : this.querySelector('input,select,textarea,button');
    }
    get innerText() { return this.textContent; }
    set innerText(v) { this.textContent = v; }
    get outerText() { return this.textContent; }
    get innerHTML() { return this.childNodes.map((c) => markup(c, false)).join(''); }
    set innerHTML(v) {
      this.textContent = '';
      for (const n of parse(String(v == null ? '' : v))) this.appendChild(n);
    }
    get outerHTML() { return markup(this, false); }
    set outerHTML(v) { const p = this.parentNode; if (p) { for (const n of parse(String(v))) p.insertBefore(n, this); this.remove(); } }
    insertAdjacentHTML(where, html) { this.insertAdjacent(where, parse(String(html))); }
    insertAdjacentElement(where, el) { this.insertAdjacent(where, [el]); return el; }
    insertAdjacentText(where, t) { this.insertAdjacent(where, [new Text_(t)]); }
    insertAdjacent(where, ns) {
      switch (String(where).toLowerCase()) {
        case 'beforebegin': this.before(...ns); break;
        case 'afterbegin': this.prepend(...ns); break;
        case 'beforeend': this.append(...ns); break;
        case 'afterend': this.after(...ns); break;
      }
    }
    closest(s) {
      const sel = parseSelector(s);
      for (let x = this; x && x.nodeType === 1; x = x.parentNode) if (matches(x, sel)) return x;
      return null;
    }
    matches(s) { return matches(this, parseSelector(s)); }
    webkitMatchesSelector(s) { return this.matches(s); }
    click() { activate(this); }
    focus() { doc.__active = this; }
    blur() { if (doc.__active === this) doc.__active = null; }
    select() {}
    scrollIntoView() {}
    scrollTo() {}
    animate() { return { finished: Promise.resolve(), cancel() {}, onfinish: null }; }
    getBoundingClientRect() { return { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0 }; }
    getClientRects() { return []; }
    get offsetWidth() { return 0; }
    get offsetHeight() { return 0; }
    get clientWidth() { return 0; }
    get clientHeight() { return 0; }
    get scrollTop() { return 0; }
    set scrollTop(v) {}
    get offsetParent() { return this.parentElement; }
    checkValidity() { return true; }
    reportValidity() { return true; }
    setCustomValidity() {}
    submit() {}
    reset() {
      for (const el of this.elements) { el.__value = null; el.__checked = null; }
      for (const o of this.querySelectorAll('option')) o.__selected = null;
      touch();
    }
  }
  for (const n of reflected) {
    Object.defineProperty(Element_.prototype, n, {
      get() { return this.getAttribute(n) || ''; },
      set(v) { this.setAttribute(n, v); },
    });
  }
  for (const n of flags) {
    Object.defineProperty(Element_.prototype, n === 'readonly' ? 'readOnly' : n, {
      get() { return this.hasAttribute(n); },
      set(v) { this.toggleAttribute(n, !!v); },
    });
  }
  const handlerProps = ['click', 'dblclick', 'mousedown', 'mouseup', 'change', 'input',
    'submit', 'load', 'keydown', 'keyup', 'focus', 'blur', 'pointerdown', 'pointerup',
    'touchstart', 'touchend', 'toggle', 'reset'];
  for (const t of handlerProps) {
    Object.defineProperty(Element_.prototype, 'on' + t, {
      get() { return this['__on' + t] || null; },
      set(f) { this['__on' + t] = typeof f === 'function' ? f : null; touch(); },
    });
  }

  function radioGroup(input) {
    const name = input.getAttribute('name');
    if (!name) return [input];
    const root = input.form || doc;
    return root.querySelectorAll('input').filter((r) => r.type === 'radio' && r.getAttribute('name') === name);
  }

  // ---- Document and window ---------------------------------------------

  class Document_ extends Node_ {
    constructor() { super(9); this.__active = null; }
    get nodeName() { return '#document'; }
    get documentElement() { return this.children[0] || null; }
    get head() { return this.querySelector('head'); }
    get body() { return this.querySelector('body'); }
    get title() { const t = this.querySelector('title'); return t ? t.textContent.trim() : ''; }
    set title(v) { const t = this.querySelector('title'); if (t) t.textContent = v; }
    get activeElement() { return this.__active || this.body; }
    get readyState() { return 'complete'; }
    get defaultView() { return G; }
    get location() { return G.location; }
    get forms() { return this.getElementsByTagName('form'); }
    get images() { return this.getElementsByTagName('img'); }
    get links() { return list(this.querySelectorAll('a[href]')); }
    get scripts() { return this.getElementsByTagName('script'); }
    get cookie() { return ''; }
    set cookie(v) {}
    get characterSet() { return 'UTF-8'; }
    get compatMode() { return 'CSS1Compat'; }
    get visibilityState() { return 'visible'; }
    get hidden() { return false; }
    hasFocus() { return true; }
    getElementById(id) {
      id = String(id);
      return descendants(this).find((el) => el.getAttribute('id') === id) || null;
    }
    getElementsByName(n) { return list(descendants(this).filter((el) => el.getAttribute('name') === String(n))); }
    createElement(n) { return new Element_(String(n).toLowerCase()); }
    createElementNS(_, n) { return new Element_(String(n)); }
    createTextNode(t) { return new Text_(t); }
    createComment(t) { return new Comment_(t); }
    createDocumentFragment() { return new Fragment_(); }
    createEvent() { return new Event_(''); }
    importNode(n, deep) { return n.cloneNode(deep); }
    adoptNode(n) { n.remove(); return n; }
    execCommand() { return false; }
  }
  Event_.prototype.initEvent = function (type, bubbles, cancelable) {
    this.type = type; this.bubbles = !!bubbles; this.cancelable = !!cancelable;
  };

  const doc = new Document_();

  class Storage_ {
    constructor() { this.__m = new Map(); }
    get length() { return this.__m.size; }
    key(i) { return Array.from(this.__m.keys())[i] ?? null; }
    getItem(k) { return this.__m.has(String(k)) ? this.__m.get(String(k)) : null; }
    setItem(k, v) { if (this.__m.size < 1000) this.__m.set(String(k), String(v)); }
    removeItem(k) { this.__m.delete(String(k)); }
    clear() { this.__m.clear(); }
  }

  function addTimer(f, ms, args, repeat) {
    if (timers.length >= MAX_TIMERS) return 0;
    if (typeof f !== 'function') {
      const code = String(f);
      f = () => (0, eval)(code);
    }
    const delay = Math.max(repeat ? 16 : 0, Number(ms) || 0);
    const id = timerSeq++;
    timers.push({ id: id, at: now + delay, f: f, args: args, every: repeat ? delay : 0 });
    return id;
  }
  const dropTimer = (id) => { timers = timers.filter((t) => t.id !== id); };

  function defineWindow() {
    const plain = {
      window: G, self: G, top: G, parent: G, frames: G, document: doc,
      Node: Node_, Element: Element_, HTMLElement: Element_, Text: Text_, Comment: Comment_,
      DocumentFragment: Fragment_, Document: Document_, EventTarget: Target_,
      Event: Event_, MouseEvent: MouseEvent_, PointerEvent: MouseEvent_, KeyboardEvent: KeyboardEvent_,
      CustomEvent: CustomEvent_, InputEvent: Event_, FocusEvent: Event_, UIEvent: Event_,
      Storage: Storage_, localStorage: new Storage_(), sessionStorage: new Storage_(),
      navigator: { userAgent: 'mareader', language: 'en', languages: ['en'], onLine: false,
        platform: '', vendor: '', cookieEnabled: false, maxTouchPoints: 0 },
      screen: { width: 800, height: 1100, availWidth: 800, availHeight: 1100, colorDepth: 24 },
      history: { length: 1, state: null, back() {}, forward() {}, go() {}, pushState() {}, replaceState() {} },
      innerWidth: 800, innerHeight: 1100, outerWidth: 800, outerHeight: 1100,
      devicePixelRatio: 1, scrollX: 0, scrollY: 0, pageXOffset: 0, pageYOffset: 0,
      NodeFilter: { SHOW_ALL: -1, SHOW_ELEMENT: 1, SHOW_TEXT: 4 },
    };
    for (const k in plain) G[k] = plain[k];
    G.HTMLInputElement = G.HTMLButtonElement = G.HTMLSelectElement = G.HTMLDivElement = Element_;
    G.HTMLAnchorElement = G.HTMLImageElement = G.HTMLTextAreaElement = G.HTMLFormElement = Element_;
    G.HTMLSpanElement = G.HTMLParagraphElement = G.HTMLLIElement = G.SVGElement = Element_;
    G.addEventListener = (t, f, o) => win.addEventListener(t, f, o);
    G.removeEventListener = (t, f, o) => win.removeEventListener(t, f, o);
    G.dispatchEvent = (e) => dispatch(win, e);
    G.alert = (m) => { if (alerts.length < MAX_ALERTS) alerts.push(String(m === undefined ? '' : m)); };
    G.confirm = (m) => { G.alert(m); return true; };
    G.prompt = (m, d) => (d === undefined ? null : String(d));
    G.print = () => {};
    G.open = () => null;
    G.close = () => {};
    G.focus = () => {};
    G.scrollTo = G.scroll = G.scrollBy = () => {};
    G.setTimeout = (f, ms, ...args) => addTimer(f, ms, args, false);
    G.setInterval = (f, ms, ...args) => addTimer(f, ms, args, true);
    G.clearTimeout = G.clearInterval = dropTimer;
    G.requestAnimationFrame = (f) => addTimer(() => f(now), 16, [], false);
    G.cancelAnimationFrame = dropTimer;
    G.queueMicrotask = (f) => { Promise.resolve().then(f); };
    G.getComputedStyle = (el) => computed(el);
    G.matchMedia = (q) => ({ matches: false, media: String(q), addListener() {}, removeListener() {},
      addEventListener() {}, removeEventListener() {} });
    G.getSelection = () => ({ toString: () => '', removeAllRanges() {}, addRange() {}, rangeCount: 0 });
    const log = () => {};
    G.console = { log: log, info: log, warn: log, error: log, debug: log, trace: log,
      group: log, groupEnd: log, table: log, dir: log, time: log, timeEnd: log, assert: log };
    G.performance = { now: () => now };
    G.location = { href: '', protocol: 'file:', host: '', hostname: '', pathname: '', search: '',
      hash: '', origin: 'null', assign() {}, replace() {}, reload() {}, toString() { return this.href; } };
  }

  // ---- Selectors -----------------------------------------------------

  const cache = new Map();
  function parseSelector(text) {
    text = String(text).trim();
    const known = cache.get(text);
    if (known) return known;
    const groups = splitTop(text, ',').map((g) => parseComplex(g.trim()));
    if (groups.some((g) => g === null)) throw new SyntaxError("'" + text + "' is not a valid selector");
    cache.set(text, groups);
    return groups;
  }
  function splitTop(s, ch) {
    const out = [];
    let depth = 0, quote = '', start = 0;
    for (let i = 0; i < s.length; i++) {
      const c = s[i];
      if (quote) { if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'") quote = c;
      else if (c === '(' || c === '[') depth++;
      else if (c === ')' || c === ']') depth--;
      else if (c === ch && depth === 0) { out.push(s.slice(start, i)); start = i + 1; }
    }
    out.push(s.slice(start));
    return out;
  }
  // A complex selector: compounds right to left, each with its combinator.
  function parseComplex(s) {
    if (!s) return null;
    const parts = [];
    let i = 0, comb = '';
    while (i < s.length) {
      while (i < s.length && /\s/.test(s[i])) { i++; if (!comb) comb = ' '; }
      if (i >= s.length) break;
      if ('>+~'.includes(s[i])) { comb = s[i]; i++; continue; }
      const r = parseCompound(s, i);
      if (!r) return null;
      parts.push({ comb: parts.length ? (comb || ' ') : '', test: r.test });
      comb = '';
      i = r.end;
    }
    return parts.length ? parts.reverse() : null;
  }
  const ident = /^-?[_a-zA-Z\u00a0-\uffff\\-][_a-zA-Z0-9\u00a0-\uffff\\-]*/;
  function parseCompound(s, i) {
    const tests = [];
    const start = i;
    while (i < s.length && !/[\s>+~,]/.test(s[i])) {
      const rest = s.slice(i);
      let m;
      if (rest[0] === '*') { i++; continue; }
      if (rest[0] === '#' && (m = rest.slice(1).match(ident))) {
        const id = unescape_(m[0]);
        tests.push((el) => el.getAttribute('id') === id);
        i += 1 + m[0].length;
      } else if (rest[0] === '.' && (m = rest.slice(1).match(ident))) {
        const c = unescape_(m[0]);
        tests.push((el) => el.classList.contains(c));
        i += 1 + m[0].length;
      } else if (rest[0] === '[') {
        const end = closing(s, i, '[', ']');
        if (end < 0) return null;
        const t = attrTest(s.slice(i + 1, end));
        if (!t) return null;
        tests.push(t);
        i = end + 1;
      } else if (rest[0] === ':') {
        const r = pseudo(s, i);
        if (!r) return null;
        tests.push(r.test);
        i = r.end;
      } else if ((m = rest.match(ident))) {
        const n = m[0].toLowerCase();
        tests.push((el) => el.localName.toLowerCase() === n);
        i += m[0].length;
      } else return null;
    }
    if (i === start) return null;
    return { test: (el) => tests.every((t) => t(el)), end: i };
  }
  const unescape_ = (s) => s.replace(/\\(.)/g, '$1');
  function closing(s, i, open, close) {
    let depth = 0, quote = '';
    for (let j = i; j < s.length; j++) {
      const c = s[j];
      if (quote) { if (c === quote) quote = ''; continue; }
      if (c === '"' || c === "'") quote = c;
      else if (c === open) depth++;
      else if (c === close && --depth === 0) return j;
    }
    return -1;
  }
  function attrTest(body) {
    const m = body.trim().match(/^([^\s~|^$*!=]+)\s*(?:([~|^$*]?=)\s*("[^"]*"|'[^']*'|[^\s\]]+)\s*(i)?)?$/);
    if (!m) return null;
    const name = m[1].toLowerCase();
    if (!m[2]) return (el) => el.hasAttribute(name);
    let want = m[3];
    if (/^["']/.test(want)) want = want.slice(1, -1);
    const fold = !!m[4];
    if (fold) want = want.toLowerCase();
    const op = m[2];
    return (el) => {
      let v = el.getAttribute(name);
      if (v === null) return false;
      if (fold) v = v.toLowerCase();
      switch (op) {
        case '=': return v === want;
        case '~=': return v.split(/\s+/).includes(want);
        case '|=': return v === want || v.startsWith(want + '-');
        case '^=': return want !== '' && v.startsWith(want);
        case '$=': return want !== '' && v.endsWith(want);
        case '*=': return want !== '' && v.includes(want);
      }
      return false;
    };
  }
  function nth(formula) {
    const f = formula.replace(/\s+/g, '').toLowerCase();
    if (f === 'odd') return [2, 1];
    if (f === 'even') return [2, 0];
    const m = f.match(/^([+-]?\d*)n([+-]\d+)?$/);
    if (m) {
      const a = m[1] === '' || m[1] === '+' ? 1 : m[1] === '-' ? -1 : Number(m[1]);
      return [a, Number(m[2] || 0)];
    }
    return /^[+-]?\d+$/.test(f) ? [0, Number(f)] : null;
  }
  const fits = ([a, b], n) => (a === 0 ? n === b : (n - b) / a >= 0 && (n - b) % a === 0);
  function pseudo(s, i) {
    const element = s[i + 1] === ':';
    const m = s.slice(i + (element ? 2 : 1)).match(/^[a-zA-Z-]+/);
    if (!m) return null;
    const name = m[0].toLowerCase();
    let end = i + (element ? 2 : 1) + m[0].length;
    let arg = null;
    if (s[end] === '(') {
      const close = closing(s, end, '(', ')');
      if (close < 0) return null;
      arg = s.slice(end + 1, close);
      end = close + 1;
    }
    const same = (el) => (el.parentNode ? el.parentNode.children : [el]);
    const ofType = (el) => same(el).filter((x) => x.localName === el.localName);
    let test;
    if (element) test = () => false;
    else switch (name) {
      case 'first-child': test = (el) => same(el)[0] === el; break;
      case 'last-child': test = (el) => { const a = same(el); return a[a.length - 1] === el; }; break;
      case 'only-child': test = (el) => same(el).length === 1; break;
      case 'first-of-type': test = (el) => ofType(el)[0] === el; break;
      case 'last-of-type': test = (el) => { const a = ofType(el); return a[a.length - 1] === el; }; break;
      case 'nth-child':
      case 'nth-last-child':
      case 'nth-of-type':
      case 'nth-last-of-type': {
        const f = nth(arg || '');
        if (!f) return null;
        const kind = name.includes('type') ? ofType : same;
        const last = name.includes('last');
        test = (el) => {
          const a = kind(el);
          const n = last ? a.length - a.indexOf(el) : a.indexOf(el) + 1;
          return fits(f, n);
        };
        break;
      }
      case 'not': {
        const inner = parseSelector(arg || '');
        test = (el) => !matches(el, inner);
        break;
      }
      case 'is':
      case 'where':
      case 'matches': {
        const inner = parseSelector(arg || '');
        test = (el) => matches(el, inner);
        break;
      }
      case 'has': {
        const inner = parseSelector(arg || '');
        test = (el) => descendants(el).some((d) => matches(d, inner));
        break;
      }
      case 'checked': test = (el) => (el.localName === 'option' ? el.selected : !!el.checked && el.localName === 'input'); break;
      case 'disabled': test = (el) => el.hasAttribute('disabled'); break;
      case 'enabled': test = (el) => !el.hasAttribute('disabled'); break;
      case 'empty': test = (el) => el.childNodes.every((c) => c.nodeType === 8); break;
      case 'root': test = (el) => el.parentNode === doc; break;
      case 'scope': test = () => true; break;
      case 'focus': test = (el) => doc.__active === el; break;
      case 'link': case 'any-link': test = (el) => el.localName === 'a' && el.hasAttribute('href'); break;
      default: test = () => false;
    }
    return { test: test, end: end };
  }
  function matches(el, groups) { return groups.some((g) => matchFrom(el, g, 0)); }
  function matchFrom(el, parts, k) {
    if (!el || el.nodeType !== 1 || !parts[k].test(el)) return false;
    if (k + 1 >= parts.length) return true;
    const comb = parts[k].comb;
    if (comb === '>') return matchFrom(el.parentElement, parts, k + 1);
    if (comb === '+') return matchFrom(el.previousElementSibling, parts, k + 1);
    if (comb === '~') {
      for (let x = el.previousElementSibling; x; x = x.previousElementSibling) if (matchFrom(x, parts, k + 1)) return true;
      return false;
    }
    for (let x = el.parentElement; x; x = x.parentElement) if (matchFrom(x, parts, k + 1)) return true;
    return false;
  }

  // ---- Computed style: only what scripts toggle on ----------------------

  function cssRules() {
    if (rules) return rules;
    rules = [];
    const own = doc.querySelectorAll('style').map((s) => s.textContent);
    for (const text of sheets.concat(own)) {
      const plain = String(text).replace(/\/\*[\s\S]*?\*\//g, '');
      const re = /([^{}]+)\{([^{}]*)\}/g;
      let m;
      while ((m = re.exec(plain)) && rules.length < 5000) {
        const head = m[1].trim();
        if (head.startsWith('@')) continue;
        let sel;
        try { sel = parseSelector(head.replace(/^.*;/, '')); } catch (e) { continue; }
        const decls = parseStyle(m[2]);
        if (decls.size) rules.push({ sel: sel, decls: decls });
      }
    }
    return rules;
  }
  function computed(el) {
    const own = parseStyle(el.getAttribute && el.getAttribute('style'));
    const found = new Map();
    if (el.nodeType === 1) {
      for (const r of cssRules()) {
        if (matches(el, r.sel)) for (const [k, v] of r.decls) found.set(k, v.replace(/\s*!important$/, ''));
      }
    }
    for (const [k, v] of own) found.set(k, v);
    if (el.hasAttribute && el.hasAttribute('hidden') && !own.has('display')) found.set('display', 'none');
    if (!found.has('display')) found.set('display', BLOCKS.has(el.localName) ? 'block' : 'inline');
    if (!found.has('visibility')) {
      let v = 'visible';
      for (let x = el.parentElement; x; x = x.parentElement) {
        const s = parseStyle(x.getAttribute('style')).get('visibility');
        if (s) { v = s; break; }
      }
      found.set('visibility', v);
    }
    return new Proxy({}, {
      get(_, k) {
        if (k === 'getPropertyValue') return (n) => found.get(String(n).toLowerCase()) || '';
        if (typeof k !== 'string') return undefined;
        return found.get(kebab(k)) || '';
      },
    });
  }

  // ---- Markup in and out -------------------------------------------------

  function build(json) {
    if (typeof json === 'string') return new Text_(json);
    const el = new Element_(json[0]);
    for (const k in json[1]) el.__a.set(k, json[1][k]);
    for (const c of json[2]) {
      const n = build(c);
      n.parentNode = el;
      el.childNodes.push(n);
    }
    return el;
  }
  function parse(html) {
    let json;
    try { json = JSON.parse(__mr_parse(html)); } catch (e) { report(e); json = []; }
    return json.map(build);
  }
  const text = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/\u00a0/g, '&#160;');
  const quoted = (s) => text(s).replace(/"/g, '&quot;');
  function attrs(el, skip, extra) {
    let out = '';
    for (const [k, v] of el.__a) if (!skip || !skip.has(k)) out += ' ' + k + '="' + quoted(v) + '"';
    for (const k in extra || {}) out += ' ' + k + '="' + quoted(String(extra[k])) + '"';
    return out;
  }
  // `reader` marks what responds to clicks and draws controls as text.
  function markup(n, reader, ctx) {
    if (n.nodeType === 3) return text(n.data);
    if (n.nodeType === 8) return '';
    if (n.nodeType !== 1) return n.childNodes.map((c) => markup(c, reader, ctx)).join('');
    const name = n.localName;
    const kids = () => {
      if (RAW.has(name)) return n.childNodes.map((c) => c.textContent).join('');
      const inner = ctx && { delegated: ctx.delegated || listens(n) };
      return n.childNodes.map((c) => markup(c, reader, inner)).join('');
    };
    if (reader) {
      // Scripts run here, so what shows without them goes.
      if (name === 'noscript') return '';
      const control = drawn(n);
      if (control !== undefined) return control;
    }
    const extra = reader && clickable(n, ctx) ? { 'data-mareader-node': n.__id } : null;
    const open = '<' + name + attrs(n, null, extra);
    if (VOID.has(name)) return open + '/>';
    return open + '>' + kids() + '</' + name + '>';
  }
  function listens(n) {
    if (n.__l && CLICKS.some((t) => (n.__l.get(t) || []).length)) return true;
    return CLICKS.some((t) => typeof ownHandler(n, t) === 'function' ||
      (n.nodeType === 1 && n.hasAttribute('on' + t)));
  }
  const likely = new Set(['a', 'button', 'img', 'li', 'td', 'th', 'label', 'summary', 'dt', 'dd', 'figure']);
  function clickable(n, ctx) {
    if (listens(n)) return true;
    if (n.localName === 'label' && n.control) return true;
    if (n.localName === 'button' || n.localName === 'summary') return true;
    if (!ctx || !ctx.delegated) return false;
    // Under a parent that handles its children's clicks: a likely target.
    return likely.has(n.localName) || n.hasAttribute('id') || n.hasAttribute('class') ||
      n.hasAttribute('role') || n.hasAttribute('tabindex') ||
      n.getAttributeNames().some((a) => a.startsWith('data-'));
  }
  const fieldTypes = /^(text|search|email|number|tel|url|password)$/;
  function drawn(n) {
    const name = n.localName;
    const mark = { 'data-mareader-node': n.__id };
    const skip = new Set(['type', 'value', 'checked', 'name', 'placeholder', 'size', 'selected']);
    const span = (body, style) => {
      const a = attrs(n, skip, mark);
      const merged = style ? ' style="' + quoted(style + ';' + (n.getAttribute('style') || '')) + '"' : '';
      return '<span' + (merged ? a.replace(/ style="[^"]*"/, '') + merged : a) + '>' + text(body) + '</span>';
    };
    if (name === 'select') {
      const o = n.options[n.selectedIndex];
      return span((o ? o.textContent.trim() : '') + ' \u25bc', 'text-decoration: underline');
    }
    if (name === 'textarea') return field(n, span);
    if (name !== 'input') return undefined;
    const type = n.type;
    if (type === 'hidden') return '';
    if (type === 'checkbox') return span(n.checked ? '\u25a0' : '\u25a1');
    if (type === 'radio') return span(n.checked ? '\u25c6' : '\u25c7');
    if (/^(button|submit|reset)$/.test(type)) {
      const label = n.getAttribute('value') || (type === 'reset' ? 'Reset' : type === 'submit' ? 'Submit' : '');
      return span('[\u2009' + label + '\u2009]', 'font-weight: bold');
    }
    if (fieldTypes.test(type)) return field(n, span);
    return '';
  }
  function field(n, span) {
    let v = n.value;
    if (n.type === 'password') v = '\u2022'.repeat(v.length);
    if (v) return span(v, 'text-decoration: underline');
    const hint = n.getAttribute('placeholder');
    if (hint) return span(hint, 'text-decoration: underline; font-style: italic; opacity: 0.6');
    return span('\u00a0'.repeat(12), 'text-decoration: underline');
  }

  // ---- What the reader does ----------------------------------------------

  function activate(el) {
    if (!el || el.hasAttribute('disabled')) return;
    const name = el.localName;
    const type = name === 'input' ? el.type : '';
    let undo = null;
    if (type === 'checkbox') {
      const was = el.checked;
      el.checked = !was;
      undo = () => { el.checked = was; };
    } else if (type === 'radio') {
      const was = el.checked;
      const before = radioGroup(el).find((r) => r.checked);
      el.checked = true;
      undo = () => { el.checked = was; if (before) before.checked = true; };
      if (was) undo = null;
    }
    const ok = dispatch(el, new MouseEvent_('click', { bubbles: true, cancelable: true }));
    if (!ok && undo) { undo(); return; }
    if (undo) {
      dispatch(el, new Event_('input', { bubbles: true }));
      dispatch(el, new Event_('change', { bubbles: true }));
      return;
    }
    if (!ok) return;
    if (name === 'select') {
      const count = el.options.length;
      if (count) {
        el.selectedIndex = (el.selectedIndex + 1) % count;
        dispatch(el, new Event_('input', { bubbles: true }));
        dispatch(el, new Event_('change', { bubbles: true }));
      }
    } else if (name === 'label') {
      const c = el.control;
      if (c && c !== el) activate(c);
    } else if (name === 'summary' && el.parentElement && el.parentElement.localName === 'details') {
      el.parentElement.toggleAttribute('open');
      dispatch(el.parentElement, new Event_('toggle'));
    } else if ((name === 'button' && el.type === 'submit') || type === 'submit') {
      const form = el.form;
      if (form) dispatch(form, new Event_('submit', { bubbles: true, cancelable: true }));
    } else if ((name === 'button' && el.type === 'reset') || type === 'reset') {
      const form = el.form;
      if (form && dispatch(form, new Event_('reset', { bubbles: true, cancelable: true }))) form.reset();
    }
  }

  function fields() {
    const out = {};
    for (const el of descendants(doc)) {
      const area = el.localName === 'textarea';
      if (area || (el.localName === 'input' && fieldTypes.test(el.type))) {
        out[el.__id] = { value: el.value, multiline: area, secret: el.type === 'password',
          hint: el.getAttribute('placeholder') || '' };
      }
    }
    return out;
  }

  G.__mr = {
    load(tree, styles) {
      for (const n of JSON.parse(tree).map(build)) { n.parentNode = doc; doc.childNodes.push(n); }
      sheets = JSON.parse(styles);
      defineWindow();
    },
    ready() {
      dispatch(doc, new Event_('readystatechange'));
      dispatch(doc, new Event_('DOMContentLoaded', { bubbles: true }));
      dispatch(win, new Event_('load'));
      const body = doc.body;
      const onload = body && body.getAttribute('onload');
      if (onload) { try { new Function('event', onload).call(body); } catch (e) { report(e); } }
    },
    flush() {
      const out = { page: null, fields: fields(), alerts: alerts, errors: errors };
      if (dirty) out.page = doc.childNodes.map((c) => markup(c, true, { delegated: listens(doc) || listens(win) })).join('');
      dirty = false;
      alerts = [];
      errors = [];
      rules = null;
      return JSON.stringify(out);
    },
    click(id) { const el = nodes.get(id); if (el && el.isConnected) activate(el); },
    pick(selector) {
      let el = null;
      try { el = doc.querySelector(selector); } catch (e) { report(e); }
      if (el) activate(el);
    },
    value(id, v) {
      const el = nodes.get(id);
      if (!el || !el.isConnected) return;
      el.value = v;
      dispatch(el, new Event_('input', { bubbles: true }));
      dispatch(el, new Event_('change', { bubbles: true }));
    },
    at(clock) { now = Math.max(now, clock); },
    due() {
      let at = Infinity;
      for (const t of timers) at = Math.min(at, t.at);
      return at === Infinity ? -1 : Math.max(0, at - now);
    },
    tick() {
      for (let n = 0; n < 1000; n++) {
        let next = null;
        for (const t of timers) if (t.at <= now && (!next || t.at < next.at)) next = t;
        if (!next) break;
        if (next.every) next.at += next.every; else dropTimer(next.id);
        try { next.f(...next.args); } catch (e) { report(e); }
      }
    },
    report: report,
  };
})();
