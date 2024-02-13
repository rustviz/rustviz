pub mod lifetime_parse;
pub mod lifetime_render;
pub mod lifetime_render_data_structures;
use crate::data::{ ResourceAccessPoint, LifetimeBind, self};
use crate::hover_messages;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::process::exit;
use handlebars::Handlebars;
use lifetime_parse::*;
use lifetime_render::*;
use lifetime_render_data_structures::*;
use std::{fs, cmp};
use std::io::{BufReader, BufRead};
use regex::*;
use serde::Serialize;
use syn::{parse_file, Item, ItemStruct, Field, Type};

/*********************
						LIFETIME ANNOTATION SPEC (cont.)
															**********************/
/**
 * 1. every variables in the function signature should be annotated with lifetime, even though some of them may not have lifetime parameter
 * 2. for input variables, annotation should start from left to right, in strict order. If this function is invoked on struct instance, then the struct instance should be at the first place.
 * 		For example:
```
  (x,y) = my_instance.create_conv(a,b,c)
```
		Then, annotations for input vars should in order of (my_instance, a, b, c)
 */

/*********************
						Parsed Result from Lifetime Syntax Parser
 																	**********************/
#[derive(Debug, Clone)]
pub struct LifetimeVisualization {
    pub annotation_type: LifetimeType,
    pub input_lives: Vec<Lifetime>,
    pub return_lives: Vec<Lifetime>,
}

#[derive(Debug, Clone)]
pub enum ExtraExplanation{
	NAME(String),   // hover message of variable/reference at top of timeline column
	CRPT(String),   // creation point
	DRPT(String),   // drop point
	BODY(String)    // hover message of solid lifetime line
}

#[derive(Debug, Clone)]
pub enum LifetimeType{
    Var(String),
    Func(String),
    Struct(String),
	None,
}

/**
 * Lifetime struct is meant for each variable that has explicit lifetime annotation
 * `start_line` and `end_line` must be annotated in the syntax, regardless whether this variable is related to the lifetime parameter or not
 */
#[derive(Debug, Clone)]
pub struct Lifetime {
	pub rap: ResourceAccessPoint,
	pub start_line: u64,
	pub end_line: u64,
	pub explanation: Option<Vec<ExtraExplanation>>,
}

#[derive(Debug, Clone)]
pub struct LifetimeStartEndPoint{
	pub start: u32,
	pub end: u32
}


/*********************
						Data Structure for Rendering Lifetime SVG
 																	**********************/

#[derive(Debug, Clone)]
pub struct FuncSignatureSpec{
	/**
	 * Option(struct name)
	 */
	pub struct_group_name: Option<String>,
	pub is_not_static_struct_method: bool,
	/**
	 * pure function name, doesn't have struct name for decoration.
	 * if it belongs to struct method, e.g, `MyStruct::new()` will have function name `new` and struct group name `Some(MyStruct)`
	 */
	pub function_name: String,
	/**
	 * lifetime parameter with their ticks, e.g: 'i, 'a
	 */
	pub lifetime_param: Option<Vec<String>>,
	/**
	 * Names in function definitions
	 */
	pub input_variables: VecDeque<VariableSpec>,
	pub output_variables: VecDeque<VariableSpec>,
	/* indexed the same as input_variables */
	/**
	 * Names when the function is invoked
	 */
	pub input_var_called_names: VecDeque<String>,
	pub output_var_called_names: VecDeque<String>
	// pub is_output_tuple: bool
}

#[derive(Debug, Clone)]
pub struct VariableSpec{
	pub name: String,
	/* lifetime parameter without their ticks, e.g: i, a */
	pub lifetime_param: Option<String>,
	/* if it's a reference, then data_type will contain &. e.g, "&i32" */
	pub data_type: String,
	/* some can do without lifetime parameter  */
	pub lifetime_info: Option<LifetimeStartEndPoint>,
	/* hover message of label, lifetime start point, end point and body */
	pub hover_messages: Vec<ExtraExplanation>,
	/* data hash is used for SVG styling */
	pub data_hash: Option<u32>,
	/* variables that do not show up in function signature however related to this variable.
	For example, instances possessed by container type such as VecDeque */
	pub subordinates: Vec<VariableSpec>,
	/* relationship between master and subordinates */
	pub relationship: String,
}


