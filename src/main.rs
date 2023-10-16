use std::process::exit;
// rust lib
use std::{
    env, path::Path,
    collections::BTreeMap
};
// svg_generator
mod parse;
use rustviz_lib::svg_frontend::{lifetime_vis::*, utils};
use rustviz_lib::svg_frontend::lifetime_vis::lifetime_render::*;
use rustviz_lib::svg_frontend::{svg_generation, lifetime_vis::{self, lifetime_parse::*}};
use rustviz_lib::data::{VisualizationData, self};
use handlebars::Handlebars;
use rand::Rng;
use std::cmp;


fn main() {
    // verify usage
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage Error: cargo run <filename>"); 
        return;
    }

    let path_to_ex = Path::new("examples").join(&args[1]);
    if !path_to_ex.is_dir() {
        println!("Error: no corresponding directory exists in examples/!");
        return;
    }

    let filename = path_to_ex.join("main.rs");
    if !Path::new(&filename).is_file() {
        println!("Example source file (main.rs) not found in {:?}!", &filename);
        return;
    }

    let source_filename = path_to_ex.join("source.rs");
    if !Path::new(&source_filename).is_file() {
        println!("source.rs not found in {:?}!", &source_filename);
        return;
    }

    /* ******************************************
            --- Parse main.rs file ---
    ****************************************** */
    let (contents, line_num, var_map) = parse::parse_vars_to_map(filename.clone());
    // println!("var_map: {:?}", var_map);
    let events = parse::extract_events(contents, line_num);
    /* ******************************************
            --- Build VisualizationData ---
    ****************************************** */
    // println!("{:?}", events);

    

    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new(),
        lifetimes: None
        // lifetimes: todo!(),
    };
    parse::add_events(&mut vd, var_map, events);
    // println!("{:#?}", vd);

    


    /* ******************************************
            --- Render SVG images ---
    ****************************************** */
    let input_path = path_to_ex.join("input/")
        .into_os_string().into_string()
        .expect("Error in input file path!");
    let output_path = path_to_ex
        .into_os_string().into_string()
        .expect("Error in output file path!");
    // println!("in: {}\nout: {}", input_path, output_path);
    svg_generation::render_svg(&input_path, &(output_path+"/"), &mut vd);
}


/* for testing */

fn test_add_output_vars_and_lifetime_num(func_info: &mut FuncSignatureSpec){
    let mut rng = rand::thread_rng();
    for idx in 0..func_info.output_variables.len(){
        let ch = (('a' as u8) + (idx as u8)) as char;
        func_info.output_var_called_names.push_back(ch.to_string());
        func_info.output_variables[idx].name = ch.to_string();
    }
    for  var in func_info.input_variables.iter_mut(){
        if var.lifetime_param.is_some(){
            let st = rng.gen_range(2..10);
            var.lifetime_info = Some(LifetimeStartEndPoint { start: st, end: st + rng.gen_range(1..6) })
        }
    }
    for  var in func_info.output_variables.iter_mut(){
        if var.lifetime_param.is_some(){
            let st = rng.gen_range(2..10);
            var.lifetime_info = Some(LifetimeStartEndPoint { start: st, end: st + rng.gen_range(1..6) })
        }
    }
}

fn test_gather_input_output_var(func_info: &mut FuncSignatureSpec) -> Vec<VariableSpec>{
    let mut vars: Vec<VariableSpec> = Vec::new();
    let mut data_hash : u32 = 1;
    for elem in func_info.input_variables.iter_mut(){
        if elem.lifetime_info.is_some(){
            elem.data_hash = Some(data_hash);
            data_hash += 1;
            vars.push(elem.clone());
        }
       
    }
    for elem in func_info.output_variables.iter_mut(){
        if elem.lifetime_info.is_some(){
            elem.data_hash = Some(data_hash);
            data_hash += 1;
            vars.push(elem.clone())
        }
    }
    vars
}
