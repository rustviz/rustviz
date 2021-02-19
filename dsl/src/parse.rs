// rust lib
use std::collections::HashMap;
// svg_generator
use rustviz_lib::data::{
    ExternalEvent, Function, LifetimeTrait, MutRef, Owner,
    ResourceAccessPoint, StaticRef, VisualizationData, Visualizable
};
// crates.io
use regex::Regex;

pub fn extract_vars_to_map(fin: &String) -> HashMap<String, ResourceAccessPoint> {
    // Extract ResourceAccessPoints with regex
    let re_vars = Regex::new(r"/\*(?s:.)*?!\[{1}(?P<variables>(?s:.)[^]/\*]*)\]?")
        .expect("Something went wrong with the regex.");
    
    // capture text between ![ ]
    let mut vars: Vec<String> =
        re_vars.captures_iter(&fin)
            .map(|caps| caps["variables"].to_string())
            .collect();

    let vars: Vec<String> = vars.iter()
        .flat_map(move |s| s.split("\n")) // split by newline
        .map(|s| s.trim().to_string()) // trim whitespace
        .filter(|s| !s.is_empty()) // remove empty strings
        .collect();

    vec_to_map(vars) // return HashMap
}

pub fn vec_to_map(vars: Vec<String>) -> HashMap<String, ResourceAccessPoint> {
    // TODO: set defined fields, check for invalid fields
    vars.iter().enumerate().map(|(hash, v)| {
        // format = ResourceAccessPoint name{field1,field2}
        // fields = [type, name, Option<field1>, Option<field2>]
        let fields: Vec<&str> = v
            .split(|c| c == ' ' || c == '{' || c == ',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        
        // returns tuple (key, item) : (String, ResourceAccessPoint)
        (fields[1].to_string(), 
            // match type with possible ResourceAccessPoints
            match fields[0] {
                "Owner" => ResourceAccessPoint::Owner(Owner {
                    hash: hash as u64 + 1,
                    name: String::from(fields[1]),
                    is_mut: false,
                    lifetime_trait: LifetimeTrait::Copy,
                }),
                "MutRef" => ResourceAccessPoint::MutRef(MutRef {
                    hash: hash as u64 + 1,
                    name: String::from(fields[1]),
                    my_owner_hash: Some(1),
                    is_mut: false,
                    lifetime_trait: LifetimeTrait::Copy,
                }),
                "StaticRef" => ResourceAccessPoint::StaticRef(StaticRef {
                    hash: hash as u64 + 1,
                    name: String::from(fields[1]),
                    my_owner_hash: Some(1),
                    is_mut: false,
                    lifetime_trait: LifetimeTrait::Copy,
                }),
                "Function" => ResourceAccessPoint::Function(Function {
                    hash: hash as u64 + 1,
                    name: String::from(fields[1]),
                }),
                _ => panic!("Invalid ResourceAccessPoint \"{}\"", fields[0])
        })
    })
    .collect()
}

pub fn extract_events_to_string(fin: &String) -> Vec<String> {
    // Extract groups of "!{<events>}"
    let re = Regex::new(r"(//|/\*)(?s:.)*?!\{{1}(?P<events>(?s:.)[^}/\*]*)\}?")
        .expect("Something went wrong with the regex.");

    // collect groups into vector
    let events: Vec<String> =
        re.captures_iter(&fin)
            .map(|caps| caps["events"].to_string())
            .collect();

    // extract and format into individual events
    events.iter()
        .flat_map(move |str| str.split(",")) // split around commas
        .map(|s| s.trim().to_string()) // remove surrounding whitespace
        .collect::<Vec<String>>() // collect into vec of strings
}

pub fn add_events(vd: &mut VisualizationData, events: Vec<String>) {
    for e in events {
        if let Some(idx) = e.find("(") {
            match &e[..idx] {
                "Duplicate" => (*vd).append_external_event(
                    ExternalEvent::Duplicate{from: None, to: None}, &(0 as usize)
                ),
                _ => println!("{} is not a valid event.", &e[..idx])
            }
        }
        else {
            println!("{} is not a valid event.", e);
        }
    }
}