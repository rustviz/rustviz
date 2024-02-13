use std::{collections::{BTreeMap, VecDeque, HashSet},process::exit, fs, hash::Hash};
use std::io::{self, Error};
use std::io::BufReader;
use quote::__private::ext::RepToTokensExt;
use rand::seq::IteratorRandom;
use syn::{parse_quote};
use crate::svg_frontend::{lifetime_vis::LifetimeType, utils };
use regex::Regex;





/** Require: first char starts with '( 
 *  Return: object inside the parenthesis, and index of the closing parenthesis
 * 
 */
fn extract_string_from_parenthesis(raw_input_string : &String) -> (Option<&str>, Option<usize>){
    if raw_input_string.chars().nth(0) != Some('('){
        (None, None)
    }
    else{
        if let Some(index_p_close) = raw_input_string.find(')'){
            ( raw_input_string.get(1..index_p_close), Some(index_p_close) )
        }
        else{
            (None, None)
        }
    }
}




use std::io::BufRead;
use super::*;
/**
 * Effect: fill `source_func_signatures_infos` with key value pair (function_name -> `FuncSignatureSpec`)
 * Now has considered struct method parsing
 * Required: function signature 1) -> must be at the same line of the closing parenthesis of the function signature
 */
pub fn parse_all_function_signature(source_file_pathname: &str, source_func_signatures_infos : &mut BTreeMap<String, FuncSignatureSpec>)  {
    // Read the source.rs file
    let file = fs::File::open(source_file_pathname).expect((String::from("error opening ") + &source_file_pathname.to_string()).as_str());
    let reader = BufReader::new(file);
    let mut lines_buffer : Vec<String> = Vec::new();
    for _line in reader.lines() {
        match _line {
            Ok(line) => lines_buffer.push(line),
            Err(err) => {
                eprintln!("{}",err);
                break;
            }
        }
    }
    // for i in 0..lines_buffer.len(){
    //     println!("{}: {}", i, lines_buffer[i])
    // }
    let mut ln  = 0;
    let mut is_struct_method = false;
    let mut struct_group_name = String::new();
    let mut struct_group_brackets_cnt = 0;
    let mut found_first_impl_opening_braket = false;
    let mut struct_group_lp_vec : Vec<String> = Vec::new();
    while ln < lines_buffer.len(){
        // find closing ')'
        let mut line = lines_buffer[ln].clone();
        let mut flag = false;
        // find impl block and parse struct group name
        if line.find("impl").is_some(){
            assert!(!found_first_impl_opening_braket);
            // clear previous struct group name
            struct_group_name = String::new();
            assert!(struct_group_brackets_cnt == 0);
            let lncpy = line.trim();
            let re = Regex::new(r"<([^>]+)>").unwrap();
            // find all lifetime parameters, if there is any
            if let Some(captures) = re.captures(lncpy){
                if let Some(first_match) = captures.get(1){
                    let lps = first_match.as_str();
                    struct_group_lp_vec = lps.split(",").map(|c| c.trim().to_string()).collect();
                }
            }
            let filtered_str = re.replace_all(lncpy, "").to_string();
            let tmp_v : Vec<&str> = filtered_str.split_whitespace().into_iter().collect();
            // consider impl block like this : impl<'i> myStruct<'i> {..}
            if tmp_v[0].find("impl").is_none(){
                eprintln!("something wrong with impl syntax!");
                exit(0);
            }
            let name_raw = tmp_v[1];
            for ch in name_raw.chars(){
                if ch == '{'{
                    break
                }
                struct_group_name.push(ch)
            }
            is_struct_method = true;
        }

        // update brackets cnt when inside struct group, in order to track when struct group ends
        if is_struct_method{
            if line.find('{').is_some(){
                struct_group_brackets_cnt += 1;
                found_first_impl_opening_braket = true;
            }
            if line.find('}').is_some(){
                struct_group_brackets_cnt -= 1;
            }
        }
        // if cnt is zero, no longer in struct group
        if struct_group_brackets_cnt == 0 && found_first_impl_opening_braket{
            is_struct_method = false;
            found_first_impl_opening_braket = false;
        }
        if line.find("fn").is_some(){
            // parse first line, extract function name
            let mut tmp_func_info = FuncSignatureSpec::new();
            // if it's struct method, update struct name and possible lifetime parameters annotated already in the start of impl block
            if is_struct_method && tmp_func_info.struct_group_name.is_none(){
                tmp_func_info.struct_group_name = Some(struct_group_name.clone());
                tmp_func_info.lifetime_param = Some(struct_group_lp_vec.clone());

            }
            match parse_one_line_variables(line.clone(), &mut tmp_func_info){
                sig_parse_status::Finished => flag = false,
                sig_parse_status::Processing => flag = true,
                sig_parse_status::Error => panic!("error when parsing function signature, must be incorrect signature format!")
            }
            assert_ne!(tmp_func_info.function_name.len(), 0);
            while flag {
                ln += 1;
                line = lines_buffer[ln].clone();
                match parse_one_line_variables(line.clone(), &mut tmp_func_info){
                    sig_parse_status::Finished => flag = false,
                    sig_parse_status::Processing => flag = true,
                    sig_parse_status::Error => panic!("error when parsing function signature, must be incorrect signature format!")
                }
            }
            source_func_signatures_infos.insert(tmp_func_info.function_name.clone(), tmp_func_info);
            ln += 1;
        }
        else{ ln += 1; continue;}
    }
}

