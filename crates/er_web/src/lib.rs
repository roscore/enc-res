//! Enc Res browser UI: wires static index.html markup to er_core and draws the plots on a 2D canvas.

use er_core::*;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d as Ctx, Document, Element, Event, HtmlCanvasElement, HtmlElement};

struct App {
    d: Design,
    /// Last valid design and its report; invalid input keeps showing this.
    good: Option<(Design, Report)>,
    pin: Option<(Design, Report)>,
    en: bool,
    rpm: bool,
    /// Hover position as a fraction of the plot width.
    hover: Option<f64>,
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App { d: Design::default(), good: None, pin: None, en: false, rpm: false, hover: None });
}

const PAD_L: f64 = 58.0;
const PAD_R: f64 = 14.0;

fn doc() -> Option<Document> {
    web_sys::window()?.document()
}
fn el(id: &str) -> Option<Element> {
    doc()?.get_element_by_id(id)
}
fn set_text(id: &str, s: &str) {
    if let Some(e) = el(id) {
        e.set_text_content(Some(s));
    }
}
/// Only ever called with app-generated markup (labels + formatted numbers), never raw user text.
fn set_html(id: &str, s: &str) {
    if let Some(e) = el(id) {
        e.set_inner_html(s);
    }
}
fn val(id: &str) -> String {
    el(id).and_then(|e| js_sys::Reflect::get(&e, &"value".into()).ok()).and_then(|v| v.as_string()).unwrap_or_default()
}
fn set_val(id: &str, s: &str) {
    if let Some(e) = el(id) {
        let _ = js_sys::Reflect::set(&e, &"value".into(), &s.into());
    }
}
fn tr<'a>(en: bool, ko: &'a str, eng: &'a str) -> &'a str {
    if en { eng } else { ko }
}
fn on(target: &web_sys::EventTarget, ev: &str, f: impl FnMut(Event) + 'static) {
    let c = Closure::<dyn FnMut(Event)>::new(f);
    let _ = target.add_event_listener_with_callback(ev, c.as_ref().unchecked_ref());
    c.forget();
}

#[wasm_bindgen(start)]
pub fn start() {
    let Some(d) = doc() else { return };
    // Remember the Korean text of every translatable node so the toggle can swap back.
    if let Ok(list) = d.query_selector_all("[data-en]") {
        for i in 0..list.length() {
            if let Some(e) = list.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
                let _ = e.set_attribute("data-ko", &e.text_content().unwrap_or_default());
            }
        }
    }
    APP.with(|a| {
        let mut a = a.borrow_mut();
        write_inputs(&a);
        recompute(&mut a);
    });
    on(&d, "input", handle_input);
    on(&d, "change", handle_input);
    on(&d, "click", handle_click);
    if let Some(w) = web_sys::window() {
        on(&w, "resize", |_| APP.with(|a| draw(&a.borrow())));
    }
    if let Some(cv) = el("plot") {
        on(&cv, "mousemove", |e| {
            let Some(m) = e.dyn_ref::<web_sys::MouseEvent>() else { return };
            let w = el("plot").map(|c| c.client_width() as f64).unwrap_or(1.0);
            let f = (m.offset_x() as f64 - PAD_L) / (w - PAD_L - PAD_R).max(1.0);
            set_hover(if (0.0..=1.0).contains(&f) { Some(f) } else { None });
        });
        on(&cv, "mouseleave", |_| set_hover(None));
        on(&cv, "blur", |_| set_hover(None));
        on(&cv, "focus", |_| {
            let f = APP.with(|a| work_frac(&a.borrow()));
            set_hover(f);
        });
        on(&cv, "keydown", |e| {
            let Some(k) = e.dyn_ref::<web_sys::KeyboardEvent>() else { return };
            let cur = APP.with(|a| { let a = a.borrow(); a.hover.or(work_frac(&a)) }).unwrap_or(0.5);
            let step = if k.shift_key() { 0.1 } else { 0.01 };
            let next = match k.key().as_str() {
                "ArrowLeft" | "ArrowDown" => Some((cur - step).max(0.0)),
                "ArrowRight" | "ArrowUp" => Some((cur + step).min(1.0)),
                "Home" => Some(0.0),
                "End" => Some(1.0),
                "Escape" => None,
                _ => return,
            };
            e.prevent_default();
            set_hover(next);
        });
    }
}

