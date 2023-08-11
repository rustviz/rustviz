extern crate handlebars;

use crate::data::{StructsInfo, VisualizationData, Visualizable, ExternalEvent, State, ResourceAccessPoint, Event, LINE_SPACE, self};
use crate::svg_frontend::lifetime_vis::lifetime_render_data_structures::{DoubleHeadedArrowHolder, FuncSignatureStructInstanceHolder};
use crate::svg_frontend::line_styles::{RefDataLine, RefValueLine, OwnerLine};
use handlebars::Handlebars;
use core::panic;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::format;
use serde::Serialize;
use super::*;
use super::lifetime_render_data_structures::{FuncSignatureRenderHolder, LineNumberDashHolder, VarLifetimeColumnHoler, LifetimeParameterColumnSetHoler, LifetimeRegionSquareHoler};
use std::cmp;


/*** global setting for rendering ***/
static FUNC_SIG_CHAR_X_SPACE: u32 = 10;
static SIG_LT_CMP_CHAR_X_SPACE: u32 = 8;
static X_START: u32 = 10;
static Y_START: u32 = 20;
static CODE_VERTICAL_LINE_SPACE: u32 = 30;
static LABEL_Y_VAL: u32 = 70;
static CODE_LINE_Y_START: u32 = 90;
static DASH_NUM_LINE_X_START: u32 = 30;
static mut hash : u32 = 0;
// pub static AMPERSAND : &'static str = "&amp;";
// pub static LEFT_ANGLE_BR : &'static str = "&lt;";
// pub static RIGHT_ANGLE_BR : &'static str = "&gt;";
pub static AMPERSAND : &'static str = "&";
pub static LEFT_ANGLE_BR : &'static str = "<";
pub static RIGHT_ANGLE_BR : &'static str = ">";
// set style for code string
static SPAN_BEGIN : &'static str = "<span style=\"font-family: 'Source Code Pro', Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono', monospace, monospace !important;\">";
static SPAN_END : &'static str = "</span>";


/**********************
                        Function Signature Rendering
                                                        ***********************/
/**
 * Require: Make sure output variable names as well as real input variable name has been updated
 * Effect Special:
    * If it's struct method invoked on struct instance, then there will be hover message above that instance name
 * Return: `(width: u32, y_end: u32, output: String)` first field, current width after rendering function signature; second field, y coordinate after rending; last field, rendered output string.
 */
