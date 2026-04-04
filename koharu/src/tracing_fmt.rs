use std::time::{Duration, Instant};

use console::{Style, Term, measure_text_width, style};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

// ---------------------------------------------------------------------------
// Tree
// ---------------------------------------------------------------------------

enum Node {
    Span {
        name: &'static str,
        fields: String,
        duration: Duration,
        offset: Duration, // offset from parent start
        children: Vec<Node>,
    },
    Event {
        level: Level,
        message: String,
    },
}

struct SpanState {
    name: &'static str,
    fields: String,
    start: Instant,
    children: Vec<Node>,
}

// ---------------------------------------------------------------------------
// Field collector
// ---------------------------------------------------------------------------

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(&format!("{value:?}"));
        } else {
            if !self.0.is_empty() {
                self.0.push_str("  ");
            }
            self.0.push_str(&format!("{}={value:?}", field.name()));
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(value);
        } else {
            if !self.0.is_empty() {
                self.0.push_str("  ");
            }
            self.0.push_str(&format!("{}={value}", field.name()));
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if !self.0.is_empty() {
            self.0.push_str("  ");
        }
        self.0.push_str(&format!("{}={value}", field.name()));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        if !self.0.is_empty() {
            self.0.push_str("  ");
        }
        self.0.push_str(&format!("{}={value}", field.name()));
    }
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

fn fmt_dur(d: Duration) -> String {
    let us = d.as_micros();
    if us < 1000 {
        return format!("{us}µs");
    }
    let ms = d.as_millis();
    if ms < 1000 {
        return format!("{ms}ms");
    }
    format!("{:.2}s", d.as_secs_f64())
}

fn dur_style(d: Duration) -> Style {
    let ms = d.as_millis();
    if ms < 50 {
        Style::new().dim()
    } else if ms < 200 {
        Style::new().green()
    } else if ms < 1000 {
        Style::new().yellow()
    } else {
        Style::new().red().bold()
    }
}

/// Jaeger-style bar: offset spaces, then filled block, then empty
fn make_bar(offset_ratio: f64, width_ratio: f64, bar_w: usize, ds: &Style) -> String {
    let off = (offset_ratio * bar_w as f64).round() as usize;
    let filled = (width_ratio * bar_w as f64).round().max(1.0) as usize;
    let off = off.min(bar_w);
    let filled = filled.min(bar_w.saturating_sub(off));
    let empty = bar_w.saturating_sub(off + filled);
    format!(
        "{}{}{}",
        style("░".repeat(off)).dim(),
        ds.apply_to("█".repeat(filled)),
        style("░".repeat(empty)).dim(),
    )
}

fn tw() -> usize {
    let w = Term::stdout().size().1 as usize;
    if w > 20 { w } else { 120 }
}

fn truncate(s: &str, max: usize) -> String {
    if measure_text_width(s) <= max {
        return s.to_string();
    }
    if max <= 2 {
        return ".".repeat(max);
    }
    let b = s.floor_char_boundary(max.saturating_sub(2));
    format!("{}..", &s[..b])
}

// ---------------------------------------------------------------------------
// Gutter — miette-style smooth left border
// ---------------------------------------------------------------------------

fn gutter_color(depth: usize) -> Style {
    match depth % 6 {
        0 => Style::new().cyan(),
        1 => Style::new().blue(),
        2 => Style::new().magenta(),
        3 => Style::new().green(),
        4 => Style::new().yellow(),
        _ => Style::new().red(),
    }
}

/// Gutter for a span line
fn gutter_span(depth: usize) -> String {
    if depth == 0 {
        return format!("{} ", style("▶").cyan().bold());
    }
    let mut s = String::new();
    let c = gutter_color(depth - 1);
    for d in 0..depth - 1 {
        s.push_str(&format!("{} ", gutter_color(d).apply_to("│")));
    }
    s.push_str(&format!("{} ", c.apply_to("├")));
    s
}

/// Gutter for continuation/event lines: "│ " at every level
fn gutter_cont(depth: usize) -> String {
    let mut s = String::new();
    for d in 0..depth {
        s.push_str(&format!("{} ", gutter_color(d).apply_to("│")));
    }
    s
}

/// Visible char width of gutter_span
fn gutter_vis(depth: usize) -> usize {
    // depth 0: "▶ " = 2, depth N: N * 2
    depth.max(1) * 2
}

