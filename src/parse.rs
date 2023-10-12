// rust lib
use std::process::exit;
use std::collections::HashMap;
type Lines = std::io::Lines<std::io::BufReader<std::fs::File>>;
// svg_generator
use rustviz_lib::data::{
    ExternalEvent, Function, MutRef, Owner, Struct,
    ResourceAccessPoint, StaticRef, VisualizationData, Visualizable
};

/* Requires: Valid file path
             Variables specified within BEGIN and END statements
   Modifies: Nothing, unchanged
   Effects: Parses variable definitions into HashMap with
            {key, value} pair = {name, ResourceAccessPoint}
            Returns std::io::Line iterator to file
*/
pub fn parse_vars_to_map<P>(fpath: P) -> (
    Lines, u64, HashMap<String, ResourceAccessPoint>
) where
    P: AsRef<std::path::Path>,
{
    // read file
    let mut fin_lines =  rustviz_lib::svg_frontend::utils::read_lines(fpath)
        .expect("Unable to read file!");

    // check for unchanged template
    let mut line = fin_lines.next()
        .expect("Oops, could not read. Empty file maybe?")
        .expect("Unable to read first line!");
    if line != "/* --- BEGIN Variable Definitions ---" {
        eprintln!("Uh oh! Do not change the first line!");
        exit(1);
    }

    // parse variables definitions to string
    let mut vars_string = String::new();
    let mut num_lines = 2; // tracks curr line num
    while {
        line = fin_lines.next()
            .expect("Something went wrong! Do not remove BEGIN and END statements!")
            .expect("Unable to read file!");
        line != "--- END Variable Definitions --- */"
    } {
        num_lines += 1;
        vars_string.push_str(&line); // get vars to string
    }

    // split string into individual variables
    let vars: Vec<String> = vars_string.split(';')
        .map(|s| s.trim().to_string()) // trim whitespace
        .filter(|s| !s.is_empty()) // remove empty strings
        .collect();

    // return Lines iterator
    (fin_lines, num_lines, vec_to_map(vars))
}

/* Requires: Well-formatted variable definitions in the form:
             ResourceAccessPoint name{field1,field2}
   Modifies: Nothing, unchanged
   Effects: Uses strings to build HashMap with
            {key, value} pair = {name, ResourceAccessPoint}
*/
fn vec_to_map(vars_str: Vec<String>) -> HashMap<String, ResourceAccessPoint> {
    // iterate over all parsed strings
    let mut vars_map = HashMap::<String, ResourceAccessPoint>::new();

    let mut hash : u64 = 1;
    for v in vars_str.iter() {
        // fields = [type, is_mut, name] or [type, name]
        let fields: Vec<&str> = v
            .split(|c| c == ' ' || c == ',' || c == '{' || c == '}')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        // type and name are required fields
        if fields.is_empty() || fields.len() < 2 {
            print_var_usage_error(&fields);
            exit(1);
        }

        // returns tuple (key, item) : (String, ResourceAccessPoint)
        let name = (if fields.len() > 2 { fields[2] } else { fields[1] }).to_string();
        // match type with possible ResourceAccessPoints
        match (fields[0], fields.len()) {
            ("Owner", 2) | ("Owner", 3) => {
                vars_map.insert(
                    name,
                    ResourceAccessPoint::Owner(Owner {
                        hash: hash,
                        name: get_name_field(&fields),
                        is_mut: get_mut_qualifier(&fields),
                    })
                );
            },
            ("MutRef", 2) | ("MutRef", 3) => {
                vars_map.insert(
                    name,
                    ResourceAccessPoint::MutRef(MutRef {
                        hash: hash,
                        name: get_name_field(&fields),
                        is_mut: get_mut_qualifier(&fields),
                    })
                );
            },
            ("StaticRef", 2) | ("StaticRef", 3) => {
                vars_map.insert(
                    name,
                    ResourceAccessPoint::StaticRef(StaticRef {
                        hash: hash,
                        name: get_name_field(&fields),
                        is_mut: get_mut_qualifier(&fields),
                    })
                );
            },
            ("Function", 2) => {
                vars_map.insert(
                    name,
                    ResourceAccessPoint::Function(Function {
                        hash: hash,
                        name: String::from(fields[1]),
                    })
                );
            },
            ("Struct", _) => get_structs(&mut hash, &fields, &mut vars_map),
            // default to error if invalid ResourceAccessPoint type
            // or incorrect number of qualifiers/fields
            _ => {
                print_var_usage_error(&fields);
                exit(1);
            }
        }

        hash += 1;
    }

    vars_map
}

