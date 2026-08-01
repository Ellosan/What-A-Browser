# JavaScript in WAT

WAT has its own JavaScript engine. It is not V8, SpiderMonkey or JavaScriptCore,
and it is not a binding to one: a lexer, a recursive-descent parser and a
tree-walking interpreter, in `crates/wat-js`, with the DOM bindings in
`crates/wat-script`. Together they are about 8,000 lines of Rust with no
dependencies beyond `log`.

This document says exactly what runs and what does not, because a browser that
is vague about that is worse than one that says no.

## Two safety properties

Page scripts run on the same thread as the browser's own interface, so two
things are enforced on every run:

* a **step budget** — 5,000,000 statements and loop iterations by default
* a **call depth limit** — 200 nested calls

Both surface as a fatal error that `try`/`catch` deliberately cannot swallow. A
runaway loop stops in well under a second and a runaway recursion stops before
the Rust stack does. A bad script spoils its own page and nothing else.

```js
try { while (true) {} } catch (e) { 'this never runs' }
// → script took too long and was stopped
```

Both limits are configurable through `wat_js::Limits`.

## The language

Supported:

| | |
| --- | --- |
| Values | numbers, strings, booleans, `null`, `undefined`, objects, arrays, functions |
| Declarations | `var` (hoisted, function-scoped), `let`, `const`, per-iteration loop bindings |
| Functions | declarations, expressions, arrows, default parameters, rest parameters, `arguments`, closures |
| Objects | shorthand, computed keys, methods, spread, `get`/`set` accessors |
| Classes | `extends`, `super`, static and instance fields, `#private` fields, accessors |
| Destructuring | objects, arrays, nesting, defaults, rest, in parameters and assignments |
| Control flow | `if`, `for`, `for-in`, `for-of`, `while`, `do`, `switch`, labelled `break`/`continue`, `try`/`catch`/`finally`, `throw` |
| Operators | arithmetic, comparison, logical, bitwise, `**`, `??`, `?.`, `&&=`/`\|\|=`/`??=`, `typeof`, `instanceof`, `in`, `delete`, `void` |
| Other | template literals, automatic semicolon insertion, `strict`-ish scoping |

Not supported:

* **Regular expressions.** No `RegExp`, and therefore no `String#match`,
  `matchAll` or `search`. `replace` and `replaceAll` take plain strings.
* **Promises, `async`/`await`, generators.** There is no microtask queue.
  `await` parses and evaluates to its operand; `async` functions run
  synchronously; `yield` is not implemented.
* **`Symbol`, `Proxy`, `Reflect`, `Map`, `Set`, `WeakMap`, `WeakSet`.**
* **Modules.** `import` and `export` are not parsed; a `<script type="module">`
  runs as an ordinary script.
* **Labelled statements other than loops**, `with`, and `eval` of dynamic source
  from inside a script.

Strings are indexed by Unicode scalar value rather than UTF-16 code unit, so an
astral character counts as one, not two. Everything else about string handling
matches.

## The built-ins

`console` (`log`, `info`, `warn`, `error`, `debug`, `dir`, `trace`), `Math`
(every function and constant except `imul` and `clz32`), `JSON` (`parse` and
`stringify`, with indentation), `Object` (`keys`, `values`, `entries`, `assign`,
`create`, `fromEntries`, `hasOwn`, `defineProperty`, `getPrototypeOf`,
`setPrototypeOf`, `freeze`), `Array` (`isArray`, `from`, `of` and the full set of
instance methods including a stable `sort`), `String`, `Number`, `Boolean`,
`Date` (UTC only — there is no time-zone database), `Error` and its subclasses,
`parseInt`, `parseFloat`, `isNaN`, `isFinite`, the URI escaping functions, and
`setTimeout`/`setInterval`/`clearTimeout`/`clearInterval`.

`Object.freeze` returns its argument without enforcing anything, and
`Object.defineProperty` supports `value` and calls a `get` once rather than
installing an accessor. Both are documented compromises rather than silent
failures.

## The DOM