#[derive(Debug)]
pub enum sig_parse_status{
    Finished,
    Processing,
    Error
}
/**
 * This function takes in a line of rust function definition and parse the function name, generic (lifetime) parameters and variable types
 * Required:
            1) if function signature spans multiple lines, a comma is expected at the end of line if the definition is not over.
            2) the return -> must be at the same line of closing right parenthesis of the function signature.
            3) a variable must be present on each line if the definition is not over (including input variable and output variable)
 * For example:
  ```
  fn parse<'i, 'a> (s: &'i String,      // comma must be at the end if the definition is not over
                    t: mut Vec<i32>,
                    p: &'a (u32, BTreeMap<u64, Timeline>)
                    ) -> &'i String     // closing parenthesis and -> must be at the same line
 ```
 */
pub fn parse_one_line_variables(mut line: String, func_info: &mut FuncSignatureSpec) -> sig_parse_status{
    /* erase possible left curly brace and white spaces from both ends */
    line = line.replace("{", "").trim().to_string();
    // println!("input: {}", line);
    if line.find("fn").is_some(){
        if let Some(idx) = line.find('(') {
            let left_part = line.get(0..idx).unwrap(); // ✅
            // contains everything to the left of '(' or '<'
            let mut func_name_str = left_part.clone().trim().to_string();
            // first line, extract possible lifetime parameter (must find "fn" and first left parenthesis)
            if let Some(left_idx) = left_part.find('<'){
                func_name_str = left_part.get(0..left_idx).unwrap().to_string();
                // since has < , then it must has >
                let right_idx = left_part.find('>').unwrap();
                let extracted_content = left_part.get(left_idx+1..right_idx).unwrap();
                let generics: Vec<&str> = extracted_content.split(',').map(|t| t.trim()).collect();
                for elem in generics{
                    if elem.starts_with("'"){
                        if func_info.lifetime_param == None{
                            func_info.lifetime_param = Some(Vec::new())
                        }
                        if let Some(v) = func_info.lifetime_param.as_mut(){
                            v.push(elem.to_string())
                        }
                    }
                }
            } // ✅
            /* parse function name  */
            func_name_str = func_name_str.replace("fn", "").trim().to_string();
  
            func_info.function_name = func_name_str;
            // trim line to contain only the right segment of '('
            line = line.get(idx+1..).unwrap().to_string(); // ✅
            if line.len() == 0{
                return sig_parse_status::Processing;
            }
        }
    }
    // println!("1: {}", line);
    let ret: sig_parse_status;
    // judge whether its the last line
    if is_last_line_of_func_signature(line.clone()){
        ret = sig_parse_status::Finished;
        // find if there are output variable
        if let Some(a_idx) = line.find("->"){
            let output_var_str = line.get(a_idx + 2..).unwrap().trim().to_string();
            func_info.output_variables.push_back(parse_variable_single_cell(output_var_str, true));
            // remove stuff to the right of ->
            line = line.get(0..a_idx).unwrap().to_string();
        }

        let idx = line.rfind(')').unwrap();
            // remove the last ')', preserve the former part for further parsing
        line = line.get(0..idx).unwrap().to_string();
    }
    else{
        ret = sig_parse_status::Processing;
    }

    // if the function has no input parameters, then directly return
    if line.len() == 0{
        return ret;
    }
    // right parenthesis, if any, has been truncated ✅
    // println!("2: {}", line);
    turn_commas_surrounded_by_brackets_to_semicolon(&mut line);
    // println!("3: {}", line);
    let vars: Vec<_> = line.split(',').map(
        |elem| /*mut x: &'i i32 */{
            if elem.len() != 0{
                let tmp = parse_variable_single_cell(elem.to_string(), false);
                func_info.input_variables.push_back(tmp);
            }
        }
    ).collect();

    ret
}

