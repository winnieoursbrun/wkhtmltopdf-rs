//! The `__wk` object: natives the prelude builds the browser globals on
//! (console, dialogs, timers, CSSOM, canvas, XHR, URL resolution).

use boa_engine::object::FunctionObjectBuilder;
use boa_engine::object::builtins::JsArray;
use boa_engine::{Context, JsArgs, JsObject, JsResult, JsString, JsValue, NativeFunction};

use crate::canvas;
use crate::dom::{Fp, arg_str, get_attr, id_of, opt_id, s, set_attr};
use crate::host::{self, console_location, services, warning, with_host};

/// `MyQWebPage::javaScriptConsoleMessage`: with `--debug-javascript`,
/// `Warning: <sourceURL>:<line> <message>` (first argument only, like
/// QtWebKit 2.2).
fn wk_console(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    if !with_host(|h| h.debug_js) {
        return Ok(JsValue::undefined());
    }
    let msg = arg_str(args, 0, ctx)?;
    let (url, line) = console_location(ctx);
    warning(&format!("{url}:{line} {msg}"));
    Ok(JsValue::undefined())
}

/// `javaScriptAlert`/`Confirm`/`Prompt`, printed at warning level by the
/// 0.12.6 binary; confirm answers yes, prompt answers an empty string.
fn wk_dialog(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let kind = arg_str(args, 0, ctx)?;
    let msg = arg_str(args, 1, ctx)?;
    let def = arg_str(args, 2, ctx)?;
    match kind.as_str() {
        "alert" => warning(&format!("Javascript alert: {msg}")),
        "confirm" => warning(&format!("Javascript confirm: {msg} (answered yes)")),
        _ => warning(&format!("Javascript prompt: {msg} (answered {def})")),
    }
    Ok(JsValue::undefined())
}

fn wk_add_timer(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let cb = args.get_or_undefined(0).clone();
    let ms = args.get_or_undefined(1).to_number(ctx)?;
    let repeat = args.get_or_undefined(2).to_boolean();
    let mut extra = Vec::new();
    if let Some(o) = args.get_or_undefined(3).as_object() {
        let arr = JsArray::from_object(o.clone())?;
        for i in 0..arr.length(ctx)? {
            extra.push(arr.get(i, ctx)?);
        }
    }
    Ok(host::add_timer(cb, ms, repeat, extra).into())
}

fn wk_clear_timer(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = args.get_or_undefined(0).to_number(ctx)?;
    if id.is_finite() && id > 0.0 {
        host::clear_timer(id as u32);
    }
    Ok(JsValue::undefined())
}

fn wk_now(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    Ok(with_host(|h| h.now()).into())
}

fn wk_print(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_host(|h| h.print_called = true);
    Ok(JsValue::undefined())
}

fn style_attr(id: blitz_dom::NodeId) -> String {
    with_host(|h| get_attr(h, id, "style")).unwrap_or_default()
}

fn wk_style_get(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    let prop = arg_str(args, 1, ctx)?;
    let attr = style_attr(id);
    Ok(s(&with_host(|h| h.doc.style_attr_get_property(&attr, &prop))))
}

fn wk_style_set(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    let prop = arg_str(args, 1, ctx)?;
    let value = arg_str(args, 2, ctx)?;
    let important = args.get_or_undefined(3).to_boolean();
    let attr = style_attr(id);
    with_host(|h| {
        // The style attribute is the single source of truth: serialisation
        // and restyle both see the change. Invalid declarations are ignored.
        // An empty value removes the property.
        if let Some(new) = h.doc.style_attr_set_property(&attr, &prop, &value, important) {
            if new != attr {
                set_attr(h, id, "style", &new);
            }
        }
    });
    Ok(JsValue::undefined())
}

fn wk_style_known(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let prop = arg_str(args, 0, ctx)?;
    Ok(blitz_dom::css_property_is_supported(&prop).into())
}

fn wk_computed(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    let prop = arg_str(args, 1, ctx)?;
    Ok(s(&with_host(|h| {
        if !crate::dom::is_connected_in(h, id) {
            return String::new();
        }
        h.ensure_layout();
        h.doc.resolved_style_value(id, &prop)
    })))
}

fn wk_media_matches(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let q = arg_str(args, 0, ctx)?.to_ascii_lowercase();
    let print = with_host(|h| h.media_print);
    let r = if q.contains("print") {
        print && !q.contains("not print")
    } else if q.contains("screen") {
        !print && !q.contains("not screen")
    } else {
        true
    };
    Ok(r.into())
}

fn wk_force_layout(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    with_host(|h| h.ensure_layout());
    Ok(JsValue::undefined())
}

/// Resolve a URL against the document base (`a.href`, `img.src`, XHR).
fn wk_resolve(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let rel = arg_str(args, 0, ctx)?;
    Ok(with_host(|h| h.doc.base_url().join(&rel).map(|u| s(u.as_str())).unwrap_or_else(|_| s(&rel))))
}