| Object | What is bound |
| --- | --- |
| `document` | `getElementById`, `querySelector`/`All`, `getElementsByTagName`/`ClassName`, `createElement`, `createTextNode`, `createComment`, `body`, `head`, `documentElement`, `title`, `location`, `readyState`, `addEventListener` |
| Elements | `tagName`, `id`, `className`, `classList`, `style`, `textContent`, `innerHTML`, `outerHTML`, `getAttribute` and friends, reflected properties (`href`, `src`, `value`, `disabled`, …), the whole tree-walking family, `appendChild`, `insertBefore`, `removeChild`, `replaceChild`, `remove`, `prepend`, `append`, `cloneNode`, `contains`, `matches`, `closest`, `getBoundingClientRect`, `offsetWidth`/`Height`/`Left`/`Top`, `addEventListener`, `click` |
| `classList` | `add`, `remove`, `toggle` (with force), `contains`, `replace`, `item`, indexing, `length`, `value` |
| `style` | every property by camelCase or hyphenated name, `cssText`, `setProperty`, `getPropertyValue`, `removeProperty` |
| `window` | `document`, `location`, `navigator`, `innerWidth`/`Height`, `devicePixelRatio`, `scrollX`/`Y`, `scrollTo`, `scrollBy`, `alert`, `confirm`, `prompt`, `addEventListener`, and any global you assign to it |
| `location` | `href`, `protocol`, `host`, `hostname`, `port`, `pathname`, `search`, `hash`, `origin`, `assign`, `replace`, `reload` |
| `Event` | `type`, `target`, `currentTarget`, `bubbles`, `defaultPrevented`, `preventDefault`, `stopPropagation` |

`querySelectorAll` and friends return **JavaScript arrays**, not live
`NodeList`s. That costs liveness; in exchange `forEach`, `map`, `filter` and
spread all work through the ordinary array built-ins.

Layout rectangles are a **snapshot**, taken before each run. A script that
measures after mutating the page in the same run sees the previous layout;
`load` handlers, event handlers and timer callbacks all measure the current one,
because the page is laid out again between runs. There is no synchronous reflow.

Not bound: `getComputedStyle`, `requestAnimationFrame`, `matchMedia`, `fetch`,
`XMLHttpRequest`, `localStorage`, cookies, `history`, `MutationObserver`,
`IntersectionObserver`, canvas, workers and custom elements. A page that fetches
its content after rendering will stay empty.

## Events

`addEventListener(type, fn, { once })`, `el.onclick = fn` and inline
`onclick="…"` attributes all work. An event fires at its target and then at each
ancestor; `stopPropagation` ends the walk and `preventDefault` is reported back
to the browser, which is what stops a link being followed.

An assigned `el.onclick` supersedes the matching attribute rather than firing
alongside it. Capture-phase listeners are accepted but run in the bubble phase.

Timers are **queued, not run**: `setTimeout` hands the callback to the host,
which runs it from its own event loop. That keeps the browser in control of when
page code executes and means a timer can never fire in the middle of another
script. The window pumps timers between events and goes back to waiting when no
page has one pending.

## Running scripts yourself

The engine is usable on its own. Nothing about the DOM is privileged — it goes
through the same `HostObject` trait an embedder would use:

```rust
use wat_js::{Interp, Value};

let mut interp = Interp::new();
interp.define_global("answer", Value::Number(42.0));
assert_eq!(
    interp.eval("`the answer is ${answer}`").unwrap().to_js_string(),
    "the answer is 42"
);
```

With a document, through `wat-script`:

```rust
use wat_script::ScriptRuntime;

let mut document = wat_html::parse("<p id='out'>before</p>");
let mut runtime = ScriptRuntime::new("about:blank");
runtime.eval(&mut document, "document.getElementById('out').textContent = 'after'");
```

The runtime borrows the document for the duration of each call and hands it
back, so the engine's layout and paint passes keep working with a plain
`&Document`.

Or against a live page, which is what a developer console would do:

```rust
let mut page = /* … */;
println!("{}", page.eval("document.querySelectorAll('a').length").unwrap());
```

`Page::eval` restyles and re-lays out whatever the expression changed.

## Turning it off

```rust
page.set_scripting_enabled(false);
```

Scripting is a flag, not a separate code path. Switching it off drops the
runtime, so the page loses its listeners and timers along with its ability to
run anything new.