fn set_hover(h: Option<f64>) {
    APP.with(|a| {
        let mut a = a.borrow_mut();
        a.hover = h;
        draw(&a);
    });
}

fn handle_input(e: Event) {
    let Some(t) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
    let id = t.id();
    APP.with(|a| {
        let mut a = a.borrow_mut();
        match id.as_str() {
            "preset" => {
                if let Some(p) = Preset::ALL.iter().find(|p| p.key() == val("preset")) {
                    a.d = p.design();
                    write_inputs(&a);
                }
            }
            "speed_unit" => {
                // Keep the same physical speed and rewrite the field in the new unit.
                a.rpm = val("speed_unit") == "rpm";
                if a.d.max_speed_dps.is_finite() {
                    set_val("max_speed_dps", &speed_text(a.d.max_speed_dps, a.rpm));
                }
            }
            _ => {}
        }
        read_all(&mut a);
        recompute(&mut a);
    });
}

fn handle_click(e: Event) {
    let Some(t) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else { return };
    let Some(btn) = t.closest("[data-act]").ok().flatten() else { return };
    let act = btn.get_attribute("data-act").unwrap_or_default();
    // Build the CSV inside the borrow, click the download link after releasing it.
    let csv = APP.with(|a| {
        let mut a = a.borrow_mut();
        match act.as_str() {
            "lang" => {
                a.en = !a.en;
                apply_lang(a.en);
                recompute(&mut a);
            }
            "pin" => {
                a.pin = a.good.clone();
                render(&a);
            }
            "unpin" => {
                a.pin = None;
                render(&a);
            }
            "hint" => {
                let _ = el("hint").map(|h| h.set_attribute("hidden", ""));
            }
            "sheet" => {
                let _ = el("settings").map(|s| s.class_list().toggle("open"));
            }
            "csv" => return a.good.as_ref().map(|(d, r)| to_csv(d, r)),
            _ => {}
        }
        None
    });
    if let Some(csv) = csv {
        download(&csv);
    }
}

fn download(csv: &str) -> Option<()> {
    let parts = js_sys::Array::of1(&csv.into());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/csv;charset=utf-8");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).ok()?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
    let a = doc()?.create_element("a").ok()?.dyn_into::<web_sys::HtmlAnchorElement>().ok()?;
    a.set_href(&url);
    a.set_download("enc-res.csv");
    a.click();
    // ponytail: URL not revoked (early revoke can cancel the download); one small blob leaks per export.
    Some(())
}

fn apply_lang(en: bool) {
    let Some(d) = doc() else { return };
    if let Some(h) = d.document_element() {
        let _ = h.set_attribute("lang", if en { "en" } else { "ko" });
    }
    if let Ok(list) = d.query_selector_all("[data-en]") {
        for i in 0..list.length() {
            if let Some(e) = list.item(i).and_then(|n| n.dyn_into::<Element>().ok()) {
                let s = e.get_attribute(if en { "data-en" } else { "data-ko" }).unwrap_or_default();
                e.set_text_content(Some(&s));
            }
        }
    }
}

fn speed_text(dps: f64, rpm: bool) -> String {
    format!("{}", if rpm { dps / 6.0 } else { dps })
}

fn write_inputs(a: &App) {
    set_val("kind", if a.d.kind == Kind::Absolute { "absolute" } else { "incremental" });
    set_val("mount", if a.d.mount == Mount::Output { "output" } else { "motor" });
    for (id, ..) in FIELDS {
        let v = a.d.get(id);
        set_val(id, &if id == "max_speed_dps" { speed_text(v, a.rpm) } else { format!("{v}") });
    }
}

