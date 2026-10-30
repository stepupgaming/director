//! Page scripts on Boa.

use crate::natives::{Args, Fault, Out, NATIVES};
use boa_engine::{
    object::builtins::JsArray, Context, JsError, JsNativeError, JsResult, JsString, JsValue, NativeFunction, Source,
};
use director_dom::protocol::Script;

/// Browser environment for page scripts.
const DOM: &str = include_str!("dom.js");

fn js_error(e: JsError) -> String {
    let text = e.to_string();
    text.strip_prefix("Error: ").unwrap_or(&text).to_string()
}

/// Evaluate the scripts, fire the load events and seek each sample time in
/// `times[from..]`. A run that starts mid-timeline first seeks the sample
/// before it, and before that the last earlier sample inside each finished
/// animation that draws from `onUpdate`, so the run starts from the state
/// earlier samples leave. Returns each sample's change record and how many
/// random numbers the scripts drew while the timeline played.
pub(crate) fn execute(scripts: &[Script], times: &[f64], from: usize) -> Result<(Vec<String>, u64), String> {
    let mut cx = Context::default();
    cx.runtime_limits_mut().set_loop_iteration_limit(50_000_000);
    cx.runtime_limits_mut().set_recursion_limit(4_096);
    register(&mut cx).map_err(js_error)?;
    cx.eval(Source::from_bytes(DOM)).map_err(js_error)?;
    let eval = |cx: &mut Context, code: &str| -> Result<String, String> {
        let value = cx.eval(Source::from_bytes(code)).map_err(js_error)?;
        cx.run_jobs().map_err(js_error)?;
        Ok(value.to_string(cx).map_err(js_error)?.to_std_string_escaped())
    };
    for script in scripts {
        eval(&mut cx, &script.source).map_err(|e| format!("{}: {e}", script.name))?;
        if let Some(major) = script.major {
            let version = eval(&mut cx, "typeof gsap === 'object' && gsap.version || ''")?;
            let found = version.split('.').next().and_then(|v| v.parse::<u32>().ok());
            if found.is_some_and(|found| found != major) {
                return Err(format!("{} names GSAP {major}, and the local copy is GSAP {version}", script.name));
            }
        }
    }
    eval(&mut cx, "__director.loaded()")?;
    eval(&mut cx, "__director.settle()")?;
    crate::natives::with_doc(|d| {
        d.playing = true;
        Ok(())
    })
    .map_err(|_| "no installed document")?;
    eval(&mut cx, "__director.play()")?;
    let frame = |cx: &mut Context, t: f64| eval(cx, &format!("__director.frame({t})")).map_err(|e| format!("at {t:.3}s: {e}"));
    if from > 0 {
        let earlier = &times[..from];
        let before = earlier[earlier.len() - 1];
        let spans: Vec<f64> = serde_json::from_str(&eval(&mut cx, "__director.drawers()")?).map_err(|e| e.to_string())?;
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
            frame(&mut cx, t)?;
        }
    }
    let mut records = Vec::with_capacity(times.len() - from);
    for &t in &times[from..] {
        records.push(frame(&mut cx, t)?);
    }
    let draws = eval(&mut cx, "__director.draws()")?.parse().map_err(|_| "GSAP draw count is malformed".to_string())?;
    Ok((records, draws))
}

struct BoaArgs<'a> {
    args: &'a [JsValue],
    cx: &'a mut Context,
}

impl BoaArgs<'_> {
    fn arg(&self, n: usize) -> JsValue {
        self.args.get(n).cloned().unwrap_or_default()
    }
}

fn thrown(e: JsError) -> Fault {
    Fault::Thrown(Box::new(e))
}

impl Args for BoaArgs<'_> {
    fn int(&mut self, n: usize) -> Result<i64, Fault> {
        Ok(self.arg(n).to_i32(self.cx).map_err(thrown)? as i64)
    }
    fn text(&mut self, n: usize) -> Result<String, Fault> {
        Ok(self.arg(n).to_string(self.cx).map_err(thrown)?.to_std_string_escaped())
    }
    fn opt(&mut self, n: usize) -> Result<Option<String>, Fault> {
        match self.args.get(n) {
            None => Ok(None),
            Some(v) if v.is_null_or_undefined() => Ok(None),
            Some(_) => self.text(n).map(Some),
        }
    }
    fn flag(&mut self, n: usize) -> bool {
        self.args.get(n).is_some_and(JsValue::to_boolean)
    }
    fn number(&mut self, n: usize) -> Result<f64, Fault> {
        self.arg(n).to_number(self.cx).map_err(thrown)
    }
    fn given(&self, n: usize) -> bool {
        self.args.get(n).is_some_and(|v| !v.is_undefined())
    }
}

fn value(out: Out, cx: &mut Context) -> JsValue {
    let text = |s: &str| JsValue::from(JsString::from(s));
    match out {
        Out::Undefined => JsValue::undefined(),
        Out::Null => JsValue::null(),
        Out::Bool(b) => JsValue::from(b),
        Out::Int(i) => JsValue::from(i),
        Out::Number(n) => JsValue::from(n),
        Out::Text(s) => text(&s),
        Out::Indexes(list) => JsArray::from_iter(list.into_iter().map(|i| JsValue::from(i as i32)), cx).into(),
        Out::Strings(list) => JsArray::from_iter(list.iter().map(|s| text(s)), cx).into(),
        Out::Numbers(list) => JsArray::from_iter(list.iter().map(|n| JsValue::from(*n as f64)), cx).into(),
    }
}

fn fault(f: Fault) -> JsError {
    match f {
        Fault::Error(m) => JsNativeError::error().with_message(m).into(),
        Fault::Range(m) => JsNativeError::range().with_message(m).into(),
        Fault::Thrown(e) => *e.downcast::<JsError>().expect("a Boa exception"),
    }
}

/// Register the DOM natives as globals.
fn register(cx: &mut Context) -> JsResult<()> {
    for (name, length, f) in NATIVES {
        let body = NativeFunction::from_copy_closure(move |_, args, cx| {
            let mut a = BoaArgs { args, cx };
            match f(&mut a) {
                Ok(out) => Ok(value(out, a.cx)),
                Err(e) => Err(fault(e)),
            }
        });
        cx.register_global_callable(JsString::from(name), length, body)?;
    }
    Ok(())
}
