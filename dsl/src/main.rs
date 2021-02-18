// rust lib
use std::{
    env, collections::BTreeMap
};
// svg_generator
use rustviz_lib::svg_frontend::{
    svg_generation, utils
};
use rustviz_lib::data::{
    ExternalEvent, LifetimeTrait, ResourceAccessPoint,
    Owner, MutRef, StaticRef, Function, VisualizationData, Visualizable
};
// crates.io
use regex::Regex;

fn main() {
    let args: Vec<String> = env::args().collect();

    // verify usage
    if args.len() != 2 {
        println!(r"Usage Error: cargo run <filename>"); 
        return;
    }

    let filename = format!("../svg_generator/examples/{}/main.rs" , &args[1]);
    let contents = utils::read_file_to_string(filename)
        .expect("Something went wrong reading the file!");

    /* ******************************************
            --- Parse main.rs file ---
    ****************************************** */
    // TODO: extract ResourceAccessPoints
    // either
    //      1) use lazy regex
    //      2) assume properly declared

    // Extract groups of "!{<events>}"
    let re = Regex::new(r"(//|/\*)(?s:.)*?!\{{1}(?P<events>(?s:.)[^}/\*]*)\}?")
        .expect("Something went wrong with the regex.");

    // collect groups into vector
    let mut events: Vec<String> =
        re.captures_iter(&contents)
            .map(|caps| caps["events"].to_string())
            .collect();

    // extract and format into individual events
    let events = events.iter()
        .flat_map(move |str| str.split(",")) // split around commas
        .map(|s| s.trim().to_string()) // remove surrounding whitespace
        .collect::<Vec<String>>(); // collect into vec of strings

    /* ******************************************
            --- Build VisualizationData ---
    ****************************************** */
    let mut vd = VisualizationData {
        timelines: BTreeMap::new(),
        external_events: Vec::new(),
        preprocess_external_events: Vec::new(),
        event_line_map: BTreeMap::new()
    };

    // TODO: match events to ExternalEvents and implement line numbers
    for e in events {
        if let Some(idx) = e.find("(") {
            match &e[..idx] {
                "Duplicate" => vd.append_external_event(
                    ExternalEvent::Duplicate{from: None, to: None}, &(0 as usize)
                ),
                _ => println!("{} is not a valid event.", &e[..idx])
            }
        }
        else {
            println!("{} is not a valid event.", e);
        }
    }

    /* ******************************************
            --- Render SVG images ---
    ****************************************** */
    let input_path = format!("../svg_generator/examples/{}/input/", &args[1]);
    let output_path = format!("../svg_generator/examples/{}/", &args[1]);
    svg_generation::render_svg(&input_path, &output_path, &mut vd);
}
