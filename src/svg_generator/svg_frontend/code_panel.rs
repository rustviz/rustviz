extern crate handlebars;

use crate::data::{ExternalEvent, LINE_SPACE};
use handlebars::Handlebars;
use rand::seq::IteratorRandom;
use std::{cmp::max, collections::BTreeMap};
use std::fs::File;
use std::io;

/**
 * Return value: (String, i32, usize) => (code panel SVG string, #lines of code, width of code panel)
 */
pub fn render_code_panel(
    annotated_lines: io::Lines<io::BufReader<File>>,
    lines: io::Lines<io::BufReader<File>>,
    max_x_space: &mut i64,
    event_line_map: &BTreeMap<usize, Vec<ExternalEvent>>,
) -> (String, i32, usize) {
    println!("\n{:#?}", event_line_map);
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
    let mut max_width: usize = 0;
    // figure out that max length
    for line in lines {
        if let Ok(line_string) = line {
            *max_x_space = max(line_string.len() as i64, *max_x_space);
            max_width = max(line_string.len() * 10, max_width)
        }
    }
    
    /* Render the code segment of the svg to a String */
    let x = 20;
    let mut y = 90;
    let mut output = String::from("    <g id=\"code\">\n");
    let mut line_of_code = 1;
    for line in annotated_lines {
        if let Ok(line_string) = line {
            let mut data = BTreeMap::new();
            data.insert("X_VAL".to_string(), x.to_string());
            data.insert("Y_VAL".to_string(), y.to_string());
            /* automatically add line numbers to code */
            let fmt_line = format!(
                "<tspan fill=\"#AAA\">{}  </tspan>{}",
                line_of_code, escape_certain_chars(&line_string)
            );
            data.insert("LINE".to_string(), fmt_line);
            output.push_str(&handlebars.render("code_line_template", &data).unwrap());
            // change line spacing
            y = y + LINE_SPACE;
        }
        let mut extra_line_num = 0;
        match event_line_map.get(&(line_of_code as usize)) {
            Some(event_vec) => extra_line_num = event_vec.len(),
            None => (),
        }
        /* add empty lines for arrows */
        while extra_line_num > 1 {
            let mut data = BTreeMap::new();
            data.insert("X_VAL".to_string(), x.to_string());
            data.insert("Y_VAL".to_string(), y.to_string());
            /* automatically add line numbers to code */
            line_of_code = line_of_code + 1;
            let empty_line = format!("<tspan fill=\"#AAA\">{}</tspan>", line_of_code);
            data.insert("LINE".to_string(), empty_line);
            output.push_str(&handlebars.render("code_line_template", &data).unwrap());
            y = y + LINE_SPACE;
            extra_line_num -= 1;
        }
        line_of_code = line_of_code + 1;
    }
    output.push_str("    </g>\n");
    (output, line_of_code, max_width)
}

// will work if tspan doesn't contain another <...>
fn escape_certain_chars( code_line: &String) -> String{
    let _vs : Vec<char> = code_line.chars().collect();
    let mut ret_str = String::new();
    let mut flag_tspan = false;
    for (idx,ch) in _vs.iter().enumerate(){
        match ch {
            '<' => {
                // don't change if the following is tspan of /tspan
                if idx + 6 < _vs.len()  {
                    let s1 : String = _vs[idx+1..idx+6].iter().collect();
                    if s1 == "tspan"{
                        flag_tspan = true;
                    }
                    let s2 : String = _vs[idx+1..idx+7].iter().collect();
                    if s2 == "/tspan"{
                        flag_tspan = true;
                    }

                }
                if flag_tspan {
                    ret_str += "<";
                }
                else{
                    ret_str += "&lt;";
                }

            },
            '>' => {
                    if flag_tspan == true{
                        flag_tspan = false;
                        ret_str += ">";
                    }
                    else{
                        ret_str += "&gt;";
                    }
            },
            '&' => ret_str += "&amp;",
            _ => ret_str += ch.to_string().as_ref(),
        }
    }
    ret_str
}