// ---------------------------------------------------------------------------
// Layout
//
// Fixed layout:
//   [gutter][name + fields + dots] [bar] [dur] [pct]
//
// Name gets 50% of available, bar gets the rest.
// Bar always starts at the same column for all depths by
// using a fixed name column (independent of depth).
// ---------------------------------------------------------------------------

const DUR_W: usize = 8;
const PCT_W: usize = 5;

fn print_span(
    name: &str,
    fields: &str,
    duration: Duration,
    offset: Duration,
    root_dur: Duration,
    depth: usize,
    children: &[Node],
) {
    let w = tw();
    let ds = dur_style(duration);
    let ratio = if root_dur.as_nanos() > 0 {
        duration.as_nanos() as f64 / root_dur.as_nanos() as f64
    } else {
        0.0
    };

    let g = gutter_span(depth);
    let gw = gutter_vis(depth);

    // Bar column starts at a fixed position: 40% of terminal
    let bar_start = w * 2 / 5;
    // Name area = bar_start - gutter
    let name_area = bar_start.saturating_sub(gw + 1);
    // Bar area = rest - dur - pct - spaces
    let bar_w = w.saturating_sub(bar_start + 1 + DUR_W + 1 + PCT_W).max(6);

    // Build plain label, strictly limited to name_area visible chars
    let name_trunc = truncate(name, name_area);
    let name_w = measure_text_width(&name_trunc);

    let (plain_label, styled_label) = if fields.is_empty() || name_w + 2 >= name_area {
        (name_trunc.clone(), format!("{}", style(&name_trunc).bold()))
    } else {
        let fields_max = name_area - name_w - 2;
        let fields_trunc = truncate(fields, fields_max);
        let plain = format!("{}  {}", name_trunc, fields_trunc);
        let styled = format!(
            "{}  {}",
            style(&name_trunc).bold(),
            style(&fields_trunc).dim()
        );
        (plain, styled)
    };

    // Fill: gutter(gw) + label(vis) + fill = bar_start
    let vis = measure_text_width(&plain_label);
    let fill_len = bar_start.saturating_sub(gw + vis);
    let fill = if fill_len > 2 {
        format!(" {}.", style(".".repeat(fill_len - 2)).dim())
    } else {
        " ".repeat(fill_len.max(1))
    };

    // Offset ratio within root
    let offset_ratio = if root_dur.as_nanos() > 0 {
        offset.as_nanos() as f64 / root_dur.as_nanos() as f64
    } else {
        0.0
    };

    // Right side
    let bar = make_bar(offset_ratio, ratio, bar_w, &ds);
    let dur_s = ds.apply_to(format!("{:>w$}", fmt_dur(duration), w = DUR_W));
    let pct_s = style(format!("{:>4.0}%", ratio * 100.0)).dim();

    eprintln!("{g}{styled_label}{fill}{bar} {dur_s} {pct_s}");

    print_tree(children, root_dur, depth + 1);
}

fn print_event(level: &Level, message: &str, depth: usize) {
    let g = gutter_cont(depth);
    let icon = match *level {
        Level::ERROR => style("✗").red().bold(),
        Level::WARN => style("⚠").yellow(),
        Level::INFO => style("ℹ").cyan(),
        _ => style("·").dim(),
    };
    eprintln!("{g}{icon} {}", style(message).dim());
}

fn print_tree(nodes: &[Node], root_dur: Duration, depth: usize) {
    for node in nodes {
        match node {
            Node::Span {
                name,
                fields,
                duration,
                offset,
                children,
            } => {
                print_span(name, fields, *duration, *offset, root_dur, depth, children);
            }
            Node::Event { level, message } => {
                print_event(level, message, depth);
            }
        }
    }
}