pub fn render_function_lifetime_signature( func_info: & FuncSignatureSpec , registry: &mut Handlebars) -> (u32, u32, String){
    prepare_registry(registry);
    let mut x_cursor : u32 = X_START;
    let mut y_cursor : u32 = Y_START;
    /* store mapping: render_template_string -> FuncSignatureRenderHolder */
    let mut render_segments: BTreeMap<u32, (String, FuncSignatureRenderHolder)> = BTreeMap::new();
    let mut ret = String::new();

    /****** check if there is return value. If yes, start from that ******/
    /* for tuple output, render open parenthesis */
    if func_info.output_variables.len() > 1{
        func_sig_render_patches(&mut x_cursor, &y_cursor, "(".to_string(), &mut render_segments)
    }
    for (idx, var_info) in func_info.output_variables.iter().enumerate(){
        assert!(var_info.name == func_info.output_var_called_names[idx]);
        render_func_sig_helper(var_info, &mut x_cursor, &mut y_cursor, &mut render_segments);
        if  func_info.output_variables.len() > 1 && idx < func_info.output_variables.len() - 1{
            /* render connecting commas */
            func_sig_render_patches(&mut x_cursor, &y_cursor, ",".to_string(), &mut render_segments);
        }
    }
    /* for tuple output, render closing parenthesis */
    if func_info.output_variables.len() > 1{
        func_sig_render_patches(&mut x_cursor, &y_cursor, ")".to_string(), &mut render_segments)
    }
    /****** render function name, equality sign, opening parenthesis. If it's struct invoked method, specialize instance name ******/
    if func_info.is_not_static_struct_method{
        // if there are output variables, render equal sign
        if func_info.output_variables.len() > 0{
            func_sig_render_patches(&mut x_cursor, &y_cursor, "=".to_string(), &mut render_segments)
        }
        // render struct instance name
        let struct_instance_info = &func_info.input_variables[0];
        assert!(struct_instance_info.data_type.find("self").is_some());
        let mut x_update:u32 = 0;
        x_update = (struct_instance_info.name.len() as u32 ) * FUNC_SIG_CHAR_X_SPACE + 4;
        ret += registry.render("func_signature_struct_instance_method_invoke_template", &FuncSignatureStructInstanceHolder{
            x_val:x_cursor, y_val:y_cursor, segment:struct_instance_info.name.clone(), hover_msg: format!("{}{}{}", SPAN_BEGIN, struct_instance_info.data_type, SPAN_END)
        }).unwrap().as_str();
        // render dot, function name and opening parenthesis
        let func_name_str = String::from(".") + &func_info.function_name + "( ";
        render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
                            FuncSignatureRenderHolder{ x_val: x_cursor + x_update, y_val: y_cursor, segment: func_name_str.clone(), hover_msg:String::new()}));
        x_update += func_name_str.len() as u32 * FUNC_SIG_CHAR_X_SPACE;
        // if struct instance has lifetime parameter, then render lifetime cmp string
        if struct_instance_info.lifetime_param.is_some(){
            if struct_instance_info.lifetime_info.is_none(){
                eprintln!("no lifetime scope for {}! Check your annotation!", struct_instance_info.name);
                exit(0);
            }
            let lifetime_s_e_pnt = struct_instance_info.lifetime_info.clone().unwrap();
            let lp_cmp_str = format!("[{},{}] <= '{}", lifetime_s_e_pnt.start, lifetime_s_e_pnt.end, struct_instance_info.lifetime_param.clone().unwrap());
            let struct_ins_lp_cmp_str =  format!("lifetime of {}{}{} should be less than '{}",SPAN_BEGIN, struct_instance_info.name.clone(), SPAN_END, struct_instance_info.lifetime_param.clone().unwrap());
            x_update = cmp::max(x_update, lp_cmp_str.len() as u32 * SIG_LT_CMP_CHAR_X_SPACE);
            ret += registry.render("func_signature_LP_cmp_template", &FuncSignatureRenderHolder{ x_val: x_cursor, y_val: y_cursor + 20, segment: lp_cmp_str, hover_msg: struct_ins_lp_cmp_str}).unwrap().as_str();
        }
        x_cursor += x_update;
    }
    else{
        let prefix_func_name = match func_info.output_variables.len(){
            0 => "".to_string(),
            _ => " = ".to_string(),
        };
        let func_name_str = prefix_func_name + &func_info.function_name + "( ";
        render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
                            FuncSignatureRenderHolder{ x_val: x_cursor, y_val: y_cursor, segment: func_name_str.clone(), hover_msg: String::new()}));
        x_cursor += (func_name_str.len() as u32) * FUNC_SIG_CHAR_X_SPACE;
    }
    /****** render function input variables, equality sign, opening parenthesis ******/
    for (idx, input_var_info) in func_info.input_variables.iter().enumerate(){
        /* if it's `self`, then continue. IT HAS ALREADY RENDERED IN HOVER MESSAGE!!! */
        if input_var_info.data_type.find("self").is_some(){
            assert!(idx == 0);
            continue;
        }
        assert!(input_var_info.name == func_info.input_var_called_names[idx]);
        render_func_sig_helper(input_var_info, &mut x_cursor, &mut y_cursor, &mut render_segments);
        if func_info.input_variables.len() > 1 && idx < func_info.input_variables.len() - 1{
            /* render connecting commas */
            func_sig_render_patches(&mut x_cursor, &y_cursor, ",".to_string(), &mut render_segments);
        }
    }

    /****** render closing parenthesis ******/
    render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
                        FuncSignatureRenderHolder{ x_val: x_cursor - 10, y_val: y_cursor, segment: ")".to_string(), hover_msg: String::new()}));
    x_cursor += 2 * FUNC_SIG_CHAR_X_SPACE;


    
    /* debug */
    for (key, strc) in render_segments.iter(){
        let tmp = registry.render(strc.0.as_str(), &strc.1).unwrap();
        ret += tmp.as_str();
    }

    (x_cursor, y_cursor, ret)
}

