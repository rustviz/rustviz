
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
    println!("var_map: {:?}", var_map);
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
    println!("{:#?}", vd);

    


    /* ******************************************
            --- Render SVG images ---
    ****************************************** */
    let input_path = path_to_ex.join("input/")
        .into_os_string().into_string()
        .expect("Error in input file path!");
    let output_path = path_to_ex
        .into_os_string().into_string()
        .expect("Error in output file path!");
    println!("in: {}\nout: {}", input_path, output_path);
    svg_generation::render_svg(&input_path, &(output_path+"/"), &mut vd);
}


/* test code for funcion signature parsing */

    // let mut s1 = "  fn Foo::foo<'i, u32, 'a>(t: mut i32, baz: &'i mut Vec<int>".to_string();
    // let mut s2 = "                           z: &mut (u32, Hash<i32, u32>), bar: &'a  mut   Hash<u32, i32>) -> &'a (i32, Vec<i32, u32>)".to_string();
    // let mut vs = FuncSignatureSpec::new();
    // println!("{:?}", parse_one_line_variables(s1, &mut vs));
    // println!("{:?}", parse_one_line_variables(s2.clone(), &mut vs));
    // let mut s = " z: &mut (u32, Hash<i32, u32>), bar: &'a  mut   Hash<u32, i32> -> (i32, Vec<i32, u32>)".to_string();
    // println!("{:?}", vs);
// let path = "/Users/alaric66/Desktop/rustviz-lifetime-feat/src/examples/copy/source.rs".to_string();
//     let mut source_func_signatures_infos: BTreeMap<String, FuncSignatureSpec> = BTreeMap::new();
//     parse_all_function_signature(&path, &mut source_func_signatures_infos);
//     println!("{:#?}", source_func_signatures_infos);

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


// let tmp = parse_variable_single_cell("& 'i mut self".to_string(), true);
//     // println!("{:?}", tmp);
//     // exit(0);
//     let mut registry = Handlebars::new();
//     let mut fs = FuncSignatureSpec::new();
//     fs.function_name = "conv".to_string();
//     fs.struct_group_name = Some("MagicBox".to_string());
//     let path_main =  "/Users/alaric66/Desktop/rustviz-lifetime-feat/src/examples/alaric_struct_cmp/main.rs".to_string();
//     let path = "/Users/alaric66/Desktop/rustviz-lifetime-feat/src/examples/alaric_struct_cmp/source.rs".to_string();
//     fs.replenish_parse(path);
//     fs.update_input_names_main_rs(path_main);
//     fs.update_struct_instance_name();

//     test_add_output_vars_and_lifetime_num(&mut fs);
//     fs.sync_var_name_with_invoked_name();
//     println!("{:?}", fs);

//     let (width, y_end, func_sig_str) = render_function_lifetime_signature(&fs, &mut registry);
//     let vars = test_gather_input_output_var(&mut fs);

//     // render different lifetime parameter
//     let mut tm = func_sig_str.clone();
//     let mut x_begin : u32 = 0;
//     // calculate max y val beforehand
//     let mut max_y = 0;
//     for var in &vars{
//         if let Some(lp_info) = &var.lifetime_info{
//             max_y = cmp::max(lp_info.end, max_y)
//         }
//     }
//     max_y = CODE_LINE_Y_START + (max_y - 1) * CODE_VERTICAL_LINE_SPACE + 15;
//     if let Some(lps) = fs.lifetime_param.clone(){
//         for (lifetime_hash,mut lp) in lps.into_iter().enumerate(){
//             remove_lifetime_tick(&mut lp);
//             println!("lp: {}", lp);
//             let mut var_same_lifetime : Vec<VariableSpec> = Vec::new();
//             for v in &vars{
//                 if let Some(v_lifetime) = v.lifetime_param.clone(){
//                     if v_lifetime == lp {
//                         var_same_lifetime.push(v.clone());
//                     }
//                 }
//             }
//             println!("this batch: {:?}", var_same_lifetime);
//             let (w2, column_str) = render_lifetime_columns_one_for_lifetime_parameter(&var_same_lifetime, &registry, x_begin, &lifetime_hash, &max_y);
//             x_begin += w2 + 20;
//             tm = tm + &column_str;
//             // render lifetime region square
//         }
//         let dash_line_str = render_dashed_number_line(vars,x_begin, &registry);
//         tm = tm + &dash_line_str;
//     }

//     utils::create_and_write_to_file(&tm, "/Users/alaric66/Desktop/rustviz-lifetime-feat/svg.txt");