/**
 *  Required: if no_colon is false, input should somehow look like this : " mut x : &'i i32 ";
 *  if no_colon is true, should be like "&'i mut u32", "HashMap<u32; Vec<i32>>" (note the semicolon here)
 *  Now consider struct method: `&self` or `&mut self`
 *
 * */
pub fn parse_variable_single_cell(elem: String, mut no_colon: bool) -> VariableSpec{
    // println!("single cell: {}", elem);
    let mut var_name = String::from("");
    let mut var_lifetime_param: Option<String> = None;
    let mut var_type = String::from("");
    let mut type_cplx = String::from("");
    if elem.find("self").is_some(){
        var_name = elem.trim().to_owned();
        no_colon = true;
    }
    if !no_colon{
        let tmp_vec: Vec<&str> = elem.split(':').map(|v| v.trim()).collect(); /* "mut x", "&'i i32" */ // ✅
        var_name = String::from(tmp_vec[0]);
        type_cplx = String::from(tmp_vec[1]);
    }
    else{
        type_cplx = String::from(elem)
    }
    /* parse variable type */
    if let Some(amp_idx) = type_cplx.find('&'){
        // if has lifetime parameter
        if let Some(tick_idx) = type_cplx.find("'"){
            let tmp = type_cplx.get(tick_idx+1..).unwrap().to_string(); /* everything to the right of 'i */
            let tv: Vec<&str> = tmp.split(' ').map(|x| x.trim()).collect();
            match tv.get(0){
                Some(s) => {
                    var_lifetime_param = Some(s.to_owned().to_owned());
                }, /* lifetime = "i" (tick removed)*/
                None => {}
            }
            // remove the lifetime parameter tick along with itself
            // type_cplx = type_cplx.replace(&("'".to_string() + var_lifetime_param.as_ref().unwrap()) , "");
        }
        // let tv : Vec<&str> = type_cplx.split(' ').map(|x| x.trim()).collect();
        // var_type = tv.join(" ");
        var_type = type_cplx.clone();
    }
    /* mut x: i32*/
    else{
        var_type = type_cplx.to_string();
    }
    turn_semicolon_back_to_commas(&mut var_name);
    turn_semicolon_back_to_commas(&mut var_type);
    if var_name.find("self").is_some(){
        var_type = var_name.clone();
    }
    // lifetime declared in structs need to be considered for special case, such as "Book<'a>"

    let lps_set = extract_wrapped_LP_in_angle_brackets(&var_type);

    // currently doesn't support multiple LP inside one variable type
    if lps_set.len() > 1{
        eprintln!("The current lifetime visualization doesn't support variable struct with multiple lifetimes!");
        exit(0);
    }
    if var_lifetime_param.is_none() && lps_set.len() > 0 {
        var_lifetime_param = Some(lps_set.iter().next().unwrap().to_owned());
    }

    let ret = VariableSpec {  name: var_name.to_string(), lifetime_param: var_lifetime_param, data_type: var_type, lifetime_info: None, hover_messages: Vec::new(), data_hash: None, subordinates : Vec::new(), relationship: String::new()};
    // println!("varSpec: {:?}", ret);
    ret
}

/**
 * With tick removed
 */