/**
 * for function signature string, render variable no matter what;
 * for lifetime comparison string, if this variable has no lifetime parameter related, then render nothing.
 */
fn render_func_sig_helper(var_info: & VariableSpec, x_cursor: &mut u32, y_cursor: &mut u32, render_segments: &mut BTreeMap<u32, (String, FuncSignatureRenderHolder)>) {
    let Y_Space : u32 = 20;
    let mut x_update : u32 = 0;
    /* render code segment */
    let var_sig_string = var_info.to_string();
    // println!("name: {}", var_sig_string);
    x_update =  (var_sig_string.len() as u32) * FUNC_SIG_CHAR_X_SPACE;

    render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
    FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor, segment: var_sig_string, hover_msg: String::new()}));

    if let Some(lifetime_param) = &var_info.lifetime_param{
        if let Some(lifetime_scope) = &var_info.lifetime_info{
            // generate lifetime scope vs lifetime param string: [3,9] <= 'a
            let cmp = format!("[{},{}] {}= '{}", lifetime_scope.start, lifetime_scope.end, LEFT_ANGLE_BR, lifetime_param );
            x_update = if x_update > ((cmp.len() as u32) * SIG_LT_CMP_CHAR_X_SPACE) {x_update} else {(cmp.len() as u32) * SIG_LT_CMP_CHAR_X_SPACE};
            render_segments.insert(get_hash(),("func_signature_LP_cmp_template".to_string(),
            FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor + Y_Space, segment: cmp,
                                       hover_msg: format!("lifetime of {} should be less than '{}", var_info.name, lifetime_param)}));
        }
    }
    // update x_cursor
    *x_cursor += x_update;

}

fn func_sig_render_patches(x_cursor: &mut u32, y_cursor: & u32, patch_to_render: String, render_segments: &mut BTreeMap<u32, (String, FuncSignatureRenderHolder)>){
    let mut x_cor = *x_cursor;
    if patch_to_render.len() == 1{
        x_cor -= 5
    }
    render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
        FuncSignatureRenderHolder{ x_val: x_cor, y_val: *y_cursor, segment: patch_to_render.clone(),  hover_msg: String::new()}));
    match patch_to_render.as_ref(){
        "," | "(" | ")" => *x_cursor += 1 * FUNC_SIG_CHAR_X_SPACE + 5,
        _ => *x_cursor += (patch_to_render.len() as u32) * FUNC_SIG_CHAR_X_SPACE,
    }
}

/**********************
                        Variable Lifetime Column Rendering
                                                            ***********************/
/**
 * Required:
    + Before this function is called, make sure lifetime line number has been changed to absolute line number to relative line number r.w.t the line where main() get called.
    + `vars_lifetime` contains variables of the same lifetime parameter!
    + `vars_lifetime` should contains `VariableSpec` with `data_hash` already calculated!
 * Return: `(width: u32, output: String)`. `width` the width (x_cursor position) after rendering all the columns.
 */
