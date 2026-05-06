extern crate handlebars;

use crate::svg_generator::data::{ExternalEvent, LINE_SPACE};
use crate::svg_generator::svg_frontend::syntax;
use handlebars::Handlebars;
use std::{cmp::max, collections::{BTreeMap, HashMap, HashSet}};

pub fn render_code_panel(
    annotated_lines: std::str::Lines,
    lines: std::str::Lines,
    max_x_space: &mut i64,
    _event_line_map: &BTreeMap<usize, Vec<ExternalEvent>>,
    l_map: &HashMap<usize, usize>,
    // Visual-row positions (1-indexed in the post-fn-blank-pass
    // sequence) where svg_generation injected a synthetic blank line
    // before a non-first fn so the fn-label header has somewhere to
    // sit. Rendered with a blank gutter and no source-line increment
    // — same treatment arrow-stack inserts get below.
    synthetic_blank_rows: &HashSet<usize>,
) -> (String, i32) {
    /* Template creation */
    let mut handlebars = Handlebars::new();
    // We want to preserve the inputs `as is`, and want to make no changes based on html escape.
    handlebars.register_escape_fn(handlebars::no_escape);
    let line_template =
        "        <text class=\"code\" x=\"{{X_VAL}}\" y=\"{{Y_VAL}}\"> {{LINE}} </text>\n";
    // register the template. The template string will be verified and compiled.
    assert!(handlebars
        .register_template_string("code_line_template", line_template)
        .is_ok());
    
    // figure out that max length
    let mut total_lines: usize = 0;
    for line in lines {
        *max_x_space = max(line.len() as i64, *max_x_space);
        total_lines += 1;
    }

    // Right-align the gutter to the widest source line number we'll
    // actually print. Arrow-clearance and fn-blank inserts both
    // render with a blank gutter, so the largest displayed number
    // is the source-line count minus all the synthetic blanks. Right
    // alignment matters because left alignment shifts the content
    // column whenever a digit boundary crosses (`9  ` vs `10  `).
    let displayed_lines = total_lines.saturating_sub(synthetic_blank_rows.len());
    let num_width = displayed_lines.max(1).to_string().len();

    /* Render the code segment of the svg to a String */
    let x = 20;
    let mut y = 90;
    let mut output = String::from("    <g id=\"code\">\n");
    // `source_line` tracks the user's source-line number — what we
    // print in the gutter for real source rows. `visual_row`
    // tracks the renderer's row counter — drives both the y
    // position and the `l_map` lookup, since `l_map`'s keys are
    // post-shift visual rows (see svg_generation.rs's `final_line_num
    // = line_num + extra_lines`). They diverge whenever an arrow
    // stack inserts a blank row: visual_row keeps ticking, source_line
    // pauses. Keeping them separate is what lets us number the
    // displayed lines 1..n_source while still indexing per-row
    // structures by the visual row.
    let mut source_line: usize = 1;
    let mut visual_row: usize = 1;
    // Threaded through per-line `highlight()` calls so a `/* … */`
    // that doesn't close on its opening line keeps subsequent lines
    // styled as a comment until we see the matching `*/`.
    let mut in_block_comment = false;
    for line in annotated_lines {
        let line_string = syntax::highlight(line, &mut in_block_comment);
        let is_synthetic_blank = synthetic_blank_rows.contains(&visual_row);
        let mut data = BTreeMap::new();
        data.insert("X_VAL".to_string(), x.to_string());
        data.insert("Y_VAL".to_string(), y.to_string());
        // Synthetic fn-blank inserts get the same blank-gutter
        // treatment as arrow-clearance rows; only real source rows
        // tick `source_line`.
        let fmt_line = if is_synthetic_blank {
            format!(
                "<tspan fill=\"#AAA\">{:>width$}  </tspan>{}",
                "", line_string, width = num_width,
            )
        } else {
            format!(
                "<tspan fill=\"#AAA\">{:>width$}  </tspan>{}",
                source_line, line_string, width = num_width,
            )
        };
        data.insert("LINE".to_string(), fmt_line);
        output.push_str(&handlebars.render("code_line_template", &data).unwrap());
        y = y + LINE_SPACE;
        let mut extra_line_num = match l_map.get(&visual_row) {
            Some(l) => *l,
            None => 0
        };
        /* add empty arrow-clearance rows. They occupy a visual row
           (so the trapezoid arrow has somewhere to draw) but stay
           blank in the gutter — line numbers in the visualization
           stay one-to-one with the editor's line numbers. */
        while extra_line_num > 0 {
            let mut data = BTreeMap::new();
            data.insert("X_VAL".to_string(), x.to_string());
            data.insert("Y_VAL".to_string(), y.to_string());
            // Pad with the same width as a real number so the
            // content column doesn't jiggle row-to-row.
            let empty_line = format!(
                "<tspan fill=\"#AAA\">{:>width$}  </tspan>",
                "", width = num_width,
            );
            data.insert("LINE".to_string(), empty_line);
            output.push_str(&handlebars.render("code_line_template", &data).unwrap());
            y = y + LINE_SPACE;
            visual_row += 1;
            extra_line_num -= 1;
        }
        if !is_synthetic_blank {
            source_line += 1;
        }
        visual_row += 1;
    }
    output.push_str("    </g>\n");
    // Returned value used to be the next line number; preserve that
    // shape (callers want a row count for height etc.) by returning
    // the visual row count.
    (output, visual_row as i32)
}
