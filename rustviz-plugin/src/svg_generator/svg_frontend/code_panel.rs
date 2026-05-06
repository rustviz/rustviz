extern crate handlebars;

use crate::svg_generator::data::{ExternalEvent, LINE_SPACE};
use handlebars::Handlebars;
use std::{cmp::max, collections::{BTreeMap, HashMap}};

pub fn render_code_panel(
    annotated_lines: std::str::Lines,
    lines: std::str::Lines,
    max_x_space: &mut i64,
    _event_line_map: &BTreeMap<usize, Vec<ExternalEvent>>,
    l_map: &HashMap<usize, usize>, 
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

    // Account for the extra arrow rows the timeline panel reserves
    // for arrow events that share a line — without this, a snippet
    // with 9 source lines but an arrow stack pushing the rendered
    // count to 10+ would lose its line-number alignment when the
    // last few rendered numbers cross to two digits.
    let total_with_arrow_rows: usize = total_lines + l_map.values().sum::<usize>();
    // Right-align line numbers to the widest one. With left-aligned
    // numbers (`9  ` vs `10  `) the trailing spaces shift content
    // by a column whenever a digit boundary is crossed; right-align
    // them and the content column stays put across the whole file.
    let num_width = total_with_arrow_rows.max(1).to_string().len();

    /* Render the code segment of the svg to a String */
    let x = 20;
    let mut y = 90;
    let mut output = String::from("    <g id=\"code\">\n");
    let mut line_of_code = 1;
    for line in annotated_lines {
        let line_string = line;
        let mut data = BTreeMap::new();
        data.insert("X_VAL".to_string(), x.to_string());
        data.insert("Y_VAL".to_string(), y.to_string());
        /* automatically add line numbers to code */
        let fmt_line = format!(
            "<tspan fill=\"#AAA\">{:>width$}  </tspan>{}",
            line_of_code, line_string, width = num_width,
        );
        data.insert("LINE".to_string(), fmt_line);
        output.push_str(&handlebars.render("code_line_template", &data).unwrap());
        // change line spacing
        y = y + LINE_SPACE;
        let mut extra_line_num = match l_map.get(&line_of_code) {
            Some(l) => *l,
            None => 0
        };
        /* add empty lines for arrows */
        while extra_line_num > 0 {
            let mut data = BTreeMap::new();
            data.insert("X_VAL".to_string(), x.to_string());
            data.insert("Y_VAL".to_string(), y.to_string());
            /* automatically add line numbers to code */
            line_of_code = line_of_code + 1;
            let empty_line = format!(
                "<tspan fill=\"#AAA\">{:>width$}</tspan>",
                line_of_code, width = num_width,
            );
            data.insert("LINE".to_string(), empty_line);
            output.push_str(&handlebars.render("code_line_template", &data).unwrap());
            y = y + LINE_SPACE;
            extra_line_num -= 1;
        }
        line_of_code = line_of_code + 1;
    }
    output.push_str("    </g>\n");
    (output, line_of_code as i32)
}