pub fn render_lifetime_columns_one_for_lifetime_parameter(vars_lifetime: &Vec<VariableSpec>, registry: &Handlebars, x_begin: u32, lifetime_hash: &usize, max_y: &u32) -> (u32, String){
    let mut lifetime_parameter = String::new();
    /* check if all vars contains same type of LP */
    for elem in vars_lifetime.iter(){
        if let Some(lp) = elem.lifetime_param.clone(){
            if lifetime_parameter.len() == 0{ lifetime_parameter = lp}
            else if lifetime_parameter != lp {
                panic!("Variables are not of the same lifetime parameter!")
            }
        }
        else{
            panic!("Input variable contain no lifetime parameter!");
        }
        if elem.data_hash.is_none(){
            panic!("Input variable has no data hash yet!");
        }
    }
    let mut x_cursor : u32 = 70 + x_begin;
    let mut var_lifetime_column_render_holer : BTreeMap<u32, (String, VarLifetimeColumnHoler)> = BTreeMap::new();
    for var_info in vars_lifetime.iter(){
        // must be lifetime number for this variable
        assert!(var_info.lifetime_info.is_some());
        let mut var_column_data = VarLifetimeColumnHoler::new();
        /* assign and update hash */
        var_column_data.data_hash = var_info.data_hash.clone().unwrap();
        /* assign x_anchor */
        var_column_data.x_anchor = x_cursor;
        /* assign y coordinate for label. Should be fixed value */
        var_column_data.y_label = LABEL_Y_VAL;
        /* calculate y_start and y_end based on lifetime number */
        var_column_data.y_start = CODE_LINE_Y_START + (var_info.lifetime_info.clone().unwrap().start - 1) * CODE_VERTICAL_LINE_SPACE;
        var_column_data.y_end = CODE_LINE_Y_START + (var_info.lifetime_info.clone().unwrap().end - 1) * CODE_VERTICAL_LINE_SPACE;
        /* assign label name and update x_cursor */
        var_column_data.label_name = var_info.name.clone() + &format!(": '{}", var_info.lifetime_param.clone().unwrap());

        x_cursor += cmp::max(70, (var_column_data.label_name.len() as u32 - 1) * 13);
        /* added additional hover messages if there are any */
        for msg in var_info.hover_messages.iter(){
            match msg{
                ExtraExplanation::BODY(s) => var_column_data.BODY_msg = s.clone(),
                ExtraExplanation::CRPT(s) => var_column_data.CRPT_msg = s.clone(),
                ExtraExplanation::DRPT(s) => var_column_data.DRPT_msg = s.clone(),
                ExtraExplanation::NAME(s) => var_column_data.NAME_msg = format!("{}.{}", SPAN_BEGIN.to_string() + var_info.name.as_str() + SPAN_END, s),
            }
        }
        /* fill other field is no extra annotation */
        if var_column_data.NAME_msg.len() == 0 {
            var_column_data.NAME_msg = var_info.to_string()
        }
        if var_column_data.BODY_msg.len() == 0 {
            var_column_data.BODY_msg = format!("Lifetime of {} continues.", var_info.name)
        }
        if var_column_data.CRPT_msg.len() == 0 {
            var_column_data.CRPT_msg = format!("{} comes into scope.", var_info.name)
        }
        if var_column_data.DRPT_msg.len() == 0 {
            var_column_data.DRPT_msg = format!("{} comes out of scope. Resource get dropped and self get destroyed.", var_info.name)
        }
        /* finished layout computation! Let's added to the Map for later rendering! */
        var_lifetime_column_render_holer.insert(var_column_data.data_hash,
                                                ("var_lifetime_label_start_body_end_template".to_string(),var_column_data));
    }

    let mut ret = String::new();
    for (_, elem) in var_lifetime_column_render_holer.iter(){
        ret += registry.render(elem.0.as_str(), &elem.1).unwrap().as_str();
    }
    /* render lifetime parameter column set along with it */
    let (lp_render_string, lp_name) = render_lifetime_parameter_columns_set(&mut x_cursor, vars_lifetime, registry, lifetime_hash);
    ret += &lp_render_string;
    ret = format!("{}\n{}", registry.render("lifetime_region_square_template", &LifetimeRegionSquareHoler{
        lifetime_hash: *lifetime_hash as u32,
        x: x_begin + 10,
        y: CODE_LINE_Y_START - 10,
        w: x_cursor - x_begin - 5,
        h: *max_y,
        hover_msg: format!("lifetime calculation block for '{}", lp_name)
    }).unwrap().as_str(),
    ret);
    (x_cursor, ret)
}

/**
 * Return (rendered SVG string, name of lifetime parameter)
 */