// Requires: Non-empty file contents
// Modifies: Nothing, unchanged
// Effects: Uses Regex to parse DSL events in file,
//          compiles Vec<(line_num, event_string)>
pub fn extract_events(
    fin_lines: Lines,
    main_line: u64,
) -> Vec<(u64, String)> {
    let mut events: Vec<(u64, String)> = Vec::new();
    let (mut block_str, mut block) = (String::new(), false); // contents, parsing_block_or_not
    let (mut line_begin, mut line_end) = (0, 0); // used for block comments

    for (lnum, line) in fin_lines.enumerate() {
        let line_string = line.expect(&format!("Unable to read line number {} from file!", lnum+1));
        if block { // if searching inside block comment
            // if '!{' found before '}', print error msg
            if let Some(_) = line_string.find("!{") {
                delimitation_err(line_begin+main_line);
            }
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
    // if block is still true, closing '}' was never found
    if block { delimitation_err(line_begin+main_line); }

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

/* Requires: Well-formatted events, HashMap of ResourceAccessPoints
   Modifies: VisualizationData
   Effects: Creates ExternalEvents and appends to VisualizationData
*/
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

        let mut field = Vec::new();
        if split.len() == 1 { // no "->"
            let idx = split[0].find("(").expect(&event_usage_err());
            field.push(&split[0][..idx]); // event
            field.push(&split[0][idx+1..split[0].len()-1]); // name
        }
        else if split.len() == 2 { // has "->"
            // [event, name1, name2]
            let idx = split[0].find("(").expect(&event_usage_err());
            field.push(&split[0][..idx]); // event
            field.push(&split[0][idx+1..]); // from
            field.push(&split[1][..split[1].len()-1]); // to
        }
        else { // uh oh, wrong
            eprintln!("{}", event_usage_err());
            exit(1);
        }

        // check for any empty fields
        for f in &field {
            if f.is_empty() {
                eprintln!("{}", event_usage_err());
                exit(1);
            }
        };

        match field[0] {
            "Bind" => vd.append_external_event(
                ExternalEvent::Bind{
                    from: get_resource(&vars, "None"),
                    to: get_resource(&vars, field[1])
                }, &(event.0 as usize)
            ),
            "Copy" => vd.append_external_event(
                ExternalEvent::Copy{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                }, &(event.0 as usize)
            ),
            "Move" => vd.append_external_event(
                ExternalEvent::Move{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                },
                &(event.0 as usize)
            ),
            "StaticBorrow" => vd.append_external_event(
                ExternalEvent::StaticBorrow{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                },
                &(event.0 as usize)
            ),
            "MutableBorrow" => vd.append_external_event(
                ExternalEvent::MutableBorrow{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                },
                &(event.0 as usize)
            ),
            "StaticDie" => vd.append_external_event(
                ExternalEvent::StaticDie{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                },
                &(event.0 as usize)
            ),
            "MutableDie" => vd.append_external_event(
                ExternalEvent::MutableDie{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                },
                &(event.0 as usize)
            ),
            "PassByStaticReference" => vd.append_external_event(
                ExternalEvent::PassByStaticReference{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                },
                &(event.0 as usize)
            ),
            "PassByMutableReference" => vd.append_external_event(
                ExternalEvent::PassByMutableReference{
                    from: get_resource(&vars, field[1]),
                    to: get_resource(&vars, field[2])
                },
                &(event.0 as usize)
            ),
            "InitRefParam" => vd.append_external_event(
                ExternalEvent::InitRefParam{
                    param: get_resource(&vars, field[1])
                        .expect("Expected Some variable, found None!")
                },
                &(event.0 as usize)
            ),
            "InitOwnerParam" => vd.append_external_event(
                ExternalEvent::Move{
                    from: get_resource(&vars, "None"),
                    to: get_resource(&vars, field[1])
                },
                &(event.0 as usize)
            ),
            "GoOutOfScope" => vd.append_external_event(
                ExternalEvent::GoOutOfScope{
                    ro: get_resource(&vars, field[1])
                        .expect("Expected Some variable, found None!")
                },
                &(event.0 as usize)
            ),
            _ => {
                eprintln!("{} is not a valid event.", field[0]);
                println!("{}", event_usage_err());
                exit(1);
            }
        }
    }
}

/* Requires: Valid, existant ResourceAccessPoint name
   Modifies: Nothing, unchanged
   Effects: Returns clone of ResourceAccessPoint
*/
fn get_resource(
    vars: &HashMap<String, ResourceAccessPoint>, name: &str
) -> Option<ResourceAccessPoint> {
    if name == "None" { None }
    else {
        match vars.get(name) {
            Some(res) => Some(res.clone()),
            None => {
                eprintln!(
                    "Variable '{}' does not exist! \
                    Name must match definition.", name
                );
                exit(1);
            }
        }
    }
}

/* Requires: Nothing
   Modifies: Nothing, unchanged
   Effects: Returns name string from field vector
*/
fn get_name_field(fields: &Vec<&str>) -> String {
    (if fields.len() == 2 { fields[1] }
    else { fields[2] }).to_string()
}

/* Requires: Nothing
   Modifies: Nothing, unchanged
   Effects: Returns mut bool from field vector
            If qualifier not recognized, exit program
*/
fn get_mut_qualifier(fields: &Vec<&str>) -> bool {
    if fields.len() == 2 { false }
    else if fields[1] == "mut" { true }
    else { 
        eprintln!(
            "Did not understand qualifier '{}' of variable '{}'! \
            Field must either be empty or 'mut'.",
            fields[1], fields[2]
        );
        exit(1);
    }
}

/* Requires: Non-empty fields vector
   Modifies: Current hash number, ResourceAccessPoint HashMap
   Effects: Parses struct instance + member variables into independent
            ResourceAccessPoints and inserts into vars HashMap
*/
fn get_structs(
    hash: &mut u64,
    fields: &Vec<&str>,
    vars_map: &mut HashMap<String, ResourceAccessPoint>
) {
    let b = fields[1] == "mut"; // mut declared for owner struct

    // assumption: mut qualifier immediately followed by name
    let parent_name = (if b {fields[2]} else {fields[1]}).to_string();
    // push owner struct
    vars_map.insert(
        parent_name.clone(), // key
        ResourceAccessPoint::Struct(Struct { // value
            owner: *hash,
            hash: *hash,
            name: parent_name.clone(),
            is_mut: if b {true} else {false},
            is_member: false
        })
    );

    // push all member variables
    // TODO: error checking
    let owner_hash = *hash;
    let mut idx = if b {3} else {2}; // members start at index 2,3
    while idx < fields.len() {
        *hash += 1;
        let cond = fields[idx] == "mut";
        let v_name = parent_name.clone() + "." + (
            if cond {
                if idx+1 >= fields.len() {
                    eprintln!("Expected variable name after 'mut' qualifier, found nothing!");
                    exit(1);
                }
                fields[idx+1]
            } else {
                fields[idx]
            }
        );

        // begin new def
        vars_map.insert(
            v_name.clone(),
            ResourceAccessPoint::Struct(Struct {
                owner: owner_hash,
                hash: *hash,
                name: v_name,
                is_mut: if cond {true} else {false},
                is_member: true
            })
        );
        
        idx = if cond {idx+2} else {idx+1};
    }
}

/* Requires: Nothing
   Modifies: Nothing
   Effects: Prints variable usage message to io::stderr
*/
fn print_var_usage_error(fields: &Vec<&str>) {
    eprintln!("Incorrect variable formatting '{}'!\
        \nUsage (':' denotes optional field):\
        \n\tOwner <:mut> <name>\
        \n\tMutRef <:mut> <name>\
        \n\tStaticRef <:mut> <name>\
        \n\tFunction <name>",
        fields.join(" ")
    );
}

/* Requires: Nothing
   Modifies: Nothing
   Effects: Returns event usage message as String
*/
fn event_usage_err() -> String {
    String::from(
        "ExternalEvents Usage:\
        \n\tFormat: <event_name>(<from> -> <to>)\
        \n\t    e.g.: // !{ PassByMutableReference(a->Some_Function()), ... }\
        \n\tNote: GoOutOfScope and InitRefParam require only the <from> parameter\
        \n\t    e.g.: // !{ GoOutOfScope(x) }"
    )
}

// Requires: Nothing
// Modifies: Nothing
// Effects: Prints delimitation error message and exits with code 1
fn delimitation_err(line_num: u64) {
    eprintln!(
        "Found unterminated delimitation on line {}! \
        Please close with }}.",
        line_num
    );
    exit(1);
}