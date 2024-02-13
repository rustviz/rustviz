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
use syn::parse_quote;
use syn::Type;

fn is_reference(s: &str) -> bool {
    // Parse the input string as a Rust type
    let ty: Result<Type, _> = syn::parse_str(s);
    
    // Check if the parsed type is a reference
    if let Ok(Type::Reference(_)) = ty {
        true
    } else {
        false
    }
}

/*** global setting for rendering ***/
pub const FUNC_SIG_CHAR_X_SPACE: u32 = 10;
pub const SIG_LT_CMP_CHAR_X_SPACE: u32 = 8;
pub const X_START: u32 = 10;
pub const Y_START: u32 = 20;
pub const CODE_VERTICAL_LINE_SPACE: u32 = 30;
pub const LABEL_Y_VAL: u32 = 90;
pub const CODE_LINE_Y_START: u32 = 110;
pub const DASH_NUM_LINE_X_START: u32 = 30;
pub const USE_VAR_NAME_AS_LP_NAME: bool = true;
static mut hash : u32 = 0;
// pub static AMPERSAND : &'static str = "&amp;";
// pub static LEFT_ANGLE_BR : &'static str = "&lt;";
// pub static RIGHT_ANGLE_BR : &'static str = "&gt;";
pub const AMPERSAND : &'static str = "&";
pub const LEFT_ANGLE_BR : &'static str = "<";
pub const RIGHT_ANGLE_BR : &'static str = ">";
// set style for code string
pub const SPAN_BEGIN : &'static str = "<span style=\"font-family: 'Source Code Pro', Consolas, 'Ubuntu Mono', Menlo, 'DejaVu Sans Mono', monospace, monospace !important;\">";
pub const SPAN_END : &'static str = "</span>";
pub const SPAN_BOLD_BEGIN : &'static str = "<span style=\"font-style: italic;font-weight: bold\">";
pub const NEW_LINE: &'static str = "<br />";

/**********************
                        Function Signature Rendering
                                                        ***********************/

