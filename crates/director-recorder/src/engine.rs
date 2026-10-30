//! Page scripts on V8.

use crate::natives::{Args, Fault, Out, NATIVES};
use director_dom::protocol::Script;

/// Browser environment for page scripts.
const DOM: &str = include_str!("dom.js");

/// Start V8 once per process. The run thread's 64 MB stack holds deep
/// script recursion, so V8 may use most of it.
fn start() {
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        v8::V8::set_flags_from_string("--stack-size=32768");
        v8::V8::initialize_platform(v8::new_default_platform(0, false).make_shared());
        v8::V8::initialize();
    });
}

/// Evaluate the scripts, fire the load events and seek each sample time in
/// `times[from..]`. A run that starts mid-timeline first seeks the sample
/// before it, and before that the last earlier sample inside each finished
/// animation that draws from `onUpdate`, so the run starts from the state
/// earlier samples leave. Returns each sample's change record and how many
/// random numbers the scripts drew while the timeline played.
pub(crate) fn execute(scripts: &[Script], times: &[f64], from: usize) -> Result<(Vec<String>, u64), String> {
    start();
    let isolate = &mut v8::Isolate::new(v8::CreateParams::default());
    isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    register(scope, context);
    eval(scope, DOM)?;
    for script in scripts {
        eval(scope, &script.source).map_err(|e| format!("{}: {e}", script.name))?;
        if let Some(major) = script.major {
            let version = eval(scope, "typeof gsap === 'object' && gsap.version || ''")?;
            let found = version.split('.').next().and_then(|v| v.parse::<u32>().ok());
            if found.is_some_and(|found| found != major) {
                return Err(format!("{} names GSAP {major}, and the local copy is GSAP {version}", script.name));
            }
        }
    }
    eval(scope, "__director.loaded()")?;
    eval(scope, "__director.settle()")?;
    crate::natives::with_doc(|d| {
        d.playing = true;
        Ok(())
    })
    .map_err(|_| "no installed document")?;
    eval(scope, "__director.play()")?;
    let frame = |scope: &mut v8::PinScope<'_, '_>, t: f64| {
        eval(scope, &format!("__director.frame({t})")).map_err(|e| format!("at {t:.3}s: {e}"))
    };
    if from > 0 {
        let earlier = &times[..from];
        let before = earlier[earlier.len() - 1];
        let spans: Vec<f64> = serde_json::from_str(&eval(scope, "__director.drawers()")?).map_err(|e| e.to_string())?;
        // The reverse half of each seek renders an animation while its end
        // is at least a microsecond past the sample.
        let mut warm: Vec<f64> = spans
            .as_chunks::<2>()
            .0
            .iter()
            .filter_map(|span| {
                let last = earlier.partition_point(|&t| t + 1e-6 <= span[1] + 1e-9);
                let t = *earlier.get(last.checked_sub(1)?)?;
                (t >= span[0] - 1e-6 && t < before).then_some(t)
            })
            .collect();
        warm.sort_by(f64::total_cmp);
        warm.dedup();
        warm.push(before);
        for t in warm {
            frame(scope, t)?;
        }
    }
    let mut records = Vec::with_capacity(times.len() - from);
    for &t in &times[from..] {
        records.push(frame(scope, t)?);
    }
    let draws = eval(scope, "__director.draws()")?.parse().map_err(|_| "GSAP draw count is malformed".to_string())?;
    Ok((records, draws))
}

/// Run `code`, then its promise jobs, and return the result as a string. An
/// exception becomes its `toString` text without a leading `Error: `.
fn eval(scope: &mut v8::PinScope<'_, '_>, code: &str) -> Result<String, String> {
    v8::tc_scope!(let tc, scope);
    let source = v8::String::new(tc, code).ok_or("a script is too long for V8")?;
    let value = v8::Script::compile(tc, source, None).and_then(|script| script.run(tc));
    if value.is_some() {
        tc.perform_microtask_checkpoint();
    }
    match (value, tc.exception()) {
        (Some(value), None) => Ok(value.to_rust_string_lossy(tc)),
        (_, Some(e)) => {
            let text = e.to_rust_string_lossy(tc);
            Err(text.strip_prefix("Error: ").unwrap_or(&text).to_string())
        }
        (None, None) => Err("V8 stopped the script".into()),
    }
}

struct V8Args<'a, 's, 'i, 'c> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    args: &'a v8::FunctionCallbackArguments<'c>,
}