impl VariableSpec{
	/**
	 * Note: Remember to update variable name after receiving parser output.
	 * Output name will contain lifetime parameter if there is any, for example
	 	* `vec: &'i Vec<String>`
		* `xb: mut T`
	 * This function is mainly used as function signature rendering.
	 * Now includes rendering struct method. If found `self`, then directly render variable name
	 */
	pub fn to_string(&self) -> String{
		let mut ret = String::new();
		/* if it's self, then directly return its original signature */
		if self.name.find("self").is_some() || self.data_type.find("self").is_some(){
			return format!("{}: {}", self.name, self.data_type)
		}
		// if self.data_type.find("&").is_some(){
		// 	match &self.lifetime_param{
		// 		Some(LP) => {
		// 			let mut tmp = self.data_type.clone();
		// 			tmp.insert_str(self.data_type.find("&").unwrap()+1, &("'".to_string() + LP) );
		// 			ret = format!("{}: {}", self.name, tmp);

		// 		},
		// 		None => ret = format!("{}: {}", self.name, self.data_type),
		// 	}
		// }
		// else{
		// 	ret = format!("{}: {}", self.name, self.data_type)
		// }
		ret = format!("{}: {}", self.name, self.data_type);
		ret
	}
}




impl FuncSignatureSpec{

	pub fn new() -> FuncSignatureSpec{
		FuncSignatureSpec {
			struct_group_name: None,
			is_not_static_struct_method: false,
			function_name: String::new(),
			lifetime_param: None,
			input_variables: VecDeque::new(),
			output_variables: VecDeque::new(),
			input_var_called_names: VecDeque::new(),
			output_var_called_names: VecDeque::new(),
		}
	}

	/**
	 * Called when input and output VariableSpecs are set up correctly.
	 * name and called name must be unified to called name in runtime!!!
	 */
	pub fn add_subordinate_to_vars_if_any(&mut self, parser_data: &LifetimeVisualization){
		// deal first with input parameters
		for  input_life  in parser_data.input_lives.iter() {
			match input_life.rap{
				ResourceAccessPoint::LifetimeBind(ref binding) => {
					// go through all input parameters
					for (idx, iv) in self.input_variables.iter_mut().enumerate(){
						assert!(self.input_var_called_names[idx] == iv.name);
						if iv.name == binding.bind_to_name{
							let mut subordinate = FuncSignatureSpec::create_var_spec_using_parser_data(input_life);
							subordinate.lifetime_param = iv.lifetime_param.clone();
							iv.subordinates.push(subordinate);
							iv.relationship = binding.relationship.clone();
						}
					}
				}
				ResourceAccessPoint::LifetimeVars(_) => (),
				_ => {
					eprintln!("Wrong variable definition! Should be related with Lifetime tag!");
					exit(0);
				}
			}
		}
		// deal with output parameters
		for  output_life  in parser_data.return_lives.iter() {
			match output_life.rap{
				ResourceAccessPoint::LifetimeBind(ref binding) => {
					// go through all input parameters
					for (idx, ov) in self.output_variables.iter_mut().enumerate(){
						assert!(self.output_var_called_names[idx] == ov.name);
						if ov.name == binding.bind_to_name{
							let mut subordinate = FuncSignatureSpec::create_var_spec_using_parser_data(output_life);
							subordinate.lifetime_param = ov.lifetime_param.clone();
							ov.subordinates.push(subordinate);
							ov.relationship = binding.relationship.clone();
						}
					}
				}
				ResourceAccessPoint::LifetimeVars(_) => (),
				_ => {
					eprintln!("Wrong variable definition! Should be related with Lifetime tag!");
					exit(0);
				}
			}
		}

	}