pub fn render_function_lifetime_signature_lifetime_type( func_info: & FuncSignatureSpec , registry: &mut Handlebars) -> (u32, u32, String){
    prepare_registry(registry);
    let mut x_cursor : u32 = X_START;
    let mut y_cursor : u32 = Y_START;
    /* store mapping: render_template_string -> FuncSignatureRenderHolder */
    let mut render_segments: BTreeMap<u32, (String, FuncSignatureRenderHolder)> = BTreeMap::new();
    /* store sub lifetime parameter index. E.g. i -> 2 means that next lifetime parameter index should be 3 */
    let mut sub_lifetime_notation_dict: HashMap<String, i32> = HashMap::new();
    let mut ret = String::new();

    /***** render function name, its generic annotation and left parenthesis e.g. max<'a>( *****/
    
    helper_render_func_name_generic_type(func_info, &mut x_cursor, &mut y_cursor, &mut render_segments, &mut ret, registry);



    /****** render function input variables, closing parenthesis ******/
    let special_case_default_constructor = func_info.function_name == "default_constructor";
    for (idx, input_var_info) in func_info.input_variables.iter().enumerate() {
        /* if it's `self`, then continue. IT HAS ALREADY RENDERED IN HOVER MESSAGE!!! */
        if input_var_info.data_type.find("self").is_some(){
            assert!(idx == 0);
            continue;
        }
        assert!(input_var_info.name == func_info.input_var_called_names[idx]);
        if func_info.input_variables.len() > 1 && idx < func_info.input_variables.len() - 1{
            render_func_sig_helper_type_new(input_var_info, &mut x_cursor, &mut y_cursor, &mut render_segments, &mut sub_lifetime_notation_dict, format!(","));
        }
        else{
            if special_case_default_constructor{
                render_func_sig_helper_type_new(input_var_info, &mut x_cursor, &mut y_cursor, &mut render_segments, &mut sub_lifetime_notation_dict, format!(" }}"));
            }
            else{
                render_func_sig_helper_type_new(input_var_info, &mut x_cursor, &mut y_cursor, &mut render_segments, &mut sub_lifetime_notation_dict, format!(" )"));
            }
        }
 
    }

    /****** render return variables if there is any ******/
    if func_info.output_variables.len() > 0{
        /* render arrow */
        render_segments.insert(get_hash(),("func_signature_code_param_template".to_string(),
                            FuncSignatureRenderHolder{ x_val: x_cursor, y_val: y_cursor, segment: "->".to_string(), hover_msg: String::new()}));
        x_cursor += 5 * FUNC_SIG_CHAR_X_SPACE;
        /* render return variables */
        if func_info.output_variables.len() > 1{
            /* render open parenthesis */
            render_segments.insert(get_hash(),("func_signature_code_param_template".to_string(),
                            FuncSignatureRenderHolder{ x_val: x_cursor, y_val: y_cursor, segment: "(".to_string(), hover_msg: String::new()}));
            x_cursor += 1 * FUNC_SIG_CHAR_X_SPACE;
        }
        for (idx, var_info) in func_info.output_variables.iter().enumerate(){
            assert!(var_info.name == func_info.output_var_called_names[idx]);
            
            if  func_info.output_variables.len() > 1 && idx < func_info.output_variables.len() - 1{
                render_func_sig_helper_type_new(var_info, &mut x_cursor, &mut y_cursor, &mut render_segments, &mut sub_lifetime_notation_dict, format!(","));
            }
            else if func_info.output_variables.len() > 1 && idx == func_info.output_variables.len() - 1{
                render_func_sig_helper_type_new(var_info, &mut x_cursor, &mut y_cursor, &mut render_segments, &mut sub_lifetime_notation_dict, format!(")"));
            }
            else{
                render_func_sig_helper_type_new(var_info, &mut x_cursor, &mut y_cursor, &mut render_segments, &mut sub_lifetime_notation_dict, format!(""));
            }
        }
    }

    // /* render closing parenthesis */
    // render_segments.insert(get_hash(),("func_signature_code_param_template".to_string(),
    //                 FuncSignatureRenderHolder{ x_val: x_cursor, y_val: y_cursor, segment: ")".to_string(), hover_msg: String::new()}));
    // x_cursor += 6 * FUNC_SIG_CHAR_X_SPACE;

    /****** render seperating lines ******/
    let length = (x_cursor) / FUNC_SIG_CHAR_X_SPACE;
    let spe_str = "-".repeat(length as usize);
    render_segments.insert(get_hash(),("func_signature_code_sep_template".to_string(),
                            FuncSignatureRenderHolder{ x_val: 10, y_val: Y_START + 31, segment: spe_str, hover_msg: String::new()}));


    for (_, strc) in render_segments.iter(){
        let tmp = registry.render(strc.0.as_str(), &strc.1).unwrap();
        ret += tmp.as_str();
    }
    (x_cursor + 5, y_cursor, ret)


}

fn is_mutable_reference(s: &str) -> bool {
    let re = Regex::new(r"^\s*&\s*('[_a-zA-Z][_a-zA-Z0-9]*)?\s*mut\s+\w+\s*$").unwrap();
    re.is_match(s)
}