fn read_all(a: &mut App) {
    a.d.kind = if val("kind") == "absolute" { Kind::Absolute } else { Kind::Incremental };
    a.d.mount = if val("mount") == "output" { Mount::Output } else { Mount::Motor };
    a.rpm = val("speed_unit") == "rpm";
    for (id, ..) in FIELDS {
        // Empty, text, "inf" and "NaN" all end up non-finite or out of range and are flagged by invalid().
        let mut v = val(id).trim().parse::<f64>().unwrap_or(f64::NAN);
        if id == "max_speed_dps" && a.rpm {
            v *= 6.0;
        }
        a.d.set(id, v);
    }
}

fn recompute(a: &mut App) {
    let inv = a.d.invalid();
    for (id, lo, hi) in FIELDS {
        let bad = inv.contains(&id);
        let (lo, hi) = if id == "max_speed_dps" && a.rpm { (lo / 6.0, hi / 6.0) } else { (lo, hi) };
        let msg = match (bad, id) {
            (false, _) => String::new(),
            (true, "decode") => tr(a.en, "1, 2, 4 중 하나", "Use 1, 2 or 4").into(),
            (true, "bits") => tr(a.en, "1–32 사이 정수", "Whole number 1–32").into(),
            (true, _) => format!("{} {}–{}", tr(a.en, "숫자 범위", "Enter a number"), fmt(lo), fmt(hi)),
        };
        set_text(&format!("m-{id}"), &msg);
        if let Some(e) = el(id) {
            let _ = if bad { e.set_attribute("aria-invalid", "true") } else { e.remove_attribute("aria-invalid") };
        }
    }
    if let Some(b) = doc().and_then(|d| d.body()) {
        let _ = b.set_attribute("data-kind", if a.d.kind == Kind::Absolute { "absolute" } else { "incremental" });
    }
    if inv.is_empty() {
        a.good = Some((a.d.clone(), a.d.report()));
    }
    set_text("state", if inv.is_empty() { "" } else { tr(a.en, "입력값을 확인하세요 — 마지막 유효 결과를 표시합니다.", "Check the highlighted inputs — showing the last valid result.") });
    render(a);
}

fn chip(en: bool, s: Status) -> String {
    let t = match s {
        Status::Pass => tr(en, "통과", "PASS"),
        Status::Warn => tr(en, "주의", "WARN"),
        Status::Fail => tr(en, "실패", "FAIL"),
    };
    format!("<span class=\"chip {}\">{}</span>", s.key(), t)
}

fn ko_label(key: &str) -> &'static str {
    match key {
        "counts_enc" => "엔코더 1회전당 카운트",
        "counts_out" => "출력 1회전당 카운트",
        "res_deg" | "res_arcsec" | "res_mrad" => "출력 각도 분해능",
        "tip_step" => "카운트당 툴팁 이동",
        "noise_tip" => "툴팁 노이즈(보간)",
        "backlash_tip" => "백래시 툴팁 유격",
        "quantum" => "M법 속도 양자",
        "min_speed" => "샘플당 1카운트 최소 속도",
        "crossover" => "이 속도 이하 T법 권장",
        "m_err" => "작업 속도 M법 오차",
        "t_err" => "작업 속도 T법 오차",
        "t_period" => "작업 속도 에지 간격",
        "edge_hz" => "최대 속도 카운트 주파수",
        "shaft_rpm" => "최대 속도 엔코더 축 속도",
        "rev_sample" => "샘플당 엔코더 회전",
        "util" => "디코더 사용률",
        _ => "목표 오차",
    }
}