	/**
	 * Need to update lifetime param if it has master;
	 * Can elide data type;
	 * Need to update data_hash;
	 */
	fn create_var_spec_using_parser_data(pv: &Lifetime) -> VariableSpec{
		let name = match pv.rap{
			ResourceAccessPoint::LifetimeBind(ref info) => info.name.clone(),
			ResourceAccessPoint::LifetimeVars(ref info) => info.name.clone(),
			_ => {
				eprintln!("Wrong variable definition! Should be related with Lifetime tag!");
				exit(0);
			},
		};
		let lifetime_info = Some(LifetimeStartEndPoint{start: pv.start_line as u32, end: pv.end_line as u32});
		let hover_messages = match pv.explanation{
			Some(ref hvmsg) => hvmsg.clone(),
			None => Vec::new(),
		};
		VariableSpec { name: name, lifetime_param: None, data_type: String::new(), lifetime_info: lifetime_info, hover_messages: hover_messages, data_hash: None, subordinates: Vec::new(), relationship: String::new() }
	}
	/**
	 * overwrite input & output `VariableSpec.name` with [input | output] variable called name
	 */
	pub fn sync_var_name_with_invoked_name(&mut self){
		for (idx, vinfo) in self.input_variables.iter_mut().enumerate() {
			vinfo.name = self.input_var_called_names[idx].clone();
		}
		for (idx, vinfo) in self.output_variables.iter_mut().enumerate() {
			vinfo.name = self.output_var_called_names[idx].clone();
		}
	}
	/**
	 * update `output_var_called_names` from lifetime syntax parsing data structure.
	 * replenish lifetime info for all input and output variables.
	 * Also, update hover messages shown on lifetime column
	 * Now consider struct method ( need to relate struct instance name with `&self`).
	 * If this function is about struct method invoked on struct instance, then the first input variable should be the struct instance:
	 * For example:
	  	```
		let ret = my_instance.clone(); /* ...my_instance(@lifetime info)... */

