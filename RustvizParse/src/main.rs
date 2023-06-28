// #[macro_use]
extern crate quote;
extern crate syn;
extern crate clap;
extern crate proc_macro2;

// use core::fmt;
// use std::error::String;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use log::{debug};
// use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use clap::{Arg, App};

mod parse;

fn main() {
    // This initializes the env_logger, a logging implementation that reads its configuration from environment variables.
    env_logger::init();
    let matches = App::new("Rustviz Parse")
                        //   .version("1.0")
                        //   .author("Kevin K. <kbknapp@gmail.com>")
                          .about("Parse Owners and Functions")
                          .arg(Arg::with_name("target")
                            //    .short("t")
                            //    .long("target")
                            //    .value_name("FILE")
                               .help("Target file for parsing")
                               .required(true)
                               .takes_value(true))
                        //   .arg(Arg::with_name("INPUT")
                        //        .help("Sets the input file to use")
                        //        .required(true)
                        //        .index(1))
                          .get_matches();
    // Create a file with header and original content
    let mut file_name = PathBuf::from(matches.value_of("target").unwrap());
    // println!("{:?}", FileName);
    let parse_res = parse::parse(&file_name);
    let origin_contents = fs::read_to_string(&file_name);
    file_name.pop();
    file_name.push("main.rs");
    let mut f = File::create(file_name).unwrap();
    f.write_all(parse_res.unwrap().as_bytes());
    f.write_all(origin_contents.unwrap().as_bytes());
}