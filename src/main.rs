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
use rust_syn_parse_lib::syn_parse::{syn_parse, asource_gen};
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
            --- Generate stack info ---
    ************************************************************ */
    // println!("{:?}", syn_parse(&source_fname));
    let (_, mut color_info) = syn_parse(&source_fname).unwrap();

    /* ******************************************
            --- Parse main.rs file ---
    ****************************************** */
    let (contents, line_num, mut var_map) = parse::parse_vars_to_map(&main_fname);
    // hash_correction(&mut color_info, &mut var_map);
    let asource_str = asource_gen(&source_fname, &color_info, &mut var_map).unwrap();
    let asource_fname = path_to_ex.join("annotated_source.rs");
    match fs::write(&asource_fname, asource_str) {
        Ok(_) => println!("successfully wrote to {:?}", asource_fname),
        Err(_) => println!("failed to write to {:?}", asource_fname)
    }

    let events = parse::extract_events(contents, line_num);

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
    // let input_path = path_to_ex.join("input/")
    //     .into_os_string().into_string()
    //     .expect("Error in input file path!");
    let input_path = path_to_ex.clone()
    .into_os_string().into_string()
    .expect("Error in input file path!");

    let output_path = path_to_ex
        .into_os_string().into_string()
        .expect("Error in output file path!");
    svg_generation::render_svg(&input_path, &(output_path+"/"), &mut vd);
    return Ok(());
}