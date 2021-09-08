// rust lib
use std::{
    env, path::Path,
    collections::BTreeMap,
    process
};
// svg_generator
mod parse;
use rustviz_lib::svg_frontend::svg_generation;
use rustviz_lib::data::VisualizationData;
// use rust_syn_parse_lib::syn_parse::{syn_parse, header_gen_str};
use std::fs;
use std::io::{Write, BufReader, BufRead, Error};
use std::io::prelude::*;

fn main() -> Result<(), Error> {
    // verify usage
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage Error: cargo run <filename>"); 
        return Ok(());
    }

    let path_to_ex = Path::new("examples").join(&args[1]);
    if !path_to_ex.is_dir() {
        println!("Error: no corresponding directory exists in examples/!");
        return Ok(());
    }

    let source_fname = path_to_ex.join("source.rs");
    let main_fname = path_to_ex.join("main.rs");
    if !Path::new(&source_fname).is_file() {
        println!("Example source file (main.rs) not found in {:?}!", &source_fname);
        return Ok(());
    }
    /* ***********************************************************
            --- Generate RAP Header and stack info ---
    ************************************************************ */
    // header is required for timeline order
    // TODO: show all RAPs instead of ones involved in the events
    //TODO: allow the stack_items reference header_info 
    // if let Ok((header_info, color_info)) = syn_parse(&source_fname) {
    //     // require main.rs and header to be generated if not provided
    //     if !Path::new(&main_fname).exists() {
    //         let header_str = header_gen_str(&header_info);
    //         //TODO: why is this ????
    //         let mut output = fs::File::create(&main_fname)?;
    //         let mut buffer = String::new();
    //         let mut f = fs::File::open(source_fname)?;
    //         let _ = f.read_to_string(&mut buffer)?;
    //         //TODO: WTF is this
    //         write!(output, "{}", format!("{}", header_str))?;
    //         write!(output, "{}", format!("{}", buffer))?;
    //     }
    // }

    /* ******************************************
            --- Parse main.rs file ---
    ****************************************** */
    let (contents, line_num, var_map) = parse::parse_vars_to_map(main_fname);
    // println!("{:?}", var_map);
    let events = parse::extract_events(contents, line_num);
    // println!("{:?}", events);
    // return Ok(());

    /* ******************************************
            --- Build VisualizationData ---
    ****************************************** */
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new()
    };
    parse::add_events(&mut vd, var_map, events);
    /* ******************************************
            --- Render SVG images ---
    ****************************************** */
    let input_path = path_to_ex.join("input/")
        .into_os_string().into_string()
        .expect("Error in input file path!");
    let output_path = path_to_ex
        .into_os_string().into_string()
        .expect("Error in output file path!");
    svg_generation::render_svg(&input_path, &(output_path+"/"), &mut vd);
    return Ok(());
}