fn render(a: &App) {
    let Some((d, r)) = &a.good else { return };
    let en = a.en;
    let hud = [
        (tr(en, "분해능", "Resolution"), format!("{} ″", fmt(r.res_arcsec)), format!("{} °", fmt(r.res_deg))),
        (tr(en, "툴팁 스텝", "Tip step"), format!("{} µm", fmt(r.tip_step_um)), format!("L = {} mm", fmt(d.arm_mm))),
        (tr(en, "속도 양자", "Velocity quantum"), format!("{} °/s", fmt(r.quantum_dps)), format!("@ {} Hz", fmt(d.rate_hz))),
        (tr(en, "M법 오차", "M-method error"), format!("{} % {}", fmt(r.m_err_pct), chip(en, r.m_status)), format!("@ {} °/s", fmt(d.work_speed_dps))),
        (tr(en, "카운트 속도", "Count rate"), format!("{} {}", match r.count_util_pct { Some(u) => format!("{} %", fmt(u)), None => format!("{} rev/sample", fmt(r.rev_per_sample)) }, chip(en, r.count_status)), format!("{} Hz", fmt(r.edge_hz))),
    ];
    set_html("hud", &hud.iter().map(|(k, v, s)| format!("<div class=\"hud-i\"><span>{k}</span><b>{v}</b><small>{s}</small></div>")).collect::<String>());

    let (tip, bl) = (fmt(r.tip_step_um), fmt(r.backlash_tip_um));
    let dom = match r.dominant {
        Dominant::QuantOnly => format!("{} {tip} µm.", tr(en, "백래시 0: 툴팁 오차는 카운트 양자화만 남습니다 —", "No backlash entered: tip error is count quantization only —")),
        Dominant::Quantization => format!("{} ({tip} µm > {bl} µm)", tr(en, "엔코더 양자화가 지배합니다: 카운트당 팁 스텝이 백래시 유격보다 큽니다.", "Encoder quantization dominates: the per-count tip step exceeds the backlash play.")),
        Dominant::Backlash => format!("{} ({bl} µm ≈ {}× {tip} µm)", tr(en, "기어 백래시가 지배합니다: 엔코더 해상도를 올려도 팁 정밀도는 거의 나아지지 않습니다.", "Gearbox backlash dominates: a finer encoder barely improves tip precision."), fmt(r.backlash_tip_um / r.tip_step_um)),
    };
    let tnote = format!(
        "{} {} °/s. {} {} ms{}. {} {} % {}",
        tr(en, "M/T 교차 속도: 이 속도 이하에서는 주기(T법) 측정이 더 정확합니다 —", "M/T crossover: below this speed period (T-method) measurement is more accurate —"),
        fmt(r.crossover_dps),
        tr(en, "작업 속도에서 에지 간격", "Edge period at working speed"),
        fmt(r.t_period_ms),
        if r.t_slow { tr(en, " (제어 주기보다 김 → 측정 갱신 지연)", " (longer than the control period → stale updates)") } else { "" },
        tr(en, "T법 오차", "T-method error"),
        fmt(r.t_err_pct),
        chip(en, r.t_status),
    );
    let cnote = match r.count_util_pct {
        Some(u) => format!("{} {} Hz / {} Hz = {} % {}", tr(en, "최대 속도 에지 주파수 대 디코더 한계 (80 % 초과 시 주의):", "Edge frequency at max speed vs decoder limit (warn above 80 %):"), fmt(r.edge_hz), fmt(d.decoder_limit_hz), fmt(u), chip(en, r.count_status)),
        None => format!("{} {} rev {}", tr(en, "절대형: 프레임(SPI/BiSS 등)을 제어 주기마다 1회 읽는다고 가정. 샘플당 엔코더 회전", "Absolute: assumes one frame (SPI/BiSS etc.) per control sample; frame rate must be ≥ the control rate. Encoder revolutions per sample"), fmt(r.rev_per_sample), chip(en, r.count_status)),
    };
    set_text("dominant", &dom);
    set_html("tnote", &tnote);
    set_html("cnote", &cnote);

    let rows_html: String = rows(d, r)
        .into_iter()
        .map(|(k, l, v, u)| format!("<tr><th scope=\"row\">{}</th><td>{}</td><td>{u}</td></tr>", if en { l } else { ko_label(k) }, fmt(v)))
        .collect();
    set_html("rows", &rows_html);

    let cmp = match &a.pin {
        None => String::new(),
        Some((_, p)) => {
            let delta = |live: f64, old: f64, unit: &str| {
                let pct = if old != 0.0 { format!(" ({:+.1} %)", (live - old) / old * 100.0) } else { String::new() };
                format!("{} → {} {unit}{pct}", fmt(old), fmt(live))
            };
            format!(
                "<b>{}</b><span>Δ {}: {}</span><span>Δ {}: {}</span><span>Δ {}: {}</span>",
                tr(en, "고정 설계 대비", "vs pinned design"),
                tr(en, "툴팁 스텝", "tip step"),
                delta(r.tip_step_um, p.tip_step_um, "µm"),
                tr(en, "최소 속도", "min speed"),
                delta(r.min_speed_dps, p.min_speed_dps, "°/s"),
                tr(en, "M법 오차", "M error"),
                delta(r.m_err_pct, p.m_err_pct, "%"),
            )
        }
    };
    set_html("compare", &cmp);
    if let Some(e) = el("unpin") {
        let _ = if a.pin.is_some() { e.remove_attribute("hidden") } else { e.set_attribute("hidden", "") };
    }
    draw(a);
}