fn render_func_sig_helper_type_new(var_info: & VariableSpec, x_cursor: &mut u32, y_cursor: &mut u32, render_segments: &mut BTreeMap<u32, (String, FuncSignatureRenderHolder)>, sub_lifetime_notation_dict: &mut HashMap<String, i32>, connect_ch: String) {
    let Y_Space : u32 = 20;
    let mut x_update : u32 = 0;
    let mut x_update_type_seg : u32 = 0;
    let mut x_update_var_type_seg : u32 = 0;
    let mut x_update_var_lp_cmp_seg : u32 = 0;
    /* render original signature segment . E.g. &'a i32 */
    let param_type_string = var_info.data_type.clone() + &connect_ch;
    x_update_type_seg =  (param_type_string.len() as u32) * FUNC_SIG_CHAR_X_SPACE;
    render_segments.insert(get_hash(),("func_signature_code_param_template".to_string(),
        FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor, segment: param_type_string, hover_msg: String::new()}));
    
    /* check whether it has lifetime parameter */
    if var_info.lifetime_param.is_none(){
        *x_cursor += x_update_type_seg;
        return;
    }
    /* render variable name segment with lifetime parameter . E.g. x: &'x i32 */
    let var_lp_string_old = var_info.to_string();
    println!("lp_string_old: {}", var_lp_string_old);
    // substitute lifetime parameter with variable name first letter
    let tick_index = var_lp_string_old.find("'").unwrap_or(0);
    // find all whitespace index
    let whitespace_idxs : Vec<usize> = var_lp_string_old.chars().enumerate()
                                        .filter(|&(_, c)| c.is_whitespace())
                                        .map(|(i, _)| i)
                                        .collect();
    let mut first_space_index = 0;
    for idx in whitespace_idxs.iter(){
        if *idx > tick_index{
            first_space_index = *idx;
            break;
        }
    }
    println!("tick_index: {}, first_space_index: {}", tick_index, first_space_index);
    let mut var_lp_string = format!("{}{}", &var_lp_string_old[..tick_index+1], &var_lp_string_old[first_space_index..]);
    println!("lp_string: {}", var_lp_string);
    // calculate local sub lifetime parameter index
    let first_ch: String = if var_info.name.chars().nth(0).unwrap() == '&' {var_info.name.chars().nth(1).unwrap().to_string()} else {var_info.name.chars().nth(0).unwrap().to_string()};
    let mut lp_idx = 1;
    if sub_lifetime_notation_dict.contains_key(&first_ch){
        lp_idx = sub_lifetime_notation_dict.get(&first_ch).unwrap().clone() + 1;
        sub_lifetime_notation_dict.insert(first_ch.clone(), lp_idx + 1);
    }
    else{
        sub_lifetime_notation_dict.insert(first_ch.clone(), 1);
    }
    // substitute lifetime parameter with local sub lifetime parameter index
    let mut sub_lifetime : String = format!("{}{}", &first_ch, lp_idx);
    if USE_VAR_NAME_AS_LP_NAME {
        println!("var_name: {} var_data_type: {}", var_info.name, var_info.data_type);
        // trying to get rid of '&mut' before real variable name
        let var_name_full: String = if var_info.name.chars().nth(0).unwrap() == '&' && !is_mutable_reference(&var_info.name) {var_info.name[1..].to_string()}
                                    else if is_mutable_reference(&var_info.name){var_info.name.clone().trim_start()[4..].to_string().trim_start().to_string()}
                                    else {var_info.name.clone()};
        // if this var has lifetime parameter but it's not a reference, then it must be generic type such as Struct<T>. In this case, no need to add outer lifetime parameter
        if !is_reference(var_info.data_type.as_str()){
            var_lp_string = format!("{}: {}", &var_info.name, &var_info.data_type);
        }
        else{
            var_lp_string.insert_str(tick_index+1, &format!("{}", &var_name_full));
        }
        sub_lifetime = format!("{}", &var_name_full);
        println!("var_name_full: {}", var_name_full);
    }
    else{
        var_lp_string.insert_str(tick_index+1, &format!("{}{}", &first_ch, lp_idx));
    }
    
    x_update_var_type_seg =  (var_lp_string.len() as u32) * FUNC_SIG_CHAR_X_SPACE;
    if var_info.subordinates.len() == 0 {
    render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
        FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor + Y_Space, segment: var_lp_string, hover_msg: String::new()}));
    }
    else {
        let var_name_string = var_info.name.clone().trim_start().to_string();
        println!("var_name_string: {}", var_name_string);
        x_update_var_type_seg =  (var_name_string.len() as u32) * 9;
        // generate hover message of info between master and subordinates
        let mut hover_message = format!("*{}* {} ", var_info.name, var_info.relationship);
        for subordinate in var_info.subordinates.iter(){
            hover_message += &format!("{}, ", subordinate.name);
        }
        hover_message += &format!("which all contribute to calculation of '{}.", var_info.lifetime_param.clone().unwrap());
        // only highlight variable name
        render_segments.insert(get_hash(),("func_signature_var_has_subordinate_template".to_string(),
        FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor + Y_Space, segment: var_name_string.clone(), hover_msg: hover_message}));
        // add variable type
        render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
        FuncSignatureRenderHolder{ x_val: *x_cursor + x_update_var_type_seg, y_val: *y_cursor + Y_Space, segment: var_lp_string[var_name_string.len()..].to_string(), hover_msg: String::new()}));
        x_update_var_type_seg += var_lp_string[var_name_string.len()..].to_string().len() as u32 * FUNC_SIG_CHAR_X_SPACE;
    }
    /* render lifetime compare inequality. E.g.  'x1 <= 'a */
    // if its struct or other generic type, use words "the scope of xxx  <= 'a"
    let mut lifetime_cmp_string = String::new();
    if !is_reference(var_info.data_type.as_str()){
        lifetime_cmp_string = format!("(scope of {}) <= '{}", &var_info.name, var_info.lifetime_param.clone().unwrap());
    }
    else{
        lifetime_cmp_string = format!("'{} <= '{}", &sub_lifetime, var_info.lifetime_param.clone().unwrap());
    }
    x_update_var_lp_cmp_seg =  (lifetime_cmp_string.len() as u32) * SIG_LT_CMP_CHAR_X_SPACE;
    render_segments.insert(get_hash(),("func_signature_LP_cmp_template".to_string(),
    FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor + 2 * Y_Space + 3, segment: lifetime_cmp_string,
                               hover_msg: format!("lifetime of {} should be contained by '{}", var_info.name, &var_info.lifetime_param.clone().unwrap())}));

    // update x_cursor
    *x_cursor += cmp::max(cmp::max(x_update_type_seg, x_update_var_type_seg), x_update_var_lp_cmp_seg) + 5;


}
/**
 * If this function is struct method, then render struct instance name, double colon, function name, dot, lifetime parameters and open left parenthesis.
 * * E.g. `MyStruct::my_method<'i,'a>(`
 */