		```
	 */
	pub fn update_output_var_name_and_update_vars_lifetimes(&mut self, parser_data: &LifetimeVisualization){
		// update output variable names and lifetime
		for (idx, ov) in parser_data.return_lives.iter().enumerate(){
			assert_eq!(self.output_variables[idx].name, String::from(""));
			// variable name (stored in called name vector )
			match &ov.rap{
				ResourceAccessPoint::LifetimeVars(info) => {
					self.output_var_called_names.push_back(info.name.clone())
				},
				ResourceAccessPoint::LifetimeBind(_) => (),
				_ => {
					eprintln!("Wrong variable definition! Should be related with Lifetime tag!");
					exit(0);
				}
			}
			// update lifetime start and end point
			self.output_variables[idx].lifetime_info = Some(LifetimeStartEndPoint { start: ov.start_line as u32, end: ov.end_line as u32 });
			// update hover messages
			if let Some(hover_msgs) = ov.explanation.clone(){
				self.output_variables[idx].hover_messages = hover_msgs;
			}
		}

		// update input variable lifetime
		let mut idx = 0;
		for  iv in parser_data.input_lives.iter(){
			match iv.rap {
				ResourceAccessPoint::LifetimeVars(_) => {
					self.input_variables[idx].lifetime_info = Some(LifetimeStartEndPoint { start: iv.start_line as u32, end: iv.end_line as u32 });
					idx += 1;
				},
				ResourceAccessPoint::LifetimeBind(_) => (),
				_ => {
					eprintln!("Wrong variable definition! Should be related with Lifetime tag!");
					exit(0);
				}
			}
		}
		// if it's struct group, relate self with struct instance
		/*
		 * Actually, we don't need to worry about this since if it's invoked on struct instance, then the function signature parser will
		 * include `&self` at the first position of input variables
		 */
	}
	/** returns (type name, lifetime parameter[empty if none]) */
	fn helper_parse_field_type(field_type: &Type) -> (String, String){
		match field_type {
			Type::Reference(reference) => {
				// Check if the reference has a lifetime
				if let Some(lifetime) = &reference.lifetime {
					let lifetime_name = lifetime.ident.to_string();
					let inner_type = Self::helper_parse_field_type(&*reference.elem);
					( format!("&'{} {}", &lifetime_name, &inner_type.0), lifetime_name)
				} else {
					// If the reference does not have a lifetime, parse the inner type
					Self::helper_parse_field_type(&*reference.elem)
				}
			}
			_ => (quote::quote! { #field_type }.to_string(), String::new()),
		}
	}
	fn parse_struct(&mut self, path_to_source_rs: &String){
		if self.struct_group_name.is_none(){
			eprintln!("struct group name is not set yet!");
			exit(0);
		}
		// Read the source file
		let source_code = fs::read_to_string(path_to_source_rs).expect("Unable to read file");
		// println!("source code: {}", source_code);

		// Parse the source code into a syntax tree
		let syntax_tree = parse_file(&source_code).expect("Unable to parse file");

		// Iterate through the items in the syntax tree
		let mut lp_set: HashSet<String> = HashSet::new();
		for item in syntax_tree.items {
			if let Item::Struct(item_struct) = item {
				// Check if the item is the target struct
				if item_struct.ident.to_string() == self.struct_group_name.clone().unwrap() {
					// Parse fields of the target struct
					for field in item_struct.fields.iter() {
						let field_name = field.ident.as_ref().expect("Field does not have an identifier").to_string();
						let field_type_lp = Self::helper_parse_field_type(&field.ty);
						let field_type = field_type_lp.0;
						let tmp_lifetime_param = if field_type_lp.1.is_empty() { None } else { Some(field_type_lp.1) };
						let var = VariableSpec { name: field_name, lifetime_param: tmp_lifetime_param.clone(), data_type: field_type, lifetime_info: None, hover_messages: Vec::new(), data_hash: None, subordinates: Vec::new(), relationship: String::new() };
						self.input_variables.push_back(var);
						if let Some(lp) = tmp_lifetime_param{
							lp_set.insert(format!("'{}", lp));
						}
					}
					break;
				}
			}
		}
		// update default constructor lifetime parameter(s)
		let lp_vec: Vec<String> = lp_set.iter().map(|x| x.clone()).collect();
		if lp_vec.len() > 0{
			self.lifetime_param = Some(lp_vec.clone());
		}
		// output variable must be the struct instance
		let lp_annotation = lp_vec.join(",");
		let output_var = VariableSpec { name: "".to_string(), lifetime_param: Some(lp_vec[0].clone()[1..].to_string()), data_type: format!("{}<{}>", &self.struct_group_name.clone().unwrap(), lp_annotation), lifetime_info: None, hover_messages: Vec::new(), data_hash: None, subordinates: Vec::new(), relationship: String::new() };
		self.output_variables.push_back(output_var);
	}
	/**
	 * Update function signature based on `self.function_name`, `self.struct_group_name` and `is_not_static_struct_method`, which is given by the parser.
	 * complete function signature struct, except for `input_var_called_names` and `output_var_called_names`. Those two are updated in `update_input_names_main_rs`.
	 * Now has added struct method parsing and determine whether it's static struct method.
	 * `is_not_static_struct_method` is determined based on whether the signature contains `self` keyword
	 */
	pub fn replenish_parse(&mut self, path_to_source_rs: String) -> Result<String, String>{
		let mut found = false;
		if self.function_name.len() == 0{
			return Result::Err("function name unknown! No clue which function signature to extract!".to_string());
		}
		let mut source_func_signatures_infos : BTreeMap<String, FuncSignatureSpec> = BTreeMap::new();
		/* special case for struct default constructor */
		if self.function_name.eq("default_constructor") {
			Self::parse_struct(self, &path_to_source_rs);
			return Ok("update success on struct default constructor".to_string());
		}
		/* parse source file function definitions */
		parse_all_function_signature(&path_to_source_rs, &mut source_func_signatures_infos);
		for (func_name, func_info) in &source_func_signatures_infos{
			if *func_name == self.function_name && func_info.struct_group_name == self.struct_group_name{
				*self = func_info.clone();
				found = true;
			}
		}

		// println!("replenish parse inside\nfs: {:#?}", self);
		/* if output is tuple, then further tear down the tuple structure */
		if self.output_variables.len() > 0 && self.output_variables[0].data_type.find("(").is_some(){
			let orig_tuple = self.output_variables.pop_front().unwrap();
			let mut tuple_str = orig_tuple.data_type;
			// the parsing makes sure the spaces are trimmed
			assert!(tuple_str.chars().nth(0).unwrap() == '(');
			assert!(tuple_str.chars().nth_back(0).unwrap() == ')');
			tuple_str = tuple_str.get(1..tuple_str.len()-1).unwrap().to_string();
			turn_commas_surrounded_by_brackets_to_semicolon(&mut tuple_str);
			// split by comma and parse each field
			for (idx, single_cell_data) in tuple_str.split(",").into_iter().enumerate(){
				let tmp = String::from(single_cell_data).trim().to_string();
				let mut out_var = parse_variable_single_cell(tmp, true);
				if idx == 0 {
					if let Some(lp) = orig_tuple.lifetime_param.clone() {
						out_var.lifetime_param = Some(lp)
					}
				}
				self.output_variables.push_back(out_var);
			}
		}
		if !found{
			Err("no matching function definition in source.rs!".to_string())
		}
		else{
			/* if it's not static struct method, then first element of input variable should be self */
			if let Some(first_iv) = self.input_variables.get(0){
				if first_iv.name.find("self").is_some(){
					self.is_not_static_struct_method = true;
				}
			}
			Ok("update success".to_string())
		}
	}


	pub fn update_hover_messages_by_parser_data(&mut self, parser_data: &LifetimeVisualization){
		// update input var hover messages
		for (idx, vinfo) in parser_data.input_lives.iter().enumerate(){
			if self.is_not_static_struct_method{
				assert!(self.input_variables[0].name.find("self").is_some());
			}
			match vinfo.rap{
				ResourceAccessPoint::LifetimeBind(_) => (),
				ResourceAccessPoint::LifetimeVars(_) => {
					if let Some(hmsgs) = vinfo.explanation.as_ref(){
						self.input_variables[idx].hover_messages = hmsgs.clone();
					}
				}
				_ => {
					eprintln!("Wrong lifetime annotation!");
					exit(0);
				}
			}
		}
		for (idx, ovinfo) in parser_data.return_lives.iter().enumerate(){
			match ovinfo.rap{
				ResourceAccessPoint::LifetimeBind(_) => (),
				ResourceAccessPoint::LifetimeVars(_) => {
					if let Some(hmsgs) = ovinfo.explanation.as_ref(){
						self.output_variables[idx].hover_messages = hmsgs.clone();
					}
				}
				_ => {
					eprintln!("Wrong lifetime annotation!");
					exit(0);
				}
			}

		}
	}
	/**
	 * Only update input variables names!
	 * It will navigate to the line with `Lifetime` keyword and parse that line. So make sure Lifetime annotation is at the right place!!!
	 * Because there might be multiple same function calls within `main`, so only the function with lifetime annotation will be parsed!!!
	 * If there is `self`, then the input variable called name will be exactly the same as `self`
	 */
	pub fn update_input_names_main_rs(&mut self, path_to_main_rs: String,  parser_data: &LifetimeVisualization) -> Result<String, String> {
		if self.function_name.len() == 0{
			return Result::Err("function name unknown! No clue which line in main.rs to match!".to_string());
		}
		// special case: struct default constructor
		if self.function_name == "default_constructor" {
			for input_var_name in parser_data.input_lives.iter(){
				match input_var_name.rap{
					ResourceAccessPoint::LifetimeVars(ref info) => {
						self.input_var_called_names.push_back(info.name.clone());
					},
					ResourceAccessPoint::LifetimeBind(ref info) => {
						self.input_var_called_names.push_back(info.name.clone());
					},
					_ => {
						eprintln!("Wrong variable definition! Should be related with Lifetime tag!");
						exit(0);
					}
				}
			}
			for output_var_name in parser_data.return_lives.iter(){
				match output_var_name.rap{
					ResourceAccessPoint::LifetimeVars(ref info) => {
						self.output_var_called_names.push_back(info.name.clone());
					},
					ResourceAccessPoint::LifetimeBind(ref info) => {
						self.output_var_called_names.push_back(info.name.clone());
					},
					_ => {
						eprintln!("Wrong variable definition! Should be related with Lifetime tag!");
						exit(0);
					}
				}
			}
			return Ok("updated struct default constructor output variables".to_string());
		}
		let file = fs::File::open(&path_to_main_rs).expect((String::from("error opening ") + &path_to_main_rs).as_str());
		let reader = BufReader::new(file);
		let pattern = self.function_name.clone() + r"\((.*?)\)";
		let re: Regex = regex::Regex::new(&pattern ).unwrap();

		let lines_vec : Vec<String>  = reader.lines().map(|ln| {
			match ln {
				Ok(line) => line,
				Err(_) => String::from("")
			}
		}).collect();
		let mut l_idx: usize = 0;
		while l_idx < lines_vec.len(){
			let call_line = &lines_vec[l_idx];
			if re.is_match(call_line){
				// only parse the one with Lifetime annotation
				if call_line.find("Lifetime").is_some(){
					// find if it's struct method
					if self.is_not_static_struct_method == true{
						self.update_first_input_var_if_struct_instance_method(call_line.clone());
					}
					// if call_line.find(format!("{}(", self.function_name).as_str()).is_some(){
					// 	self.update_first_input_var_if_struct_instance_method(call_line.clone());
					// }
					match self.update_self_input_variables(call_line.clone()) {
						Ok(_) => return Ok("".to_string()),
						_ => return Err("failed to update input parameter from main.rs! No Lifetime annotation or no matching function call from Lifetime annotation!".to_string()),
					}
				}
			}
			// special case: struct default constructor
			// if self.function_name == "default_constructor" {
			// 	match self.update_self_input_variables(call_line.clone()) {
			// 		Ok(_) => return Ok("".to_string()),
			// 		_ => return Err("failed to update input parameter from main.rs! No Lifetime annotation or no matching function call from Lifetime annotation!".to_string()),
			// 	}
			// }
			l_idx += 1;
		}
		Err("no matching function definition in source.rs!".to_string())
	}

	/**
	 * If function is static struct method, then it shall have the struct name accompany, such as `Book::new`
	 */
	fn update_first_input_var_if_struct_instance_method(&mut self, call_line:String){

		let mut lnn = call_line.trim().to_string();
		// discard everything before the equal sign
		if let Some(eq_idx) = lnn.find("="){
			lnn = lnn.get(eq_idx+1..).unwrap().trim().to_string();
		}
		// println!("lnn: {}",lnn );
		// find if there is any trace of method calling syntax
		if let Some(dot_idx) = lnn.find(format!(".{}", self.function_name).as_str()){
			// this should have been parsed in replenish_parse() !!!
			assert!(self.is_not_static_struct_method == true);
			self.input_var_called_names.push_back(lnn.get(0..dot_idx).unwrap().to_string());
		}
	}


	fn update_self_input_variables(&mut self, mut line: String) -> Result<String, String>{
		let tmp_vec: Vec<String> = line.split(";").map(|x| x.trim().to_string()).collect();
		// make sure all comments are removed
		line = tmp_vec[0].clone();
		// println!("update_self_input_var: lnn: {}", line);

		// check whether there are output variables in this function signature
		if let Some(eq_idx) = line.find("="){
			/* extract everything to the right side of '=' */
			line = line.get(eq_idx+1..).unwrap().trim().to_string();
		}
		let mut var_str = String::from("");
		let re = Regex::new(r"\((.*?)\)").unwrap();
		if let Some(captures) = re.captures(&line){
			if let Some(capture) = captures.get(1){
				var_str = capture.as_str().to_string();
			}
			for var in var_str.split(",").map(|s| s.trim().to_string()){
				self.input_var_called_names.push_back(var.clone())
			}
			Ok("update success".to_string())
		}
		else{
			Err("no function call or wrong function call!".to_string())
		}

	}

	/**
	 * Required: should be called after input variables has been parsed!!!
	 * if this function is invoked on struct instance, then update struct instance type from `self` to `struct_name.self`
	 */
	pub fn update_struct_instance_name(&mut self) {
		if self.is_not_static_struct_method == false{
			return;
		}
		assert!(self.input_variables.len() > 0, "no input variables added yet is struct method!!!");
		if let Some(struct_gp_name) = self.struct_group_name.clone(){
			let struct_ins = &mut self.input_variables[0];
			struct_ins.data_type = struct_ins.data_type.replace("self", format!("{}.self", struct_gp_name).as_str());
		}
		else{
			eprintln!("no struct group name yet is method invoked on struct instance!!!");
			exit(0)
		}
	}

}
/* helpers */

/**
 * Assign data hash for vars with lifetime parameters
 */
fn assign_hash_to_vars_with_lp(func_info: &mut FuncSignatureSpec) -> Vec<VariableSpec>{
    let mut vars: Vec<VariableSpec> = Vec::new();
    let mut data_hash : u32 = 1;
    for elem in func_info.input_variables.iter_mut(){
        if elem.lifetime_info.is_some(){
            elem.data_hash = Some(data_hash);
            data_hash += 1;
            vars.push(elem.clone());
			for subordinate in elem.subordinates.iter_mut(){
				subordinate.data_hash = Some(data_hash);
				vars.push(subordinate.clone());
				data_hash += 1;
			}
        }
    }
    for elem in func_info.output_variables.iter_mut(){
        if elem.lifetime_info.is_some(){
            elem.data_hash = Some(data_hash);
            data_hash += 1;
            vars.push(elem.clone());
			for subordinate in elem.subordinates.iter_mut(){
				subordinate.data_hash = Some(data_hash);
				vars.push(subordinate.clone());
				data_hash += 1;
			}
        }
    }
    vars
}

fn remove_lifetime_tick(lifetime_param : &mut String){
	assert!(lifetime_param.len() > 0);
	if lifetime_param.chars().nth(0).unwrap() == '\''{
		*lifetime_param = lifetime_param.get(1..).unwrap().to_string();
	}
}


/********************
					 Render SVG - Abstraction Interface
												  		*********************/
/**
 * Input: parser data, path to source.rs and main.rs for function signature rendering
 * Output: (SVG code string, max width, max height)
 */
pub fn render_lifetime_panel(path_to_main_rs: String, path_to_source_rs: String, parser_data: &LifetimeVisualization) -> (String, u32, u32){
	let mut registry = Handlebars::new();
	/*
	 * Parse function/lifetime/variables info
	 */
    let mut fs = translate_parser_data_to_function_signature_info(parser_data, &path_to_source_rs, &path_to_main_rs);
    // println!("func sig info: {:?}", fs);

    // let (width, y_end, func_sig_str) = render_function_lifetime_signature(&fs, &mut registry);
    let (width, y_end, func_sig_str) = render_function_lifetime_signature_lifetime_type(&fs, &mut registry);

	// println!("width sig: {}", width);
	/*
	 * extract `vars: Vec<VariableSpec>` from `fs` and assign data-hash for those have lifetime parameters (i.e., related to lifetime parameter calculation)
	 */
	let vars = assign_hash_to_vars_with_lp(&mut fs);
	// println!("lifetimevis:467\t {:?}", vars);
	// println!("lifetimevis:467\t {:?}", fs);
	for vv in vars.iter(){
		// println!("var: {}, lifetime: {:?}", vv.name, vv.data_hash);
	}
    let mut lifetime_vis_svg_str = func_sig_str.clone();

	let mut tmp = func_sig_str.clone();

    let mut x_begin : u32 = 30;
    // calculate max y val beforehand
    let mut max_y = 0;
    for var in &vars{
        if let Some(lp_info) = &var.lifetime_info{
            max_y = cmp::max(lp_info.end, max_y)
        }
    }
    max_y = CODE_LINE_Y_START + (max_y - 1) * CODE_VERTICAL_LINE_SPACE + 15;
    if let Some(lps) = fs.lifetime_param.clone(){
        for (lifetime_hash,mut lp) in lps.into_iter().enumerate(){
            remove_lifetime_tick(&mut lp);
            let mut var_same_lifetime : Vec<VariableSpec> = Vec::new();
            for v in &vars{
                if let Some(v_lifetime) = v.lifetime_param.clone(){
                    if v_lifetime == lp {
                        var_same_lifetime.push(v.clone());
                    }
                }
            }

            let (w2, column_str) = render_lifetime_columns_one_for_lifetime_parameter(&var_same_lifetime, &registry, x_begin, &lifetime_hash, &max_y);
            x_begin += w2;
			x_begin += 60;
            lifetime_vis_svg_str = lifetime_vis_svg_str + &column_str;
			// println!("column xbegin: {}", x_begin);
            // render lifetime region square
        }
        let dash_line_str = render_dashed_number_line(vars,x_begin, &registry);
        lifetime_vis_svg_str = lifetime_vis_svg_str + &dash_line_str;
    }
	(lifetime_vis_svg_str, cmp::max(x_begin, width), max_y + 100)
	// (tmp, cmp::max(x_begin, width), max_y + 100)
}