/// Synchronous fetch through the loader (XMLHttpRequest): `[ok, body]`.
/// Each request is accounted for like a sub-resource (`amfinished`).
fn wk_fetch(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let rel = arg_str(args, 0, ctx)?;
    let url = with_host(|h| h.doc.base_url().join(&rel).ok());
    let body = url.and_then(|u| services(|svc| svc.fetch(&u)).flatten());
    let (ok, text) = match body {
        Some(b) => (true, String::from_utf8_lossy(&b).into_owned()),
        None => (false, String::new()),
    };
    Ok(JsArray::from_iter([JsValue::from(ok), s(&text)], ctx).into())
}

/// Compile an inline event handler attribute into a function, attributed
/// to the element's line (`on<type>="…"`).
fn wk_compile_handler(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let code = arg_str(args, 0, ctx)?;
    let line = opt_id(args.get_or_undefined(1))
        .and_then(|id| with_host(|h| h.element_lines.get(&id).copied()))
        .unwrap_or(1);
    let url = with_host(|h| h.url.to_string());
    let src = format!("(function (event) {{{code}\n}})");
    Ok(host::eval(ctx, &src, &url, line).unwrap_or(JsValue::undefined()))
}

/// Queue the completion of an asynchronous request.
fn wk_network_task(_: &JsValue, args: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let f = args.get_or_undefined(0).clone();
    with_host(|h| h.network_tasks.push(f));
    Ok(JsValue::undefined())
}

/// `document.currentScript`.
fn wk_current_script(_: &JsValue, _: &[JsValue], _: &mut Context) -> JsResult<JsValue> {
    let id = with_host(|h| h.current_script.last().copied());
    Ok(id.map(crate::dom::wrap).unwrap_or(JsValue::null()))
}

/// Report an exception caught in an event listener (it does not propagate).
fn wk_report_exception(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let err = boa_engine::JsError::from_opaque(args.get_or_undefined(0).clone());
    host::report_error(&err, ctx);
    Ok(JsValue::undefined())
}

fn wk_canvas_size(_: &JsValue, args: &[JsValue], ctx: &mut Context) -> JsResult<JsValue> {
    let id = id_of(args.get_or_undefined(0))?;
    let (w, h) = with_host(|h| canvas::canvas_size(h, id));
    Ok(JsArray::from_iter([JsValue::from(w), JsValue::from(h)], ctx).into())
}

pub(crate) fn make_wk(
    ctx: &mut Context,
    protos: &crate::dom::Protos,
    url: &str,
    viewport: (u32, u32),
    ua: &str,
) -> JsResult<JsObject> {
    let wk = JsObject::with_object_proto(ctx.intrinsics());
    let natives: &[(&str, Fp)] = &[
        ("console", wk_console),
        ("dialog", wk_dialog),
        ("addTimer", wk_add_timer),
        ("clearTimer", wk_clear_timer),
        ("now", wk_now),
        ("print", wk_print),
        ("styleGet", wk_style_get),
        ("styleSet", wk_style_set),
        ("styleKnown", wk_style_known),
        ("computed", wk_computed),
        ("mediaMatches", wk_media_matches),
        ("forceLayout", wk_force_layout),
        ("resolve", wk_resolve),
        ("fetch", wk_fetch),
        ("compileHandler", wk_compile_handler),
        ("canvasSize", wk_canvas_size),
        ("currentScript", wk_current_script),
        ("networkTask", wk_network_task),
        ("reportException", wk_report_exception),
        ("canvasFill", canvas::canvas_fill),
        ("canvasStroke", canvas::canvas_stroke),
        ("canvasClip", canvas::canvas_clip),
        ("canvasPopClips", canvas::canvas_pop_clips),
        ("canvasClear", canvas::canvas_clear),
        ("canvasReset", canvas::canvas_reset),
        ("canvasText", canvas::canvas_text),
        ("canvasImage", canvas::canvas_image),
        ("measureText", canvas::measure_text),
        ("imageSize", canvas::image_size),
    ];
    for (n, f) in natives {
        let func = FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(*f))
            .name(JsString::from(*n))
            .build();
        wk.set(JsString::from(*n), func, false, ctx)?;
    }
    let p = JsObject::with_object_proto(ctx.intrinsics());
    let mut names: Vec<_> = protos.by_name.iter().collect();
    names.sort_by_key(|(n, _)| **n);
    for (n, o) in names {
        p.set(JsString::from(*n), o.clone(), false, ctx)?;
    }
    wk.set(JsString::from("protos"), p, false, ctx)?;
    wk.set(JsString::from("url"), s(url), false, ctx)?;
    wk.set(JsString::from("userAgent"), s(ua), false, ctx)?;
    wk.set(JsString::from("viewportWidth"), viewport.0, false, ctx)?;
    wk.set(JsString::from("viewportHeight"), viewport.1, false, ctx)?;
    wk.set(JsString::from("readyState"), s("loading"), false, ctx)?;
    Ok(wk)
}