/// Plot x range as log10 exponents, covering the quantum, working and max speeds.
fn xrange(d: &Design, r: &Report) -> (f64, f64) {
    let lo = (r.quantum_dps.min(d.work_speed_dps) / 10.0).log10().floor().max(-6.0).min(3.0);
    let hi = d.max_speed_dps.max(d.work_speed_dps).max(r.quantum_dps * 10.0).log10().ceil();
    (lo, hi.max(lo + 2.0).min(lo + 9.0))
}

fn work_frac(a: &App) -> Option<f64> {
    let (d, r) = a.good.as_ref()?;
    let (lo, hi) = xrange(d, r);
    Some(((d.work_speed_dps.log10() - lo) / (hi - lo)).max(0.0).min(1.0))
}

struct Area {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    lx: (f64, f64),
    ly: (f64, f64),
}

impl Area {
    fn px(&self, v: f64) -> f64 {
        self.x + (v.log10() - self.lx.0) / (self.lx.1 - self.lx.0) * self.w
    }
    /// Zero/negative values sit on the bottom edge.
    fn py(&self, v: f64) -> f64 {
        if v <= 0.0 {
            return self.y + self.h;
        }
        let f = ((v.log10() - self.ly.0) / (self.ly.1 - self.ly.0)).max(0.0).min(1.0);
        self.y + self.h - f * self.h
    }
    fn speed(&self, i: f64) -> f64 {
        10f64.powf(self.lx.0 + (self.lx.1 - self.lx.0) * i / self.w)
    }
    /// Per-pixel-column polyline of f(speed); bounded by the canvas width, never by count numbers.
    fn curve(&self, c: &Ctx, color: &str, width: f64, f: impl Fn(f64) -> f64) {
        c.begin_path();
        c.set_stroke_style_str(color);
        c.set_line_width(width);
        let n = self.w.max(1.0) as usize;
        for i in 0..=n {
            let x = self.x + i as f64;
            let y = self.py(f(self.speed(i as f64)));
            if i == 0 { c.move_to(x, y) } else { c.line_to(x, y) }
        }
        c.stroke();
    }
    fn grid(&self, c: &Ctx, ylab: &str, xlab: Option<&str>) {
        c.set_stroke_style_str("rgba(255,255,255,0.08)");
        c.set_fill_style_str("#8b95a7");
        c.set_line_width(1.0);
        c.set_font("11px system-ui, sans-serif");
        for k in (self.lx.0 as i32)..=(self.lx.1 as i32) {
            let x = self.px(10f64.powi(k));
            line(c, x, self.y, x, self.y + self.h);
            if xlab.is_some() {
                let _ = c.fill_text(&fmt(10f64.powi(k)), x - 10.0, self.y + self.h + 14.0);
            }
        }
        for k in (self.ly.0 as i32)..=(self.ly.1 as i32) {
            let y = self.py(10f64.powi(k));
            line(c, self.x, y, self.x + self.w, y);
            let _ = c.fill_text(&fmt(10f64.powi(k)), 4.0, y + 4.0);
        }
        let _ = c.fill_text(ylab, self.x + 6.0, self.y + 12.0);
        if let Some(l) = xlab {
            let _ = c.fill_text(l, self.x + self.w - 150.0, self.y + self.h + 28.0);
        }
        c.set_stroke_style_str("rgba(255,255,255,0.25)");
        c.stroke_rect(self.x, self.y, self.w, self.h);
    }
    fn vline(&self, c: &Ctx, v: f64, color: &str, dash: bool, label: &str) {
        let x = self.px(v);
        if !(self.x..=self.x + self.w).contains(&x) {
            return;
        }
        c.set_stroke_style_str(color);
        c.set_fill_style_str(color);
        c.set_line_width(1.0);
        let _ = c.set_line_dash(&js_sys::Array::of2(&4.0.into(), &4.0.into()));
        if !dash {
            let _ = c.set_line_dash(&js_sys::Array::new());
        }
        line(c, x, self.y, x, self.y + self.h);
        let _ = c.set_line_dash(&js_sys::Array::new());
        let _ = c.fill_text(label, x + 4.0, self.y + self.h - 6.0);
    }
}