fn extract_wrapped_LP_in_angle_brackets(segment: &String) -> HashSet<String>{
    let mut set_lps : HashSet<String> = HashSet::new();
    let pattern = r"<(.*?)>";
    let regex = Regex::new(pattern).unwrap();
    for capture in regex.captures_iter(&segment) {
        if let Some(_) = capture.get(1) {
            // stack, containing only '<'. When seeing one '>', pop the stack and analyze things in between
            let mut stack : Vec<(usize, char)> = Vec::new();
            let mut first_left_ab_found = false;
            // index for previous left angle bracket index, used as minimize the search space
            let mut prev_lab_idx: i32 = -1;
            for (idx, ch) in segment.chars().into_iter().enumerate(){
                // start from leftmost, until found the first left angle bracket
                if ch != '<' && !first_left_ab_found{
                    continue;
                }
                if ch == '<' && !first_left_ab_found{
                    first_left_ab_found = true;
                    stack.push((idx,ch));
                }
                else if ch == '<' && first_left_ab_found{
                    stack.push((idx,ch));
                }
                if first_left_ab_found && ch == '>'{
                    let paired_lab = stack.pop().unwrap();
                    assert!(paired_lab.1 == '<');
                    let mut sub_search_space = String::new();
                    if prev_lab_idx == -1{
                        sub_search_space = segment.get(paired_lab.0+1..idx).unwrap().to_string();
                    }
                    else{
                        assert!(prev_lab_idx >= 0);
                        sub_search_space = segment.get(paired_lab.0+1..prev_lab_idx as usize).unwrap().to_string();
                    }
                    prev_lab_idx = paired_lab.0 as i32;
                    // println!("things inside angle bracket: {}", sub_search_space);
                    // find lifetime parameter inside sub_search_space
                    let sub_search_vec : Vec<&str> = sub_search_space.split(|c| c == ',' || c == ';' || c == ' ' || c == '\t' || c == '\r').collect();
                    for elem in sub_search_vec{
                        // everything behind the tick is part of the lifetime paramter
                        if let Some(bidx) = elem.find('\''){
                            set_lps.insert(elem.get(bidx+1..).unwrap().to_string());
                        }
                    }
                }
            }

        }
    }
    set_lps
}
pub fn is_last_line_of_func_signature(line: String) -> bool{
    let mut sum = 0;
    for ch in line.chars(){
        if ch == '(' { sum += 1}
        else if ch == ')' {sum -= 1}
    }
    if sum < 0{
        true
    }
    else{
        false
    }
}
/**
 * Required: `line` should not contain parenthesis belonging to original function signature
 * this is for complex parsing scenario such as HashMap<i32, u32> as input parameters
 * This function works by calculating tokens for each comma in `line`:
        1) if a comma is surrounded by paired parenthesis or angel brackets, it's token will be 0
        2) otherwise, it's token will be non zero
        3) all comma with zero token will be substituted by semicolon
 */

 #[derive(Debug)]
 struct BracketSpanType{
     start_idx: usize,
     end_idx: usize,
     b_type: String // (), or [], or {}, or <>
 }

impl BracketSpanType{
    fn is_cover(&self, subject_idx: usize) -> bool{
        assert_ne!(self.start_idx, self.end_idx);
        if subject_idx >= self.start_idx && subject_idx <= self.end_idx{
            true
        }
        else{
            false
        }
    }
}
/**
 * Effect: all comma surrounded by full pair of parenthesis or angle bracket will be replaced by semicolon
 */
pub fn turn_commas_surrounded_by_brackets_to_semicolon(line: &mut String){
    let mut angle_stack: Vec<BracketSpanType> = Vec::new();
    let mut paren_stack: Vec<BracketSpanType> = Vec::new();
    let mut brackets_span_collections: Vec<BracketSpanType> = Vec::new();
    for (idx,ch) in line.chars().enumerate(){
        match ch{
            '(' => paren_stack.push(BracketSpanType { start_idx: idx, end_idx: idx , b_type: ch.to_string() }),
            '<' => angle_stack.push(BracketSpanType { start_idx: idx, end_idx: idx, b_type: ch.to_string() }),
            ')' => {
                if let Some(mut top) = paren_stack.pop(){
                    top.end_idx = idx;
                    brackets_span_collections.push(top);
                }
                else{panic!("no '(' but has ')'. Something wrong with line processing!")}
            }
            '>' => {
                if let Some(mut top) = angle_stack.pop(){
                    top.end_idx = idx;
                    brackets_span_collections.push(top);
                }
                else{panic!("no '<' but has '>'. Something wrong with line processing!")}
            }
            _ => {}
        }
    }
    //  println!("{:?}", brackets_span_collections); ✅
    let mut line_tmp_vec : Vec<char> = line.chars().collect();
    // find all commas and their index in `line`
    let re = Regex::new(",").unwrap();
    for mat in re.find_iter(&line){
        for br_span in &brackets_span_collections{
            if br_span.is_cover(mat.start()){
                line_tmp_vec[mat.start()] = ';';
                break;
            }
        }
    }
    *line = line_tmp_vec.into_iter().collect();
}