pub fn render_lifetime_parameter_columns_set(x_cursor: &mut u32, vars_lifetime: &Vec<VariableSpec>, registry: &Handlebars, lifetime_hash: &usize) -> (String, String){

    // calculate lifetime parameter range
    let mut result = String::new();
    let mut var_name_vec: Vec<String> = Vec::new();
    let  (mut lp_start, mut lp_end) : (u32, u32) = (u32::MAX,0);
    let lp_name = vars_lifetime[0].lifetime_param.clone().unwrap();
    for var in vars_lifetime.iter(){
        if lp_name != var.lifetime_param.clone().unwrap(){
            panic!("input variables are not of the same lifetime parameter!")
        }
        lp_start = cmp::min(lp_start, var.lifetime_info.clone().unwrap().start);
        lp_end = cmp::max(lp_end, var.lifetime_info.clone().unwrap().end);
        assert!(lp_start <= lp_end, "lp end is smaller than lp start!");
        var_name_vec.push(var.name.clone());
    }
    let render_content = LifetimeParameterColumnSetHoler{
        lp_name: format!("'{}: [{},{}]", lp_name, lp_start, lp_end),
        y_label: LABEL_Y_VAL,
        lifetime_hash: *lifetime_hash as u32,
        x_dash: *x_cursor,
        x_solid: *x_cursor + 30,
        y_dash_start: CODE_LINE_Y_START + (lp_start - 1) * CODE_VERTICAL_LINE_SPACE - 30,
        y_dash_end: CODE_LINE_Y_START + (lp_end - 1) * CODE_VERTICAL_LINE_SPACE + 30,
        y_start: CODE_LINE_Y_START + (lp_start - 1) * CODE_VERTICAL_LINE_SPACE,
        y_end: CODE_LINE_Y_START + (lp_end - 1) * CODE_VERTICAL_LINE_SPACE,
        x_middle: *x_cursor + 30,
        x_left: *x_cursor + 25,
        x_right: *x_cursor + 35,
        y_vertices_up: CODE_LINE_Y_START + (lp_start - 1) * CODE_VERTICAL_LINE_SPACE - 5,
        y_vertices_bot: CODE_LINE_Y_START + (lp_end - 1) * CODE_VERTICAL_LINE_SPACE + 5,
        y_line_up: CODE_LINE_Y_START + (lp_start - 1) * CODE_VERTICAL_LINE_SPACE,
        y_line_bot: CODE_LINE_Y_START + (lp_end - 1) * CODE_VERTICAL_LINE_SPACE,
        lp_dash_msg: format!("{}'{}{}", SPAN_BEGIN, lp_name, SPAN_END) + " can be infinitely large. But we want it as small as possible so as to reduce the constraint to borrow checker",
        msg_up:  format!("{}'{}{} could start earlier, but it's unnecessary since references related to it haven't come into scope yet.", SPAN_BEGIN, lp_name, SPAN_END),
        msg_bot: format!("{}'{}{} could end later, but it's unnecessary since references related to it have all gone out of scope.", SPAN_BEGIN, lp_name, SPAN_END),
        lp_solid_msg: format!("The smallest scope for {}'{}{} to validate all variable lifetimes related to it", SPAN_BEGIN, lp_name, SPAN_END),
        lp_calc_text: format!("{}'{}{} is calculated based on lifetimes of {}{}{}",
                            SPAN_BEGIN, lp_name, SPAN_END, SPAN_BEGIN, var_name_vec.join(", "), SPAN_END)
    };
    result += &registry.render("lifetime_parameter_column_set_template", &render_content).unwrap();
    /* render comparison arrows */
    let mut arrow_holer: Vec<DoubleHeadedArrowHolder> = Vec::new();
    for var_info in vars_lifetime.iter(){
        let (var_lifetime_start, var_lifetime_end ) = (var_info.lifetime_info.clone().unwrap().start, var_info.lifetime_info.clone().unwrap().end);
        let render_arrow = DoubleHeadedArrowHolder{
            data_hash: var_info.data_hash.clone().unwrap(),
            x_middle: render_content.x_dash,
            x_left: render_content.x_dash - 5,
            x_right: render_content.x_dash + 5,
            y_start: CODE_LINE_Y_START + (var_lifetime_start - 1) * CODE_VERTICAL_LINE_SPACE,
            y_end: CODE_LINE_Y_START + (var_lifetime_end - 1) * CODE_VERTICAL_LINE_SPACE,
            y_vertices_up: CODE_LINE_Y_START + (var_lifetime_start - 1) * CODE_VERTICAL_LINE_SPACE - 5,
            y_vertices_bot: CODE_LINE_Y_START + (var_lifetime_end - 1) * CODE_VERTICAL_LINE_SPACE + 5,
            msg: format!("{}'{}{} should be at least as large as lifetime of {}{}{}.", SPAN_BEGIN, lp_name, SPAN_END, SPAN_BEGIN, var_info.name, SPAN_END),
        };
        arrow_holer.push(render_arrow);
    }
    /* sort Vec based on (y_end - y_start) in ascending order*/
    arrow_holer.sort_by_key(|x| (x.y_start as i32 - x.y_end as i32));
    for arrow in arrow_holer.iter(){
        result += &registry.render("double_headed_arrow_template", arrow).unwrap();
    }
    *x_cursor += 35;
    (result, lp_name)
}
/**********************
                        Dashed Line Number Rendering
                                                        ***********************/