fn line(c: &Ctx, x0: f64, y0: f64, x1: f64, y1: f64) {
    c.begin_path();
    c.move_to(x0, y0);
    c.line_to(x1, y1);
    c.stroke();
}

fn draw(a: &App) {
    let Some(cv) = el("plot").and_then(|e| e.dyn_into::<HtmlCanvasElement>().ok()) else { return };
    let dpr = web_sys::window().map(|w| w.device_pixel_ratio()).unwrap_or(1.0).max(1.0).min(3.0);
    let (w, h) = ((cv.client_width() as f64).max(280.0), (cv.client_height() as f64).max(260.0));
    cv.set_width((w * dpr) as u32);
    cv.set_height((h * dpr) as u32);
    let Some(c) = cv.get_context("2d").ok().flatten().and_then(|c| c.dyn_into::<Ctx>().ok()) else { return };
    let _ = c.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    c.clear_rect(0.0, 0.0, w, h);
    let Some((d, r)) = &a.good else { return };
    let en = a.en;
    let lx = xrange(d, r);
    let pw = w - PAD_L - PAD_R;
    let top = Area { x: PAD_L, y: 10.0, w: pw, h: h * 0.6 - 26.0, lx, ly: lx };
    let ely = ((d.target_pct.log10() - 1.0).floor().min(-2.0), 2.0);
    let bot = Area { x: PAD_L, y: h * 0.6 + 4.0, w: pw, h: h * 0.4 - 40.0, lx, ly: ely };
    let t = d.target_pct / 100.0;

    // Main plot: target band around the ideal line, ideal, ghost, live staircase.
    top.grid(&c, tr(en, "측정 속도 (°/s, M법)", "measured speed (°/s, M-method)"), None);
    c.begin_path();
    c.set_fill_style_str("rgba(108,182,255,0.12)");
    let n = top.w.max(1.0) as usize;
    for i in 0..=n {
        let v = top.speed(i as f64);
        c.line_to(top.x + i as f64, top.py(v * (1.0 + t)));
    }
    for i in (0..=n).rev() {
        let v = top.speed(i as f64);
        c.line_to(top.x + i as f64, top.py(v * (1.0 - t)));
    }
    c.fill();
    top.curve(&c, "#9aa4b2", 1.0, |v| v);
    if let Some((_, p)) = &a.pin {
        top.curve(&c, "rgba(160,168,180,0.45)", 2.0, |v| m_measured(p.quantum_dps, v));
    }
    top.curve(&c, "#ffb454", 2.0, |v| m_measured(r.quantum_dps, v));
    top.vline(&c, r.min_speed_dps, "#c792ea", false, tr(en, "1카운트/샘플", "1 count/sample"));
    top.vline(&c, d.work_speed_dps, "#7fdbca", true, tr(en, "작업 속도", "working speed"));

    // Error plot: worst-case envelope (with noise), actual truncation sawtooth, target line.
    bot.grid(&c, tr(en, "상대 오차 %", "relative error %"), Some(tr(en, "실제 관절 속도 (°/s)", "true joint speed (°/s)")));
    c.set_fill_style_str("rgba(126,231,135,0.08)");
    let ty = bot.py(d.target_pct);
    c.fill_rect(bot.x, ty, bot.w, bot.y + bot.h - ty);
    c.set_stroke_style_str("#7ee787");
    line(&c, bot.x, ty, bot.x + bot.w, ty);
    if let Some((_, p)) = &a.pin {
        bot.curve(&c, "rgba(160,168,180,0.45)", 2.0, |v| m_error_pct(p.quantum_eff_dps, v));
    }
    bot.curve(&c, "rgba(255,180,84,0.35)", 1.0, |v| (v - m_measured(r.quantum_dps, v)) / v * 100.0);
    bot.curve(&c, "#ffb454", 2.0, |v| m_error_pct(r.quantum_eff_dps, v));
    bot.vline(&c, d.work_speed_dps, "#7fdbca", true, "");

    dial(&c, top.x + 60.0, top.y + 66.0, 44.0, d, r, en);

    // Hover / keyboard readout.
    let tip = el("tip").and_then(|e| e.dyn_into::<HtmlElement>().ok());
    match (a.hover, tip) {
        (Some(f), Some(tipel)) => {
            let v = top.speed(f * top.w);
            let m = m_measured(r.quantum_dps, v);
            let x = top.x + f * top.w;
            c.set_stroke_style_str("rgba(255,255,255,0.6)");
            line(&c, x, top.y, x, bot.y + bot.h);
            c.begin_path();
            c.set_fill_style_str("#ffb454");
            let _ = c.arc(x, top.py(m), 4.0, 0.0, std::f64::consts::TAU);
            c.fill();
            tipel.set_text_content(Some(&format!(
                "{} {} °/s · {} {} °/s · {} {} %",
                tr(en, "실제", "true"),
                fmt(v),
                tr(en, "측정", "measured"),
                fmt(m),
                tr(en, "오차", "error"),
                fmt((v - m) / v * 100.0)
            )));
            let s = tipel.style();
            let _ = s.set_property("left", &format!("{}px", x.min(w - 230.0).max(0.0)));
            let _ = tipel.remove_attribute("hidden");
        }
        (_, Some(tipel)) => {
            let _ = tipel.set_attribute("hidden", "");
        }
        _ => {}
    }
}

