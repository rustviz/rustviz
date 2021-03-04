// rust lib
use core::panic;
use std::collections::HashMap;
type Lines = std::io::Lines<std::io::BufReader<std::fs::File>>;
// svg_generator
use rustviz_lib::data::{
    ExternalEvent, Function, MutRef, Owner,
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
    // iterate over all parsed strings
    vars.iter().enumerate().map(|(hash, v)| {
        // fields = [type, is_mut, name] or [type, name]
        let fields: Vec<&str> = v
            .split(' ')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        // type and name are required fields
        if fields.is_empty() || fields.len() < 2 {
            print_usage_error(&fields);
            std::process::exit(1);
        }

        // returns tuple (key, item) : (String, ResourceAccessPoint)
        let name = if fields.len() > 2 { fields[2] } else { fields[1] };
        (name.to_string(), 
            // match type with possible ResourceAccessPoints
            match (fields[0], fields.len()) {
                ("Owner", 2) | ("Owner", 3) => ResourceAccessPoint::Owner(Owner {
                    hash: hash as u64 + 1,
                    name: get_name_field(&fields),
                    is_mut: get_mut_qualifier(&fields),
                }),
                ("MutRef", 2) | ("MutRef", 3) => ResourceAccessPoint::MutRef(MutRef {
                    hash: hash as u64 + 1,
                    name: get_name_field(&fields),
                    is_mut: get_mut_qualifier(&fields),
                }),
                ("StaticRef", 2) | ("StaticRef", 3) => ResourceAccessPoint::StaticRef(StaticRef {
                    hash: hash as u64 + 1,
                    name: get_name_field(&fields),
                    is_mut: get_mut_qualifier(&fields),
                }),
                ("Function", 2) => ResourceAccessPoint::Function(Function {
                    hash: hash as u64 + 1,
                    name: String::from(fields[1]),
                }),
                // default if invalid ResourceAccessPoint type
                // or incorrect number of qualifiers/fields
                _ => {
                    print_usage_error(&fields);
                    std::process::exit(1);
                }
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
    let (mut block_str, mut block) = (String::new(), false); // contents, parsing_block_or_not
    let (mut line_begin, mut line_end) = (0, 0); // used for block comments

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
                line_end = lnum as u64 + 1;
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

                    // do not count block comments towards valid lines of code
                    let diff = line_end - line_begin;
                    events.push((lnum as u64 - diff + 1, evt_str.to_string()));
                }
                else { //try next line
                    block = true;
                    line_begin = lnum as u64 + 1;
                    block_str += &line_string[i+2..];
                }
            }
        }
    }

    // separate all events in same line
    events.iter()
        .flat_map(|(lnum, evts)| { // flatten nested Vec<(u64, String)> into (u64, String)
            evts.split(',') // split all comma-separated events
                .map(|s| // make pair {line_num, event_string}
                    (lnum.to_owned(),
                    s.trim().to_string()) // trim whitespace
                )
                .filter(|e| !e.1.is_empty()) // remove empty cells
                .collect::<Vec<(u64, String)>>() // collect vec
        }
    ).collect::<Vec<(u64, String)>>() // return vec<(line_num, event_string)>
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

        match field.0 {
            "Bind" => vd.append_external_event(
                ExternalEvent::Bind{
                    from: get_resource(&vars, field.1),
                    to: get_resource(&vars, field.2)
                }, &(event.0 as usize)
            ),
            "Copy" => vd.append_external_event(
                ExternalEvent::Copy{
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
            _ => {
                eprintln!("{} is not a valid event.", field.0);
                std::process::exit(1);
            }
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

// Requires: Nothing
// Modifies: Nothing, unchanged
// Effects: Returns name string from field vector
fn get_name_field(fields: &Vec<&str>) -> String {
    (if fields.len() == 2 { fields[1] }
    else { fields[2] }).to_string()
}

// Requires: Nothing
// Modifies: Nothing, unchanged
// Effects: Returns mut bool from field vector
//          If qualifier not recognized, exit program
fn get_mut_qualifier(fields: &Vec<&str>) -> bool {
    if fields.len() == 2 { false }
    else if fields[1] == "mut" { true }
    else { 
        eprintln!(
            "Did not understand qualifier '{}' of variable '{}'!",
            fields[1], fields[2]
        );
        std::process::exit(1);
    }
}

// Requires: Nothing
// Modifies: Nothing
// Effects: Prints usage message to io::stderr
fn print_usage_error(fields: &Vec<&str>) {
    eprintln!("Incorrect variable formatting '{}'!\n{}{}{}{}{}",
        fields.join(" "),
        "Usage (':' denotes optional field) --",
        "\n\tOwner <:mut> <name>",
        "\n\tMutRef <:mut> <name>{<my_owner_name>}",
        "\n\tStaticRef <:mut> <name>{<my_owner_name>}",
        "\n\tFunction"
    );
}