pub fn render_dashed_number_line(vars: Vec<VariableSpec>, max_width: u32, registry: &Handlebars) -> String{
    let mut line_number_render_holer : BTreeMap<u32, (String, LineNumberDashHolder)> = BTreeMap::new();
    let mut all_line_numbers : HashSet<u32> = HashSet::new();
    for var in vars.iter(){
        if let Some(lifetime_info) = &var.lifetime_info{
            all_line_numbers.insert(lifetime_info.start);
            all_line_numbers.insert(lifetime_info.end);
        }
    }
    for num in all_line_numbers.iter(){
        line_number_render_holer.insert(get_hash(), ("line_num_dash_template".to_string(),
        LineNumberDashHolder{
            x1: DASH_NUM_LINE_X_START,
            y1: CODE_LINE_Y_START + (num - 1) * CODE_VERTICAL_LINE_SPACE,
            x2: DASH_NUM_LINE_X_START + max_width,
            y2: CODE_LINE_Y_START + (num - 1) * CODE_VERTICAL_LINE_SPACE,
            line_number: *num
        } ));
    }
    let mut ret = String::new();
    for (_, elem) in line_number_render_holer.iter(){
        ret += registry.render(elem.0.as_str(), &elem.1).unwrap().as_str();
    }
    ret
}