impl<'c> V8Args<'_, '_, '_, 'c> {
    fn arg(&self, n: usize) -> v8::Local<'c, v8::Value> {
        self.args.get(n as i32)
    }
}

/// The exception V8 already holds for a failed conversion.
fn thrown() -> Fault {
    Fault::Thrown
}

impl Args for V8Args<'_, '_, '_, '_> {
    fn int(&mut self, n: usize) -> Result<i64, Fault> {
        self.arg(n).int32_value(self.scope).map(i64::from).ok_or_else(thrown)
    }
    fn text(&mut self, n: usize) -> Result<String, Fault> {
        let text = self.arg(n).to_string(self.scope).ok_or_else(thrown)?;
        Ok(text.to_rust_string_lossy(self.scope))
    }
    fn opt(&mut self, n: usize) -> Result<Option<String>, Fault> {
        if self.arg(n).is_null_or_undefined() {
            return Ok(None);
        }
        self.text(n).map(Some)
    }
    fn flag(&mut self, n: usize) -> bool {
        self.arg(n).boolean_value(self.scope)
    }
    fn number(&mut self, n: usize) -> Result<f64, Fault> {
        self.arg(n).number_value(self.scope).ok_or_else(thrown)
    }
    fn given(&self, n: usize) -> bool {
        !self.arg(n).is_undefined()
    }
}

fn value<'s>(out: Out, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
    let text = |scope: &mut v8::PinScope<'s, '_>, s: &str| -> v8::Local<'s, v8::Value> {
        v8::String::new(scope, s).map_or_else(|| v8::String::empty(scope).into(), Into::into)
    };
    let list = |scope: &mut v8::PinScope<'s, '_>, items: Vec<v8::Local<'s, v8::Value>>| -> v8::Local<'s, v8::Value> {
        v8::Array::new_with_elements(scope, &items).into()
    };
    match out {
        Out::Undefined => v8::undefined(scope).into(),
        Out::Null => v8::null(scope).into(),
        Out::Bool(b) => v8::Boolean::new(scope, b).into(),
        Out::Int(i) => v8::Integer::new(scope, i).into(),
        Out::Number(n) => v8::Number::new(scope, n).into(),
        Out::Text(s) => text(scope, &s),
        Out::Indexes(items) => {
            let items = items.into_iter().map(|i| v8::Integer::new(scope, i as i32).into()).collect();
            list(scope, items)
        }
        Out::Strings(items) => {
            let items = items.iter().map(|s| text(scope, s)).collect();
            list(scope, items)
        }
        Out::Numbers(items) => {
            let items = items.into_iter().map(|n| v8::Number::new(scope, n as f64).into()).collect();
            list(scope, items)
        }
    }
}

/// Every native: the function's data slot holds its index in [`NATIVES`].
fn call(scope: &mut v8::PinScope<'_, '_>, args: v8::FunctionCallbackArguments<'_>, mut rv: v8::ReturnValue<'_, v8::Value>) {
    let k = args.data().int32_value(scope).unwrap_or(0) as usize;
    let result = NATIVES[k].2(&mut V8Args { scope, args: &args });
    let (make, message): (fn(_, _) -> _, String) = match result {
        Ok(out) => {
            let out = value(out, scope);
            rv.set(out);
            return;
        }
        Err(Fault::Error(m)) => (v8::Exception::error, m),
        Err(Fault::Range(m)) => (v8::Exception::range_error, m),
        Err(Fault::Thrown) => return,
    };
    let message = v8::String::new(scope, &message).unwrap_or_else(|| v8::String::empty(scope));
    let exception = make(scope, message);
    scope.throw_exception(exception);
}

/// Register the DOM natives as globals that do not enumerate.
fn register(scope: &mut v8::PinScope<'_, '_>, context: v8::Local<v8::Context>) {
    let global = context.global(scope);
    for (k, (name, length, _)) in NATIVES.iter().enumerate() {
        let data = v8::Integer::new(scope, k as i32);
        let function = v8::FunctionTemplate::builder(call)
            .data(data.into())
            .length(*length as i32)
            .build(scope)
            .get_function(scope)
            .expect("a native function");
        let name = v8::String::new(scope, name).expect("a native name");
        global.define_own_property(scope, name.into(), function.into(), v8::PropertyAttribute::DONT_ENUM);
    }
}