fn print_root(node: &Node) {
    match node {
        Node::Span {
            name,
            fields,
            duration,
            children,
            ..
        } => {
            eprintln!();
            print_span(
                name,
                fields,
                *duration,
                Duration::ZERO,
                *duration,
                0,
                children,
            );
        }
        Node::Event { level, message } => {
            print_event(level, message, 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Layer
// ---------------------------------------------------------------------------

pub struct TimingLayer;

impl Default for TimingLayer {
    fn default() -> Self {
        Self
    }
}

impl TimingLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for TimingLayer {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = Fields(String::new());
        attrs.record(&mut fields);
        let state = SpanState {
            name: attrs.metadata().name(),
            fields: fields.0,
            start: Instant::now(),
            children: Vec::new(),
        };
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(state);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        let node = Node::Event {
            level: *event.metadata().level(),
            message: fields.0,
        };
        if let Some(span_ref) = ctx.event_span(event)
            && let Some(state) = span_ref.extensions_mut().get_mut::<SpanState>()
        {
            state.children.push(node);
            return;
        }
        print_root(&node);
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span_ref) = ctx.span(&id) else {
            return;
        };
        let state = span_ref.extensions_mut().remove::<SpanState>();
        let Some(state) = state else { return };
        let duration = state.start.elapsed();

        // Calculate offset from root span's start
        let offset = {
            let mut root_start = state.start;
            let mut current = span_ref.parent();
            while let Some(ancestor) = current {
                if let Some(ancestor_state) = ancestor.extensions().get::<SpanState>() {
                    root_start = ancestor_state.start;
                }
                current = ancestor.parent();
            }
            state.start.duration_since(root_start)
        };

        let node = Node::Span {
            name: state.name,
            fields: state.fields,
            duration,
            offset,
            children: state.children,
        };
        if let Some(parent_ref) = span_ref.parent()
            && let Some(ps) = parent_ref.extensions_mut().get_mut::<SpanState>()
        {
            ps.children.push(node);
            return;
        }
        print_root(&node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compute the visible width of `gutter + label + fill` for a span line.
    /// This must equal `bar_start` for bars to align.
    fn label_column_width(
        name: &str,
        fields: &str,
        depth: usize,
        term_width: usize,
    ) -> (usize, usize) {
        let gw = gutter_vis(depth);
        let bar_start = term_width * 2 / 5;
        let name_area = bar_start.saturating_sub(gw + 1);

        let name_trunc = truncate(name, name_area);
        let name_w = measure_text_width(&name_trunc);

        let plain_label = if fields.is_empty() || name_w + 2 >= name_area {
            name_trunc
        } else {
            let fields_max = name_area - name_w - 2;
            let fields_trunc = truncate(fields, fields_max);
            format!("{}  {}", name, fields_trunc)
        };

        let vis = measure_text_width(&plain_label);
        let fill_len = bar_start.saturating_sub(gw + vis);

        // Total visible = gutter + label + fill
        let total = gw + vis + fill_len;
        (total, bar_start)
    }

    #[test]
    fn bar_alignment_short_name() {
        let (total, bar_start) = label_column_width("inference", "", 1, 120);
        assert_eq!(total, bar_start, "short name at depth 1");
    }

    #[test]
    fn bar_alignment_long_fields() {
        let (total, bar_start) =
            label_column_width("engine_load", "engine=yuzumarker-font-detection", 1, 120);
        assert_eq!(total, bar_start, "long fields at depth 1");
    }

    #[test]
    fn bar_alignment_depth_2() {
        let (total, bar_start) = label_column_width(
            "load",
            "r=BlobRef(\"313180b160716be573b2cdd1cf522fe1a839b219e0\")",
            2,
            120,
        );
        assert_eq!(total, bar_start, "long fields at depth 2");
    }

    #[test]
    fn bar_alignment_no_fields_depth_0() {
        let (total, bar_start) = label_column_width("load_image", "", 1, 120);
        assert_eq!(total, bar_start, "no fields at depth 1");
    }

    #[test]
    fn bar_alignment_various_widths() {
        for tw in [80, 100, 120, 160, 200] {
            for depth in [1, 2, 3] {
                let (total, bar_start) = label_column_width("inference", "blocks=8", depth, tw);
                assert_eq!(total, bar_start, "tw={tw} depth={depth}");
            }
        }
    }

    #[test]
    fn bar_alignment_name_exceeds_area() {
        let (total, bar_start) = label_column_width(
            "very_long_engine_name_that_exceeds",
            "with=extra_fields",
            1,
            80,
        );
        assert_eq!(total, bar_start, "name exceeds area");
    }

    #[test]
    fn truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long() {
        assert_eq!(truncate("hello world", 7), "hello..");
    }

    #[test]
    fn truncate_tiny() {
        assert_eq!(truncate("hello", 2), "..");
    }
}