/* helpers */
pub fn prepare_registry(registry: &mut Handlebars){

    /* function signature related */
    let func_signature_code_template =
    "       <text class=\"funcSigCode\" x=\"{{x_val}}\" y=\"{{y_val}}\"> {{segment}} </text>\n";
    let func_signature_lp_cmp_template =
    "       <text class=\"lifetime tooltip-trigger\" x=\"{{x_val}}\" y=\"{{y_val}}\" data-tooltip-text=\"{{hover_msg}}\"> {{segment}} </text>\n";

    let func_signature_struct_instance_method_invoke_template =
    "       <text class=\"structInstance tooltip-trigger\" x=\"{{x_val}}\" y=\"{{y_val}}\" data-tooltip-text=\"{{hover_msg}}\">  {{segment}} </text>\n";

    /* for variable lifetime start/end point, lifetime body rendering */
    let var_lifetime_label_start_body_end_template =
    "
            <g id=\"lifetime-column-{{label_name}}\">
                <text x=\"{{x_anchor}}\" y=\"{{y_label}}\" style=\"text-anchor:middle\" data-hash=\"{{data_hash}}\" class=\"label tooltip-trigger\" data-tooltip-text=\"{{NAME_msg}}\">{{label_name}}</text>
                <circle cx=\"{{x_anchor}}\" cy=\"{{y_start}}\" r=\"5\" data-hash=\"{{data_hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{CRPT_msg}}\" />
                <line data-hash=\"{{data_hash}}\" class=\"solid tooltip-trigger\" x1=\"{{x_anchor}}\" x2=\"{{x_anchor}}\" y1=\"{{y_start}}\" y2=\"{{y_end}}\" data-tooltip-text=\"{{BODY_msg}}\"/>
                <circle cx=\"{{x_anchor}}\" cy=\"{{y_end}}\" r=\"5\" data-hash=\"{{data_hash}}\" class=\"tooltip-trigger\" data-tooltip-text=\"{{DRPT_msg}}\" />
            </g>
    \n";

    let lifetime_parameter_column_set_template =
    "
            <g id=\"lp-column-{{lp_name}}\">
                <line lifetime-dash-hash=\"{{lifetime_hash}}\" stroke-width=\"3\" stroke-dasharray=\"5,2\" class=\"tooltip-trigger\" x1=\"{{x_dash}}\" x2=\"{{x_dash}}\" y1=\"{{y_dash_start}}\" y2=\"{{y_dash_end}}\" data-tooltip-text=\"{{lp_dash_msg}}\"/>
                <text x=\"{{x_solid}}\" y=\"{{y_label}}\" style=\"text-anchor:middle\" lifetime-body-hash=\"{{lifetime_hash}}\" class=\"label_lifetime tooltip-trigger\" data-tooltip-text=\"{{lp_calc_text}}\">{{lp_name}}</text>
                <polygon class=\"tooltip-trigger\" lifetime-body-hash=\"{{lifetime_hash}}\" points=\"{{x_middle}},{{y_vertices_up}} {{x_left}},{{y_line_up}} {{x_right}},{{y_line_up}}\" data-tooltip-text=\"{{msg_up}}\" />
                <line lifetime-body-hash=\"{{lifetime_hash}}\" class=\"solid tooltip-trigger\" x1=\"{{x_solid}}\" x2=\"{{x_solid}}\" y1=\"{{y_start}}\" y2=\"{{y_end}}\" data-tooltip-text=\"{{lp_solid_msg}}\"/>
                <polygon class=\"tooltip-trigger\" lifetime-body-hash=\"{{lifetime_hash}}\" points=\"{{x_middle}},{{y_vertices_bot}} {{x_left}},{{y_line_bot}} {{x_right}},{{y_line_bot}}\" data-tooltip-text=\"{{msg_bot}}\" />
            </g>
    \n
    ";

    let double_headed_arrow_template =
    "
            <g id=\"double-head-arrow-{{data_hash}}\">
                <polygon class=\"tooltip-trigger\" data-hash=\"{{data_hash}}\" points=\"{{x_middle}},{{y_vertices_up}} {{x_left}},{{y_start}} {{x_right}},{{y_start}}\" data-tooltip-text=\"{{msg}}\" />
                <line data-hash=\"{{data_hash}}\" class=\"solid tooltip-trigger\" x1=\"{{x_middle}}\" x2=\"{{x_middle}}\" y1=\"{{y_start}}\" y2=\"{{y_end}}\" data-tooltip-text=\"{{msg}}\"/>
                <polygon class=\"tooltip-trigger\" data-hash=\"{{data_hash}}\" points=\"{{x_middle}},{{y_vertices_bot}} {{x_left}},{{y_end}} {{x_right}},{{y_end}}\" data-tooltip-text=\"{{msg}}\" />
            </g>
    ";
    /* for dashed number line rendering */
    let line_num_dash_template =
    "
            <g id=\"dashed-line-{{line_number}}\">
                <line class=\"lineNumDashLine\" x1=\"{{x1}}\" y1=\"{{y1}}\" x2=\"{{x2}}\" y2=\"{{y2}}\" />
                <text class=\"lineNum\" x=\"{{x1}}\" y=\"{{y1}}\" text-anchor=\"start\">#{{line_number}}</text>
            </g>
    \n
    ";

    let lifetime_region_square_template =
    "
    <rect class=\"tooltip-trigger\" lifetime-reg-hash=\"{{lifetime_hash}}\" stroke-width=\"3\" stroke-dasharray=\"5,2\" x=\"{{x}}\" y=\"{{y}}\"  width=\"{{w}}\" height=\"{{h}}\" fill=\"none\" data-tooltip-text=\"{{hover_msg}}\"/>\n
    ";

    assert!(
        registry.register_template_string("func_signature_struct_instance_method_invoke_template", func_signature_struct_instance_method_invoke_template).is_ok()
    );

    assert!(
        registry.register_template_string("func_signature_code_template", func_signature_code_template).is_ok()
    );
    assert!(
        registry.register_template_string("func_signature_LP_cmp_template", func_signature_lp_cmp_template).is_ok()
    );

    assert!(
        registry.register_template_string("var_lifetime_label_start_body_end_template", var_lifetime_label_start_body_end_template).is_ok()
    );

    assert!(
        registry.register_template_string("lifetime_parameter_column_set_template", lifetime_parameter_column_set_template).is_ok()
    );
    assert!(
        registry.register_template_string("double_headed_arrow_template", double_headed_arrow_template).is_ok()
    );

    assert!(
        registry.register_template_string("line_num_dash_template", line_num_dash_template).is_ok()
    );

    assert!(
        registry.register_template_string("lifetime_region_square_template", lifetime_region_square_template).is_ok()
    );

}






fn get_hash() -> u32{
    unsafe { hash += 1 };
    return unsafe { hash }
}