fn turn_semicolon_back_to_commas(line: &mut String){
    *line = line.replace(";", ",");
}
/**
 * Required: must contain function name and left parenthesis ✅
 */
fn parse_func_name_and_is_method(mut line: String, func_info: &mut FuncSignatureSpec) -> sig_parse_status{
    line = line.trim().to_string();
    if let Some(right_idx) = line.find('('){
        // get rid of left parenthesis
        line = line.get(0..right_idx).unwrap().to_string(); // ✅
        // check if has generic type, trim everything to the right of '<'
        if let Some(g_idx) = line.find('<'){
            // trim '<', the left is `fn funcName`
            line = line.get(0..g_idx).unwrap().to_string();
        }
        // ✅
        // after split: "fn", "funcName". Extract the second field
        let tmp: Vec<&str> = line.split(" ").collect();
        let funcName = tmp[1].to_string();
        // println!("{funcName}");
        // match funcName.find("::"){
        //     Some(_) => func_info.is_method = true,
        //     None => func_info.is_method = false
        // }
        func_info.function_name = funcName;
        return sig_parse_status::Processing;
    }

    sig_parse_status::Error
}
// fn render_Func_line(){}







/*********************
                        Translate Data from Lifetime Parser to Inner Presentation
                                                                                    **********************/
pub fn translate_parser_data_to_function_signature_info(parser_data: &LifetimeVisualization, path_to_source_rs: &String, path_to_main_rs: &String) -> FuncSignatureSpec{
    let mut fs = FuncSignatureSpec::new();
    /* Step 1:
	 *  parse function name by parser data.
	 */
    match parser_data.annotation_type.clone() {
        LifetimeType::Func(func_name) => {
            fs.function_name = func_name;
        },
        LifetimeType::Struct(struct_name) => {
            // split based on :: (e.g, String::new)
            let tmp : Vec<String> = struct_name.split("::").map(|x| x.trim().to_owned()).into_iter().collect();
            let struct_group_name = tmp[0].clone();
            let method_name = tmp[1].clone();
            fs.struct_group_name = Some(struct_group_name);
            fs.function_name = method_name;
        },
        LifetimeType::Var(_) => {todo!()},
        LifetimeType::None => {eprintln!("No annotation!"); exit(0)}
    }

    // println!("step 1\nreplenish parse: {:#?}", fs);
    /* Step 2
     *  Based on function/method name, parse function signature based on function definition found in source.rs.
     *  Note that this step doesn't parse input/output variable names when this function is invoked
     */
     match fs.replenish_parse(path_to_source_rs.clone()){
		Ok(_) =>{},
		Err(err_msg) => {
			eprintln!("{}",err_msg);
			exit(0)
		}
	}
    // println!("step 2\nreplenish parse: {:#?}", fs);
    /* Step 3
     *  Update input variable names, based on where `@Lifetime` syntax is invoked.
     */
    match fs.update_input_names_main_rs(path_to_main_rs.clone(), parser_data){
		Ok(_) =>{},
		Err(err_msg) => {
			eprintln!("{}",err_msg);
			exit(0)
		}
	}
    // println!("update input names parse: {:?}", fs);
	fs.update_output_var_name_and_update_vars_lifetimes(parser_data);
    fs.update_hover_messages_by_parser_data(parser_data);
    println!("final version FS:\n{:#?}", fs);

    // sync variable invoked name with signature name
    fs.sync_var_name_with_invoked_name();
    fs.update_struct_instance_name();
    fs.add_subordinate_to_vars_if_any(parser_data);
    fs
}