/// One output revolution as a dial, zoomed so a handful of counts show as ticks; the highlighted
/// arc is the joint travel per control sample at working speed.
fn dial(c: &Ctx, cx: f64, cy: f64, rad: f64, d: &Design, r: &Report, en: bool) {
    let ticks = r.counts_out.min(24.0).max(1.0).floor();
    let span = ticks * r.res_deg;
    let (arc, zoom) = if span >= 360.0 { (360.0, 1.0) } else { (150.0, 150.0 / span) };
    let a0 = -90.0 - arc / 2.0;
    let rad_of = |deg: f64| deg.to_radians();
    c.set_fill_style_str("rgba(12,16,24,0.85)");
    c.begin_path();
    let _ = c.arc(cx, cy, rad + 12.0, 0.0, std::f64::consts::TAU);
    c.fill();
    c.set_stroke_style_str("rgba(255,255,255,0.3)");
    c.set_line_width(1.0);
    c.begin_path();
    let _ = c.arc(cx, cy, rad, rad_of(a0), rad_of(a0 + arc));
    c.stroke();
    let k = ticks as usize;
    for i in 0..=k {
        let ang = rad_of(a0 + arc * i as f64 / ticks);
        let (s, co) = ang.sin_cos();
        line(c, cx + co * (rad - 6.0), cy + s * (rad - 6.0), cx + co * rad, cy + s * rad);
    }
    // Travel per sample at working speed, in the same zoomed scale.
    let travel = (d.work_speed_dps / d.rate_hz * zoom).min(arc);
    c.set_stroke_style_str("#7fdbca");
    c.set_line_width(3.0);
    c.begin_path();
    let _ = c.arc(cx, cy, rad - 12.0, rad_of(a0), rad_of(a0 + travel));
    c.stroke();
    c.set_line_width(1.0);
    c.set_fill_style_str("#c9d1d9");
    c.set_font("10px system-ui, sans-serif");
    let _ = c.fill_text(&format!("×{}", fmt(zoom)), cx - 16.0, cy + 4.0);
    let _ = c.fill_text(&format!("{} {}", k, tr(en, "카운트", "counts")), cx - 22.0, cy + 18.0);
}
