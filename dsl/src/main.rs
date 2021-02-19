// rust lib
use std::{
    env, path::Path,
    collections::BTreeMap
};
// svg_generator
mod parse;
use rustviz_lib::svg_frontend::{
    svg_generation, utils
};
use rustviz_lib::data::{
    VisualizationData
};

fn main() {
    // verify usage
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage Error: cargo run <filename>"); 
        return;
    }

    let path_to_ex = Path::new("../svg_generator/examples").join(&args[1]);
    if !path_to_ex.is_dir() {
        println!("Error: no corresponding directory exists in svg_generator/examples/!");
        return;
    }

    let filename = path_to_ex.join("main.rs");
    if !Path::new(&filename).is_file() {
        println!("Example source file (main.rs) not found in {:?}!", &filename);
        return;
    }
    let contents = utils::read_file_to_string(filename).unwrap(); // read to single string

    /* ******************************************
            --- Parse main.rs file ---
    ****************************************** */
    let var_map = parse::extract_vars_to_map(&contents);
    let events = parse::extract_events_to_string(&contents);

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
}
