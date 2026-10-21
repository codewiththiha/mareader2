//! One chapter's scripts in their own sandboxed context.

use std::time::{Duration, Instant};

use boa_engine::{Context, JsResult, JsString, JsValue, NativeFunction, Source, js_string};
use serde_json::Value;

use crate::{Ask, Change, Field, Page};

/// The page model scripts run against.
const PAGE: &str = include_str!("page.js");
/// Iterations one loop may run before it is stopped.
const MAX_LOOP: u64 = 2_000_000;
const MAX_RECURSION: usize = 400;
/// Markup a script may parse at once, as `innerHTML`.
const MAX_MARKUP: usize = 1 << 20;
/// Errors kept from one change.
const MAX_ERRORS: usize = 20;

/// A chapter's scripts, loaded and ready for clicks.
pub struct Engine {
    chapter: usize,
    context: Context,
    started: Instant,
}

impl Engine {
    /// Runs `page`'s scripts once its markup is read; the page they made.
    pub fn new(page: &Page) -> (Self, Change) {
        let mut context = Context::default();
        let limits = context.runtime_limits_mut();
        limits.set_loop_iteration_limit(MAX_LOOP);
        limits.set_recursion_limit(MAX_RECURSION);
        let parse = NativeFunction::from_fn_ptr(parse_markup);
        let _ = context.register_global_builtin_callable(js_string!("__mr_parse"), 1, parse);
        let mut engine = Self {
            chapter: page.chapter,
            context,
            started: Instant::now(),
        };
        let mut errors = Vec::new();
        errors.extend(engine.run(PAGE).err());
        let tree = crate::tree::read(&page.xhtml);
        let styles = serde_json::to_string(&page.styles).unwrap_or_default();
        let load = format!("__mr.load({}, {})", quote(&tree), quote(&styles));
        errors.extend(engine.run(&load).err());
        for code in &page.scripts {
            errors.extend(engine.run(code).err());
        }
        errors.extend(engine.run("__mr.ready()").err());
        let mut change = engine.flush();
        errors.append(&mut change.errors);
        errors.truncate(MAX_ERRORS);
        change.errors = errors;
        (engine, change)
    }

    pub fn chapter(&self) -> usize {
        self.chapter
    }

    /// Does what the reader asked; what changed.
    pub fn ask(&mut self, ask: &Ask) -> Change {
        let code = match ask {
            Ask::Click(node) => format!("__mr.click({node})"),
            Ask::Value(node, value) => format!("__mr.value({node}, {})", quote(value)),
            Ask::Pick(selector) => format!("__mr.pick({})", quote(selector)),
        };
        let error = self.run(&code).err();
        let mut change = self.flush();
        change.errors.extend(error);
        change
    }

    /// How long until a timer is due; none if no timer is set.
    pub fn due(&mut self) -> Option<Duration> {
        let ms = self.eval(&format!("{}; __mr.due()", self.clock())).ok()?;
        let ms = ms.as_number()?;
        (ms >= 0.0 && ms.is_finite()).then(|| Duration::from_secs_f64(ms / 1000.0))
    }

    /// Runs the timers due by now; what changed.
    pub fn tick(&mut self) -> Change {
        let error = self.run("__mr.tick()").err();
        let mut change = self.flush();
        change.errors.extend(error);
        change
    }

    /// The page if it changed, its fields, and what it said.
    fn flush(&mut self) -> Change {
        let mut change = Change {
            chapter: self.chapter,
            ..Change::default()
        };
        let text = self.eval("__mr.flush()").ok().and_then(|v| v.as_string());
        let json = text.map(|t| t.to_std_string_escaped()).unwrap_or_default();
        let Ok(Value::Object(out)) = serde_json::from_str::<Value>(&json) else {
            return change;
        };
        change.page = out.get("page").and_then(Value::as_str).map(str::to_owned);
        let strings = |key: &str| -> Vec<String> {
            let items = out.get(key).and_then(Value::as_array);
            let items = items
                .map(|a| a.iter().filter_map(Value::as_str))
                .into_iter()
                .flatten();
            items.map(str::to_owned).collect()
        };
        change.alerts = strings("alerts");
        change.errors = strings("errors");
        if let Some(Value::Object(fields)) = out.get("fields") {
            for (node, f) in fields {
                let Ok(node) = node.parse::<u32>() else {
                    continue;
                };
                let text = |k: &str| {
                    f.get(k)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned()
                };
                let flag = |k: &str| f.get(k).and_then(Value::as_bool).unwrap_or(false);
                change.fields.push((
                    node,
                    Field {
                        value: text("value"),
                        hint: text("hint"),
                        multiline: flag("multiline"),
                        secret: flag("secret"),
                    },
                ));
            }
        }
        change
    }

    /// Runs `code` and the jobs it queued; its error, if it threw.
    fn run(&mut self, code: &str) -> Result<(), String> {
        let clock = self.clock();
        let _ = self.eval(&clock);
        let result = self.eval(code);
        let jobs = self.context.run_jobs().map_err(|e| e.to_string());
        result.map(|_| ()).and(jobs)
    }

    /// Code that moves the page's clock to now.
    fn clock(&self) -> String {
        let ms = self.started.elapsed().as_secs_f64() * 1000.0;
        format!("typeof __mr == 'object' && __mr.at({ms})")
    }

    fn eval(&mut self, code: &str) -> Result<JsValue, String> {
        let result: JsResult<JsValue> = self.context.eval(Source::from_bytes(code));
        result.map_err(|e| e.to_string())
    }
}

/// `__mr_parse(markup)`: markup's nodes as JSON, for `innerHTML`.
fn parse_markup(_: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let markup = match args.first() {
        Some(v) => v.to_string(context)?.to_std_string_escaped(),
        None => String::new(),
    };
    let markup = markup
        .get(..markup.len().min(MAX_MARKUP))
        .unwrap_or_default();
    Ok(JsValue::from(JsString::from(
        crate::tree::read(markup).as_str(),
    )))
}

/// `s` as a JavaScript string literal.
fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}
