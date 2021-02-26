// rust lib
use core::panic;
use std::collections::HashMap;
type Lines = std::io::Lines<std::io::BufReader<std::fs::File>>;
// svg_generator
use rustviz_lib::data::{
    ExternalEvent, Function, LifetimeTrait, MutRef, Owner,
    ResourceAccessPoint, StaticRef, VisualizationData, Visualizable
};

// Requires: Valid file path
//           Variables specified within BEGIN and END statements
// Modifies: Nothing, unchanged
// Effects: Parses variable definitions into HashMap with
//          {key, value} pair = {name, ResourceAccessPoint}
//          Returns std::io::Line iterator to file
pub fn parse_vars_to_map<P>(fpath: P) -> (
    Lines, HashMap<String, ResourceAccessPoint>
) where
    P: AsRef<std::path::Path>,
{
    // read file
    let mut lines =  rustviz_lib::svg_frontend::utils::read_lines(fpath)
        .expect("Unable to read file!");

    // check for unchanged template
    let mut line = lines.next()
        .expect("Oops, could not read. Empty file maybe?")
        .expect("Unable to read first line!");
    if line != "/* --- BEGIN Variable Definitions ---" {
        panic!("Uh oh! Do not change the first line!");
    }

    // parse variables definitions to string
    let mut vars_string = String::new();
    while {
        line = lines.next()
            .expect("Something went wrong! Do not remove BEGIN and END statements!")
            .expect("Unable to read file!");
        line != " --- END Variable Definitions --- */"
    } {
        vars_string.push_str(&line); // get vars to string
    }

    // split string into individual variables
    let vars: Vec<String> = vars_string.split(",")
        .map(|s| s.trim().to_string()) // trim whitespace
        .filter(|s| !s.is_empty()) // remove empty strings
        .collect();

    // return Lines iterator
    (lines, vec_to_map(vars))
}

// Requires: Well-formatted variable definitions in the form:
//           ResourceAccessPoint name{field1,field2}
// Modifies: Nothing, unchanged
// Effects: Uses strings to build HashMap with
//          {key, value} pair = {name, ResourceAccessPoint}
fn vec_to_map(vars: Vec<String>) -> HashMap<String, ResourceAccessPoint> {
    // TODO: set defined fields, check for invalid fields
    vars.iter().enumerate().map(|(hash, v)| {
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

// Requires: Non-empty file contents
// Modifies: Nothing, unchanged
// Effects: Uses Regex to parse DSL events in file,
//          compiles Vec<(line_num, event_string)>
pub fn extract_events(fin_lines: Lines) -> Vec<(u64, String)> {
    let mut events: Vec<(u64, String)> = Vec::new();
    let (mut block_str, mut line_begin, mut block) = (String::new(), 0, false); // contents, parsing_block_or_not
    for (lnum, line) in fin_lines.enumerate() {
        let line_string = line.expect(&format!("Unable to read line number {} from file!", lnum+1));
        if block { // if searching inside block comment
            if let Some(j) = line_string.find("}") {
                block_str.push_str(&line_string[..j]); // append line to contents
                // extract all comma-separated events and format into tuple
                for s in block_str.split(',') {
                    events.push((line_begin, s.trim().to_string()));
                }
                // clear
                block_str.clear();
                block = false;
            }
            else { // append line to contents
                block_str += line_string.trim();
            }
        }
        else {
            if let Some(i) = line_string.rfind("!{") {
                if let Some(j) = line_string[i..].rfind("}") {
                    let evt_str = &line_string[
                        i+2.. // i+2: skip !{
                        i+j // i+j: capture str from !{ to }
                    ].trim();
                    events.push((lnum as u64 + 1, evt_str.to_string()));
                }
                else { //try next line
                    block = true;
                    line_begin = lnum as u64 + 1;
                    block_str += &line_string[i+2..];
                }
            }
        }
    }
    events
}

// Requires: Well-formatted events, HashMap of ResourceAccessPoints
// Modifies: VisualizationData
// Effects: Creates ExternalEvents and appends to VisualizationData
pub fn add_events(
    vd: &mut VisualizationData,
    vars: HashMap<String, ResourceAccessPoint>,
    events: Vec<(u64, String)>
) {
    for event in events {
        // fmt: Event(from->to)
        let split: Vec<String> = event.1.split("->")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let mut field = ("","",""); // (name, from, to)
        if split.len() == 1 { // no "->"
            let idx = split[0].find("(").expect("Incorrect event formatting!");
            field.0 = &split[0][..idx]; // event
            field.1 = &split[0][idx+1..split[0].len()-1]; // name
        }
        else if split.len() == 2 { // has "->"
            // (event, name1, name2)
            let idx = split[0].find("(").expect("Incorrect event formatting!");
            field.0 = &split[0][..idx]; // event
            field.1 = &split[0][idx+1..]; // from
            field.2 = &split[1][..split[1].len()-1]; // to
        }
        else { // uh oh, wrong
            panic!("Incorrect formatting!\n\tUsage: <Event>(<from>-><to>)")
        }

        // println!("{:?}", field);
        match field.0 {
            "Duplicate" => vd.append_external_event(
                ExternalEvent::Duplicate{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                }, &(event.0 as usize)
            ),
            "Move" => vd.append_external_event(
                ExternalEvent::Move{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                },
                &(event.0 as usize)
            ),
            "StaticBorrow" => vd.append_external_event(
                ExternalEvent::StaticBorrow{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                },
                &(event.0 as usize)
            ),
            "MutableBorrow" => vd.append_external_event(
                ExternalEvent::MutableBorrow{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                },
                &(event.0 as usize)
            ),
            "StaticReturn" => vd.append_external_event(
                ExternalEvent::StaticReturn{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                },
                &(event.0 as usize)
            ),
            "MutableReturn" => vd.append_external_event(
                ExternalEvent::MutableReturn{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                },
                &(event.0 as usize)
            ),
            "PassByStaticReference" => vd.append_external_event(
                ExternalEvent::PassByStaticReference{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                },
                &(event.0 as usize)
            ),
            "PassByMutableReference" => vd.append_external_event(
                ExternalEvent::PassByMutableReference{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                },
                &(event.0 as usize)
            ),
            "InitializeParam" => vd.append_external_event(
                ExternalEvent::InitializeParam{
                    param: get_resource(&vars, field.1)
                        .expect("Expected Some variable, found None!")
                },
                &(event.0 as usize)
            ),
            "GoOutOfScope" => vd.append_external_event(
                ExternalEvent::GoOutOfScope{
                    ro: get_resource(&vars, field.1)
                        .expect("Expected Some variable, found None!")
                },
                &(event.0 as usize)
            ),
            _ => println!("{} is not a valid event.", field.0)
        }
    }
}

// Requires: Valid, existant ResourceAccessPoint name
// Modifies: Nothing, unchanged
// Effects: Returns clone of ResourceAccessPoint
fn get_resource(
    vars: &HashMap<String, ResourceAccessPoint>, name: &str
) -> Option<ResourceAccessPoint> {
    if name == "None" {
        None
    }
    else {
        match vars.get(name) {
            Some(res) => Some(res.clone()),
            None => panic!("Variable {} does not exist!", name)
        }
    }
}