fn helper_render_func_name_generic_type(func_info: & FuncSignatureSpec, x_cursor: &mut u32, y_cursor: &mut u32, render_segments: &mut BTreeMap<u32, (String, FuncSignatureRenderHolder)>, ret: &mut String, registry: &mut Handlebars){


    if func_info.is_not_static_struct_method{
        // render struct instance name
        let struct_instance_info = &func_info.input_variables[0];
        let struct_group_name = func_info.struct_group_name.clone().unwrap();
        assert!(struct_instance_info.data_type.find("self").is_some());
        let mut x_update:u32 = 0;
        x_update = (struct_group_name.len() as u32 ) * FUNC_SIG_CHAR_X_SPACE + 8;
        *ret += registry.render("func_signature_struct_instance_method_invoke_template", &FuncSignatureStructInstanceHolder{
            x_val: *x_cursor, y_val: *y_cursor, segment:struct_group_name,
            hover_msg: format!("{}{}{}{}Invocation of {}{}{} is dependent on struct instance {}{}{},{} which contributes to calculation of lifetime parameter as well.",
                                SPAN_BOLD_BEGIN, struct_instance_info.data_type, SPAN_END, NEW_LINE,
                                SPAN_BOLD_BEGIN, func_info.function_name, SPAN_END,
                                SPAN_BOLD_BEGIN, struct_instance_info.name, SPAN_END, NEW_LINE
            )
        }).unwrap().as_str();
        // render double colon, function name with lifetime annotation and opening parenthesis
        let mut func_name_str = String::from("::") + &func_info.function_name;
        // add up lifetime parameters
        if func_info.lifetime_param.is_some(){
            for (i, lp) in func_info.lifetime_param.clone().unwrap().iter().enumerate(){
                if i == 0{
                    func_name_str += &format!("<{}", lp);
                }
                else if i == func_info.lifetime_param.clone().unwrap().len() - 1 && func_info.lifetime_param.clone().unwrap().len() > 1{
                    func_name_str += &format!(",{}> (", lp);
                }
                else{
                    func_name_str += &format!(",{}", lp);
                }
                if i == func_info.lifetime_param.clone().unwrap().len() - 1 &&  func_info.lifetime_param.clone().unwrap().len() == 1{
                    func_name_str += &format!("> (");
                }
            }
        }

        render_segments.insert(get_hash(),("func_signature_code_param_template".to_string(),
                            FuncSignatureRenderHolder{ x_val: *x_cursor + x_update, y_val: *y_cursor, segment: func_name_str.clone(), hover_msg:String::new()}));
        x_update += func_name_str.len() as u32 * FUNC_SIG_CHAR_X_SPACE;
        *x_cursor += x_update;
    }
    else{
        let mut func_name_str = func_info.function_name.clone();
        let special_case_default_constructor = func_info.function_name == "default_constructor";
        if special_case_default_constructor{
            func_name_str = func_info.struct_group_name.clone().unwrap_or("Error: no struct name".to_string());
        }
        // add up lifetime parameters
        if func_info.lifetime_param.is_some(){
            for (i, lp) in func_info.lifetime_param.clone().unwrap().iter().enumerate(){
                if i == 0{
                    func_name_str += &format!("<{}", lp);
                }
                else if i == func_info.lifetime_param.clone().unwrap().len() - 1 &&  func_info.lifetime_param.clone().unwrap().len()  > 1{
                    if special_case_default_constructor{
                        func_name_str += &format!(",{}> {{", lp);
                    }
                    else{
                        func_name_str += &format!(",{}> (", lp);
                    }
                }
                else{
                    func_name_str += &format!(",{}", lp);
                }
                if i == func_info.lifetime_param.clone().unwrap().len() - 1 &&  func_info.lifetime_param.clone().unwrap().len() == 1{
                    if special_case_default_constructor{
                        func_name_str += &format!("> {{");
                    }
                    else{
                        func_name_str += &format!("> (");
                    }
                }
            }
        }
        render_segments.insert(get_hash(),("func_signature_code_param_template".to_string(),
                            FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor, segment: func_name_str.clone(), hover_msg: String::new()}));
        *x_cursor += (func_name_str.len() as u32) * FUNC_SIG_CHAR_X_SPACE;
    }
}

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
        x_update = (struct_instance_info.name.len() as u32 ) * FUNC_SIG_CHAR_X_SPACE + 8;
        ret += registry.render("func_signature_struct_instance_method_invoke_template", &FuncSignatureStructInstanceHolder{
            x_val:x_cursor, y_val:y_cursor, segment:struct_instance_info.name.clone(),
            hover_msg: format!("{}{}{}{}Invocation of {}{}{} is dependent on struct instance {}{}{},{} which contributes to calculation of lifetime parameter as well.",
                                SPAN_BOLD_BEGIN, struct_instance_info.data_type, SPAN_END, NEW_LINE,
                                SPAN_BOLD_BEGIN, func_info.function_name, SPAN_END,
                                SPAN_BOLD_BEGIN, struct_instance_info.name, SPAN_END, NEW_LINE
            )
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
                        FuncSignatureRenderHolder{ x_val: x_cursor - 6, y_val: y_cursor, segment: ")".to_string(), hover_msg: String::new()}));
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
    let mut var_sig_string = var_info.to_string();
    // //println!("name: {}", var_sig_string);
    if var_info.subordinates.len() == 0{
        x_update =  (var_sig_string.len() as u32) * FUNC_SIG_CHAR_X_SPACE;
        render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
        FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor, segment: var_sig_string, hover_msg: String::new()}));
    }
    else{
        var_sig_string = var_info.name.clone();
        x_update =  (var_sig_string.len() as u32) * 9;
        // generate hover message of info between master and subordinates
        let mut hover_message = format!("*{}* {} ", var_info.name, var_info.relationship);
        for subordinate in var_info.subordinates.iter(){
            hover_message += &format!("{}, ", subordinate.name);
        }
        hover_message += &format!("which all contribute to calculation of '{}.", var_info.lifetime_param.clone().unwrap());
        // only highlight variable name
        render_segments.insert(get_hash(),("func_signature_var_has_subordinate_template".to_string(),
        FuncSignatureRenderHolder{ x_val: *x_cursor, y_val: *y_cursor, segment: var_sig_string, hover_msg: hover_message}));
        // add variable type
        render_segments.insert(get_hash(),("func_signature_code_template".to_string(),
        FuncSignatureRenderHolder{ x_val: *x_cursor + x_update, y_val: *y_cursor, segment: format!(":{}", var_info.data_type), hover_msg: String::new()}));
        x_update += var_info.data_type.len() as u32 * FUNC_SIG_CHAR_X_SPACE;
    }


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
        // for reference created on the fly
        if var_info.lifetime_info.clone().unwrap().start == var_info.lifetime_info.clone().unwrap().end{
            let same_str = format!("{} is a temporary reference created on the fly, whose lifetime starts and ends one the same line on caller side.", var_info.name);
            (var_column_data.BODY_msg, var_column_data.CRPT_msg ,var_column_data.DRPT_msg) = (same_str.clone(), same_str.clone(),same_str);
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
        x: x_begin - 5,
        y: CODE_LINE_Y_START - 10,
        w: x_cursor - x_begin + 10 ,
        h: *max_y,
        hover_msg: format!("lifetime calculation block for '{}", lp_name)
    }).unwrap().as_str(),ret);
    (x_cursor - x_begin, ret)
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
    let func_signature_code_param_template =
    "       <text class=\"funcSigCodeParam\" x=\"{{x_val}}\" y=\"{{y_val}}\"> {{segment}} </text>\n";
    let func_signature_code_sep_template =
    "       <text class=\"funcSigCodeSep\" x=\"{{x_val}}\" y=\"{{y_val}}\"> {{segment}} </text>\n";
    let func_signature_code_template =
    "       <text class=\"funcSigCodeType\" x=\"{{x_val}}\" y=\"{{y_val}}\"> {{segment}} </text>\n";
    let func_signature_lp_cmp_template =
    "       <text class=\"lifetime tooltip-trigger\" x=\"{{x_val}}\" y=\"{{y_val}}\" data-tooltip-text=\"{{hover_msg}}\"> {{segment}} </text>\n";

    let func_signature_struct_instance_method_invoke_template =
    "       <text class=\"structInstance tooltip-trigger\" x=\"{{x_val}}\" y=\"{{y_val}}\" data-tooltip-text=\"{{hover_msg}}\">  {{segment}} </text>\n";

    let func_signature_var_has_subordinate_template =
    "       <text class=\"masterInstance tooltip-trigger\" x=\"{{x_val}}\" y=\"{{y_val}}\" data-tooltip-text=\"{{hover_msg}}\">  {{segment}} </text>\n";

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
        registry.register_template_string("func_signature_code_param_template", func_signature_code_param_template).is_ok()
    );

    assert!(
        registry.register_template_string("func_signature_code_sep_template", func_signature_code_sep_template).is_ok()
    );

    assert!(
        registry.register_template_string("func_signature_struct_instance_method_invoke_template", func_signature_struct_instance_method_invoke_template).is_ok()
    );

    assert!(
        registry.register_template_string("func_signature_var_has_subordinate_template", func_signature_var_has_subordinate_template).is_ok()
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
